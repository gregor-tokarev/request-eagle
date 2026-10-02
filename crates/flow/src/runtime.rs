//! Runs a flow. Data moves along connections, and a block runs once every
//! connected input holds a value. A block runs again when one of them
//! receives another, with the latest value of the others.
//!
//! Work is taken from a stack, so what a block sends is handled before the
//! data that was already waiting: each item of a loop reaches the end of the
//! loop before the next item starts. Loops start, and cycles start their
//! next pass, once the work under way is done, so the values they combine
//! with have arrived. Every value remembers the loop iterations and the
//! passes through cycles it was sent in, and a block only combines values of
//! the same iteration or pass. Values from outside a loop or cycle combine
//! with every iteration.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use request::RequestExecutor;
use serde_json::{Map, Value, json};

use crate::graph::{CYCLE, Graph, Target};
use crate::{BlockKind, Flow, SavedRequest, TemplateFormat, http, template};

/// The most blocks one run executes, which stops a loop that never ends.
pub const MAX_BLOCK_RUNS: usize = 100_000;

pub struct RunOptions {
    /// What Start blocks send. Without it, each sends its own input.
    pub input: Option<Value>,
    /// The requests HTTP Request blocks send, by request ID.
    pub requests: HashMap<String, SavedRequest>,
    pub executor: RequestExecutor,
}

#[derive(Clone, Debug)]
pub enum RunEvent {
    /// A block that can take a while, such as an HTTP request, began.
    Started {
        block: String,
    },
    Finished(BlockRun),
    /// A Log block received a value, `at` this long into the run.
    Log {
        block: String,
        value: Arc<Value>,
        at: Duration,
    },
}

/// One time a block ran.
#[derive(Clone, Debug)]
pub struct BlockRun {
    pub block: String,
    pub inputs: Vec<(String, Arc<Value>)>,
    pub outputs: Vec<(String, Arc<Value>)>,
    pub elapsed: Duration,
    /// How long into the run the block finished.
    pub at: Duration,
    /// Why the block failed. It sent nothing.
    pub error: Option<String>,
    /// Something to know about a run that did not fail, such as an
    /// undefined result.
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RunSummary {
    pub block_runs: usize,
    pub failures: usize,
    pub elapsed: Duration,
    /// What each output of the Output blocks received last.
    pub outputs: Map<String, Value>,
    /// Why the run ended before its work did.
    pub stopped: Option<String>,
}

/// Run `flow` to its end, reporting each block's run to `events`. Dropping
/// the future stops the run.
pub async fn run(
    flow: Flow,
    options: RunOptions,
    mut events: impl FnMut(RunEvent) + Send,
) -> RunSummary {
    let started = Instant::now();
    let graph = Graph::new(&flow);
    let mut run = Run {
        started,
        inputs: vec![HashMap::new(); flow.blocks.len()],
        flow: &flow,
        graph,
        options,
        events: &mut events,
        stack: Vec::new(),
        deferred: VecDeque::new(),
        pending: VecDeque::new(),
        variables: HashMap::new(),
        buffers: HashMap::new(),
        expressions: HashMap::new(),
        validators: HashMap::new(),
        next_loop: 0,
        summary: RunSummary::default(),
    };

    // Start blocks run first, then the other blocks that need no input, in
    // the order of the flow.
    let mut sources: Vec<usize> = (0..flow.blocks.len())
        .filter(|&block| {
            flow.blocks[block].kind.is_source() && run.graph.connected[block].is_empty()
        })
        .collect();
    sources.sort_by_key(|&block| !matches!(flow.blocks[block].kind, BlockKind::Start { .. }));
    for &block in sources.iter().rev() {
        run.stack.push(Work::Fire {
            block,
            value: None,
            frames: Frames::default(),
        });
    }

    loop {
        // Once the work under way is done, loops start one at a time, then
        // the next passes through cycles.
        if matches!(run.stack.last(), None | Some(Work::LoopEnd { .. })) {
            if let Some(pending) = run.pending.pop_front() {
                run.begin_loop(pending);
                continue;
            }
            if !run.deferred.is_empty() {
                let deferred: Vec<Work> = run.deferred.drain(..).collect();
                run.stack.extend(deferred.into_iter().rev());
            }
        }
        let Some(work) = run.stack.pop() else {
            break;
        };

        if run.summary.block_runs >= MAX_BLOCK_RUNS {
            run.summary.stopped = Some(format!(
                "Stopped after {MAX_BLOCK_RUNS} block runs. Check for a loop that never ends."
            ));
            break;
        }

        match work {
            Work::Deliver {
                block,
                input,
                packet,
            } => run.deliver(block, input, packet).await,
            Work::Fire {
                block,
                value,
                frames,
            } => {
                let inputs = value
                    .map(|value| vec![("value".to_owned(), value)])
                    .unwrap_or_default();
                run.execute(block, inputs, frames).await;
            }
            Work::LoopEnd {
                loop_run,
                collects,
                frames,
            } => run.end_loop(loop_run, collects, frames),
        }
    }

    let mut summary = run.summary;
    summary.elapsed = started.elapsed();
    summary
}

/// The loop iterations and cycle passes a value was sent in, outermost
/// first: each loop run and the index of its item, or each cycle and the
/// number of its pass.
type Frames = Arc<[(u64, u32)]>;

/// The values a Collect block gathered in one loop run, by iteration.
type Gathered = Vec<(u32, Arc<Value>)>;

/// A loop that starts once the work under way is done.
struct PendingLoop {
    block: usize,
    output: &'static str,
    items: Vec<Value>,
    frames: Frames,
}

#[derive(Clone)]
struct Packet {
    value: Arc<Value>,
    frames: Frames,
}

enum Work {
    Deliver {
        block: usize,
        input: String,
        packet: Packet,
    },
    /// Run a block without new input: a source as the run starts, or a Get
    /// Variable block with the value just stored.
    Fire {
        block: usize,
        value: Option<Arc<Value>>,
        frames: Frames,
    },
    /// Every item of a loop has been handled; its Collect blocks send their lists.
    LoopEnd {
        loop_run: u64,
        collects: Vec<usize>,
        frames: Frames,
    },
}

struct Run<'a, E: FnMut(RunEvent)> {
    started: Instant,
    flow: &'a Flow,
    graph: Graph,
    options: RunOptions,
    events: &'a mut E,
    stack: Vec<Work>,
    /// Work that starts another pass through a cycle, waiting for the
    /// current pass.
    deferred: VecDeque<Work>,
    /// Loops waiting for the work under way to finish.
    pending: VecDeque<PendingLoop>,
    /// The latest value of each connected input of each block.
    inputs: Vec<HashMap<String, Packet>>,
    variables: HashMap<String, Packet>,
    /// What each Collect block gathered in each loop run.
    buffers: HashMap<(usize, u64), Gathered>,
    /// Parsed once a run, by block and expression.
    expressions: HashMap<(usize, usize), Result<fql::Expression, String>>,
    validators: HashMap<usize, Result<jsonschema::Validator, String>>,
    next_loop: u64,
    summary: RunSummary,
}

/// What a block sent, or why it failed.
type Sent = Result<Vec<(String, Value)>, String>;

impl<E: FnMut(RunEvent)> Run<'_, E> {
    async fn deliver(&mut self, block: usize, input: String, packet: Packet) {
        let flow = self.flow;
        match &flow.blocks[block].kind {
            BlockKind::Note { .. } => return,
            // Either input sends on by itself.
            BlockKind::Or => {
                self.execute(block, vec![(input, packet.value)], packet.frames)
                    .await;
                return;
            }
            // Each output keeps its latest value, whatever the others hold.
            BlockKind::Output { .. } => {
                self.summary
                    .outputs
                    .insert(input.clone(), (*packet.value).clone());
                self.summary.block_runs += 1;
                self.finish(
                    block,
                    vec![(input, packet.value)],
                    Vec::new(),
                    Duration::ZERO,
                    Ok(None),
                );
                return;
            }
            BlockKind::Collect => {
                let innermost_loop = packet
                    .frames
                    .iter()
                    .rev()
                    .find(|&&(frame, _)| frame & CYCLE == 0);
                match innermost_loop {
                    Some(&(loop_run, index)) => self
                        .buffers
                        .entry((block, loop_run))
                        .or_default()
                        .push((index, packet.value)),
                    // Outside a loop there is nothing more to wait for.
                    None => {
                        let list = Arc::new(Value::Array(vec![(*packet.value).clone()]));
                        self.send_collected(block, list, packet.frames);
                    }
                }
                return;
            }
            _ => {}
        }

        self.inputs[block].insert(input, packet);

        let connected = &self.graph.connected[block];
        let mut frames: Vec<(u64, u32)> = Vec::new();
        for name in connected {
            let Some(packet) = self.inputs[block].get(name) else {
                return;
            };

            for &(loop_run, index) in packet.frames.iter() {
                match frames.iter().find(|(other, _)| *other == loop_run) {
                    // A value of another iteration of the same loop is stale;
                    // wait for this iteration's.
                    Some(&(_, other)) if other != index => return,
                    Some(_) => {}
                    None => frames.push((loop_run, index)),
                }
            }
        }

        let inputs = connected
            .iter()
            .map(|name| (name.clone(), self.inputs[block][name].value.clone()))
            .collect();
        self.execute(block, inputs, frames.into()).await;
    }

    async fn execute(&mut self, block: usize, inputs: Vec<(String, Arc<Value>)>, frames: Frames) {
        self.summary.block_runs += 1;
        let started = Instant::now();
        let flow = self.flow;
        let kind = &flow.blocks[block].kind;
        let input = |name: &str| {
            inputs
                .iter()
                .find(|(input, _)| input == name)
                .map(|(_, value)| value.clone())
        };
        let mut notice = None;

        let sent: Sent = match kind {
            BlockKind::Start { input } => match &self.options.input {
                Some(value) => Ok(vec![("data".into(), value.clone())]),
                None => inline_value(input).map(|value| vec![("data".into(), value)]),
            },
            BlockKind::HttpRequest { request } => {
                self.send_request(block, request, &inputs, &mut notice)
                    .await
            }
            BlockKind::Evaluate { expression, .. } => {
                match self.evaluate(block, 0, expression, &inputs) {
                    Ok(Some(value)) => Ok(vec![("result".into(), value)]),
                    Ok(None) => {
                        notice = Some("Result is undefined".to_owned());
                        Ok(Vec::new())
                    }
                    Err(error) => Err(error),
                }
            }
            BlockKind::If { condition, .. } => {
                self.evaluate(block, 0, condition, &inputs).map(|result| {
                    let data = input("data")
                        .map(|data| (*data).clone())
                        .unwrap_or_else(|| variables_object(&inputs));
                    let output = if result.as_ref().is_some_and(truthy) {
                        "then"
                    } else {
                        "else"
                    };
                    vec![(output.into(), data)]
                })
            }
            BlockKind::Condition { conditions, .. } => self.route(block, conditions, &inputs),
            BlockKind::Validate { schema } => {
                let data = input("data").unwrap_or_default();
                self.validate(block, schema, &data)
            }
            BlockKind::Delay { milliseconds } => {
                (self.events)(RunEvent::Started {
                    block: flow.blocks[block].id.clone(),
                });
                smol::Timer::after(Duration::from_millis(*milliseconds)).await;
                Ok(vec![(
                    "data".into(),
                    (*input("data").unwrap_or_default()).clone(),
                )])
            }
            BlockKind::Or => Ok(inputs
                .first()
                .map(|(_, value)| vec![("data".into(), (**value).clone())])
                .unwrap_or_default()),
            BlockKind::Repeat => match input("count").as_deref().and_then(Value::as_f64) {
                Some(count) if count >= 0. && count.fract() == 0. => {
                    let count = count as u64;
                    let items = (0..count.min(MAX_BLOCK_RUNS as u64))
                        .map(Value::from)
                        .collect();
                    notice = Some(format!("Sent {count} indexes"));
                    self.start_loop(block, "index", items, &frames);
                    Ok(Vec::new())
                }
                _ => Err("Count must be a whole number of at least 0".to_owned()),
            },
            BlockKind::For => match input("list").as_deref() {
                Some(Value::Array(items)) => {
                    notice = Some(format!(
                        "Sent {} item{}",
                        items.len(),
                        if items.len() == 1 { "" } else { "s" }
                    ));
                    self.start_loop(block, "item", items.clone(), &frames);
                    Ok(Vec::new())
                }
                _ => Err("List must be a list".to_owned()),
            },
            // Collect blocks send when their loop ends; see `end_loop`.
            BlockKind::Collect | BlockKind::Note { .. } | BlockKind::Output { .. } => {
                Ok(Vec::new())
            }
            BlockKind::Display { .. } => Ok(vec![(
                "data".into(),
                (*input("data").unwrap_or_default()).clone(),
            )]),
            BlockKind::Log => {
                (self.events)(RunEvent::Log {
                    block: flow.blocks[block].id.clone(),
                    value: input("data").unwrap_or_default(),
                    at: self.started.elapsed(),
                });
                Ok(Vec::new())
            }
            BlockKind::String { value } => Ok(vec![("value".into(), Value::String(value.clone()))]),
            BlockKind::Number { value } => Ok(vec![("value".into(), number(*value))]),
            BlockKind::Boolean { value } => Ok(vec![("value".into(), Value::Bool(*value))]),
            BlockKind::Null => Ok(vec![("value".into(), Value::Null)]),
            BlockKind::Now => Ok(vec![(
                "value".into(),
                json!(chrono::Utc::now().timestamp_millis()),
            )]),
            BlockKind::Date { value } => {
                date_millis(value).map(|ms| vec![("value".into(), json!(ms))])
            }
            BlockKind::Select { path } => {
                let data = input("data").unwrap_or_default();
                match select(&data, path) {
                    Some(value) => Ok(vec![("value".into(), value.clone())]),
                    None => {
                        notice = Some(format!("Nothing is at {}", path.trim()));
                        Ok(Vec::new())
                    }
                }
            }
            BlockKind::Record { fields } => fields
                .iter()
                .map(|field| {
                    let value = match input(&field.key) {
                        Some(value) => (*value).clone(),
                        None => inline_value(&field.value)?,
                    };
                    Ok((field.key.clone(), value))
                })
                .collect::<Result<Map<_, _>, String>>()
                .map(|record| vec![("record".into(), Value::Object(record))]),
            BlockKind::List { items } => items
                .iter()
                .enumerate()
                .map(|(index, item)| match input(&format!("item{}", index + 1)) {
                    Some(value) => Ok((*value).clone()),
                    None => inline_value(item),
                })
                .collect::<Result<Vec<_>, String>>()
                .map(|list| vec![("list".into(), Value::Array(list))]),
            BlockKind::Template {
                template, format, ..
            } => {
                template::render(template, &variables_object(&inputs)).and_then(|text| match format
                {
                    TemplateFormat::Text => Ok(vec![("result".into(), Value::String(text))]),
                    TemplateFormat::Json => serde_json::from_str(&text)
                        .map(|value| vec![("result".into(), value)])
                        .map_err(|error| format!("The filled template is not JSON: {error}")),
                })
            }
            BlockKind::SetVariable { name } => {
                let value = input("value").unwrap_or_default();
                self.store_variable(name, value, &frames)
                    .map(|()| Vec::new())
            }
            BlockKind::GetVariable { .. } => Ok(vec![(
                "value".into(),
                (*input("value").unwrap_or_default()).clone(),
            )]),
        };

        match sent {
            Ok(outputs) => {
                let outputs: Vec<_> = outputs
                    .into_iter()
                    .map(|(output, value)| (output, Arc::new(value)))
                    .collect();
                self.send(block, &outputs, &frames);
                self.finish(block, inputs, outputs, started.elapsed(), Ok(notice));
            }
            Err(error) => {
                self.finish(block, inputs, Vec::new(), started.elapsed(), Err(error));
            }
        }
    }

    fn finish(
        &mut self,
        block: usize,
        inputs: Vec<(String, Arc<Value>)>,
        outputs: Vec<(String, Arc<Value>)>,
        elapsed: Duration,
        result: Result<Option<String>, String>,
    ) {
        let (error, notice) = match result {
            Ok(notice) => (None, notice),
            Err(error) => {
                self.summary.failures += 1;
                (Some(error), None)
            }
        };

        (self.events)(RunEvent::Finished(BlockRun {
            block: self.flow.blocks[block].id.clone(),
            inputs,
            outputs,
            elapsed,
            at: self.started.elapsed(),
            error,
            notice,
        }));
    }

    /// Queue what a block sent for the inputs it is connected to. The first
    /// output's data is handled first.
    fn send(&mut self, block: usize, outputs: &[(String, Arc<Value>)], frames: &Frames) {
        let frames = self.frames_from(block, frames);
        let mut deliveries = Vec::new();

        for (output, value) in outputs {
            let Some(targets) = self.graph.targets[block].get(output) else {
                continue;
            };
            for target in targets {
                deliveries.push(self.delivery(block, target, value.clone(), &frames));
            }
        }

        self.queue(deliveries);
    }

    /// The work of carrying a value to an input, and whether the connection
    /// leads back to start another pass through a cycle.
    fn delivery(
        &self,
        block: usize,
        target: &Target,
        value: Arc<Value>,
        frames: &Frames,
    ) -> (Work, bool) {
        let frames = match self.graph.cycle[block] {
            Some(cycle) if target.back => next_pass(frames, cycle),
            _ => frames.clone(),
        };

        (
            Work::Deliver {
                block: target.block,
                input: target.input.clone(),
                packet: Packet { value, frames },
            },
            target.back,
        )
    }

    /// Handle deliveries in order, after what they cause. A cycle's next pass
    /// waits until the work of its current pass is done.
    fn queue(&mut self, deliveries: Vec<(Work, bool)>) {
        let mut now = Vec::with_capacity(deliveries.len());
        for (work, back) in deliveries {
            if back {
                self.deferred.push_back(work);
            } else {
                now.push(work);
            }
        }

        self.stack.extend(now.into_iter().rev());
    }

    /// The frames of what a block sends. A block in a cycle marks the pass
    /// its data belongs to, starting at the first.
    fn frames_from(&self, block: usize, frames: &Frames) -> Frames {
        match self.graph.cycle[block] {
            Some(cycle) if !frames.iter().any(|&(frame, _)| frame == cycle) => {
                let mut frames = frames.to_vec();
                frames.push((cycle, 0));
                frames.into()
            }
            _ => frames.clone(),
        }
    }

    async fn send_request(
        &mut self,
        block: usize,
        request: &str,
        inputs: &[(String, Arc<Value>)],
        notice: &mut Option<String>,
    ) -> Sent {
        if request.is_empty() {
            return Err("Choose a request to send".to_owned());
        }
        let saved = self
            .options
            .requests
            .get(request)
            .ok_or("The chosen HTTP request is no longer saved")?;
        let variables = inputs
            .iter()
            .filter(|(name, _)| name != "send")
            .map(|(name, value)| (name.clone(), http::variable_text(value)))
            .collect();

        (self.events)(RunEvent::Started {
            block: self.flow.blocks[block].id.clone(),
        });
        let execution = self
            .options
            .executor
            .execute(saved.request.clone(), saved.variables(variables))
            .await;

        match execution {
            Ok(execution) => {
                let (success, value) = http::response_json(&execution);
                if let Some(error) = execution
                    .scripts
                    .iter()
                    .find_map(|report| report.error.as_ref())
                {
                    *notice = Some(format!("Script error: {error}"));
                }

                Ok(vec![(
                    if success { "success" } else { "fail" }.into(),
                    value,
                )])
            }
            Err(error) => {
                let message = error.to_string();
                *notice = Some(message.clone());
                Ok(vec![("fail".into(), json!({ "error": message }))])
            }
        }
    }

    /// Evaluate the `index`th expression of a block with its variables as
    /// the fields of the input.
    fn evaluate(
        &mut self,
        block: usize,
        index: usize,
        source: &str,
        inputs: &[(String, Arc<Value>)],
    ) -> Result<Option<Value>, String> {
        if source.trim().is_empty() {
            return Err("Write an FQL expression".to_owned());
        }

        let expression = self
            .expressions
            .entry((block, index))
            .or_insert_with(|| fql::Expression::parse(source).map_err(|error| error.to_string()))
            .as_ref()
            .map_err(Clone::clone)?;

        expression
            .evaluate(Some(&variables_object(inputs)), &fql::Bindings::default())
            .map_err(|error| error.to_string())
    }

    fn route(
        &mut self,
        block: usize,
        conditions: &[String],
        inputs: &[(String, Arc<Value>)],
    ) -> Sent {
        for (index, condition) in conditions.iter().enumerate() {
            let result = self
                .evaluate(block, index, condition, inputs)
                .map_err(|error| format!("Condition {}: {error}", index + 1))?;

            if result.as_ref().is_some_and(truthy) {
                return Ok(vec![(
                    format!("condition{}", index + 1),
                    variables_object(inputs),
                )]);
            }
        }

        Ok(vec![("default".into(), variables_object(inputs))])
    }

    fn validate(&mut self, block: usize, schema: &str, data: &Value) -> Sent {
        let validator = self
            .validators
            .entry(block)
            .or_insert_with(|| {
                let schema: Value = serde_json::from_str(schema)
                    .map_err(|error| format!("The schema is not JSON: {error}"))?;

                jsonschema::options()
                    .offline()
                    .should_validate_formats(true)
                    .build(&schema)
                    .map_err(|error| format!("Invalid JSON Schema: {error}"))
            })
            .as_ref()
            .map_err(Clone::clone)?;

        let errors: Vec<Value> = validator
            .iter_errors(data)
            .take(100)
            .map(|error| json!({"path": error.instance_path().as_str(), "message": error.to_string()}))
            .collect();

        if errors.is_empty() {
            Ok(vec![("pass".into(), data.clone())])
        } else {
            Ok(vec![(
                "fail".into(),
                json!({"data": data, "errors": errors}),
            )])
        }
    }

    /// Start a loop once the work under way is done, so values it combines
    /// with, such as constants, have arrived.
    fn start_loop(
        &mut self,
        block: usize,
        output: &'static str,
        items: Vec<Value>,
        frames: &Frames,
    ) {
        self.pending.push_back(PendingLoop {
            block,
            output,
            items,
            frames: frames.clone(),
        });
    }

    /// Send each item from `output` in its own iteration, then let the
    /// loop's Collect blocks send what they gathered.
    fn begin_loop(
        &mut self,
        PendingLoop {
            block,
            output,
            items,
            frames,
        }: PendingLoop,
    ) {
        let frames = &frames;
        let loop_run = self.next_loop;
        self.next_loop += 1;
        self.stack.push(Work::LoopEnd {
            loop_run,
            collects: self.graph.collects[block].clone(),
            frames: frames.clone(),
        });

        let frames = self.frames_from(block, frames);
        let Some(targets) = self.graph.targets[block].get(output) else {
            return;
        };
        let mut deliveries = Vec::with_capacity(items.len() * targets.len());
        for (index, item) in items.into_iter().enumerate() {
            let mut item_frames = frames.to_vec();
            item_frames.push((loop_run, index as u32));
            let item_frames: Frames = item_frames.into();
            let value = Arc::new(item);

            for target in targets {
                deliveries.push(self.delivery(block, target, value.clone(), &item_frames));
            }
        }

        self.queue(deliveries);
    }

    fn end_loop(&mut self, loop_run: u64, mut collects: Vec<usize>, frames: Frames) {
        // A Collect can also gather a loop it does not close, when the
        // loop's items reach it without passing their own Collect.
        for &(block, gathered) in self.buffers.keys() {
            if gathered == loop_run && !collects.contains(&block) {
                collects.push(block);
            }
        }

        for block in collects.into_iter().rev() {
            let mut items = self.buffers.remove(&(block, loop_run)).unwrap_or_default();
            items.sort_by_key(|(index, _)| *index);
            let list = Value::Array(
                items
                    .into_iter()
                    .map(|(_, value)| (*value).clone())
                    .collect(),
            );
            self.send_collected(block, Arc::new(list), frames.clone());
        }
    }

    fn send_collected(&mut self, block: usize, list: Arc<Value>, frames: Frames) {
        self.summary.block_runs += 1;
        let outputs = vec![
            ("list".to_owned(), list),
            ("finish".to_owned(), Arc::new(Value::Bool(true))),
        ];
        self.send(block, &outputs, &frames);
        self.finish(block, Vec::new(), outputs, Duration::ZERO, Ok(None));
    }

    fn store_variable(
        &mut self,
        name: &str,
        value: Arc<Value>,
        frames: &Frames,
    ) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Name the variable".to_owned());
        }

        let packet = Packet {
            value,
            frames: frames.clone(),
        };
        self.variables.insert(name.to_owned(), packet.clone());

        for (block, readers) in self.flow.blocks.iter().enumerate().rev() {
            if matches!(&readers.kind, BlockKind::GetVariable { name: read } if read.trim() == name)
            {
                self.stack.push(Work::Fire {
                    block,
                    value: Some(packet.value.clone()),
                    frames: packet.frames.clone(),
                });
            }
        }

        Ok(())
    }
}

/// The frames of data that goes around `cycle` again.
fn next_pass(frames: &Frames, cycle: u64) -> Frames {
    frames
        .iter()
        .map(|&(frame, pass)| {
            if frame == cycle {
                (frame, pass + 1)
            } else {
                (frame, pass)
            }
        })
        .collect()
}

/// A number as JSON, written without a fraction when it is whole.
fn number(value: f64) -> Value {
    if value.fract() == 0. && value.abs() < 9e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// The inputs as the fields of an object, which FQL and templates read.
fn variables_object(inputs: &[(String, Arc<Value>)]) -> Value {
    Value::Object(
        inputs
            .iter()
            .map(|(name, value)| (name.clone(), (**value).clone()))
            .collect(),
    )
}

/// Whether FQL counts a value as true, as `$boolean` does.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64() != Some(0.),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => items.iter().any(truthy),
        Value::Object(object) => !object.is_empty(),
    }
}

/// A value written into a block: JSON, or text when it is not JSON.
pub(crate) fn inline_value(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }

    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned())))
}

/// The value at a dotted path such as `body.items.0.id`. `items[0]` also
/// works. An empty path selects all of the data.
pub fn select<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim().replace('[', ".").replace(']', "");

    path.split('.')
        .filter(|segment| !segment.is_empty())
        .try_fold(data, |value, segment| match value {
            Value::Object(object) => object.get(segment),
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?),
            _ => None,
        })
}

fn date_millis(text: &str) -> Result<i64, String> {
    let text = text.trim();

    if let Ok(date) = DateTime::parse_from_rfc3339(text) {
        return Ok(date.timestamp_millis());
    }
    if let Ok(date) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(date.and_utc().timestamp_millis());
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d")
        && let Some(midnight) = date.and_hms_opt(0, 0, 0)
    {
        return Ok(midnight.and_utc().timestamp_millis());
    }

    Err(format!(
        "\"{text}\" is not an ISO 8601 date, such as 2024-05-01T09:30:00Z"
    ))
}
