use anyhow::{Context as _, Result, anyhow, bail};
use collection::{CollectionRegistry, Entry};
use flow::{BlockKind, BlockType, Flow, RunEvent, RunOptions, SavedRequest};
use request::{Request, RequestExecutor};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::collections::{load, lock_for_edit};
use crate::commands::Command;

/// The most Log values a run returns. Later ones are counted, not returned.
const MAX_LOGS: usize = 1000;

pub async fn dispatch(
    root: &Path,
    preferences: &preferences::PreferencesFile,
    cookies: &Path,
    command: Command,
) -> Result<Value> {
    match command {
        Command::FlowsBlocks {} => Ok(blocks()),
        Command::FqlEvaluate {
            expression,
            input,
            bindings,
        } => evaluate(&expression, input, bindings),
        Command::FlowsList { collection, query } => {
            let registry = load(root)?;
            if let Some(path) = &collection
                && !registry
                    .collections()
                    .iter()
                    .any(|entry| entry.path == *path)
            {
                bail!("Unknown collection path");
            }

            let query = query.to_lowercase();
            let mut output = Vec::new();
            for entry in registry
                .collections()
                .iter()
                .filter(|entry| collection.as_ref().is_none_or(|path| *path == entry.path))
            {
                list_flows(&entry.entries, &entry.path, &query, &mut output);
            }
            Ok(json!(output))
        }
        Command::FlowsGet { path } => {
            let registry = load(root)?;
            flow_json(&registry, &path)
        }
        Command::FlowsCreate { parent, name, flow } => {
            let _lock = lock_for_edit(root)?;
            let mut registry = load(root)?;
            let flow = flow.unwrap_or_else(Flow::starter);
            flow.check().map_err(|error| anyhow!(error))?;
            let path = registry.create_flow(&parent, &name, flow)?;
            flow_json(&registry, &path)
        }
        Command::FlowsUpdate {
            path,
            expected_id,
            flow,
        } => {
            let _lock = lock_for_edit(root)?;
            let mut registry = load(root)?;
            flow.check().map_err(|error| anyhow!(error))?;
            registry.update_flow(&path, &expected_id, flow)?;
            flow_json(&registry, &path)
        }
        Command::FlowsRun {
            path,
            input,
            trust_scripts,
            variables,
            timeout_ms,
        } => {
            run(
                root,
                preferences,
                cookies,
                &path,
                input,
                trust_scripts,
                variables,
                timeout_ms,
            )
            .await
        }
        _ => bail!("Expected a flow operation"),
    }
}

fn list_flows(items: &[Entry], collection: &Path, query: &str, output: &mut Vec<Value>) {
    for entry in items {
        match entry {
            Entry::Directory(folder) => list_flows(&folder.entries, collection, query, output),
            Entry::Flow(flow) => {
                if query.is_empty() || flow.name.to_lowercase().contains(query) {
                    output.push(json!({
                        "path": flow.path, "id": flow.id, "name": flow.name,
                        "blocks": flow.flow.blocks.len(), "collection": collection,
                    }));
                }
            }
            Entry::File(_) => {}
        }
    }
}

/// The flow with what an agent needs to connect its HTTP Request blocks:
/// each request's name, URL and the variables that are its inputs.
fn flow_json(registry: &CollectionRegistry, path: &Path) -> Result<Value> {
    let entry = registry.flow(path).context("Unknown saved flow path")?;
    let mut requests = Map::new();

    for block in &entry.flow.blocks {
        if let BlockKind::HttpRequest { request: id } = &block.kind
            && let Some((_, file)) = registry.request_by_id(id)
        {
            let value = match &file.request {
                Request::Http(request) => json!({
                    "name": file.name, "path": file.path, "method": request.method, "url": request.path,
                    "variables": flow::request_variables(request),
                }),
                _ => {
                    json!({"name": file.name, "path": file.path, "error": "Flows send HTTP requests only"})
                }
            };
            requests.insert(id.clone(), value);
        }
    }

    Ok(json!({
        "path": entry.path, "id": entry.id, "name": entry.name,
        "flow": entry.flow, "requests": requests,
    }))
}

fn blocks() -> Value {
    let blocks: Vec<Value> = BlockType::ALL
        .iter()
        .map(|block_type| {
            let kind = block_type.block_kind();
            let mut settings = serde_json::to_value(&kind).unwrap_or_default();
            let name = settings
                .as_object_mut()
                .and_then(|settings| settings.remove("type"))
                .unwrap_or_default();

            json!({
                "type": name,
                "name": block_type.name(),
                "description": block_type.description(),
                "inputs": kind.inputs(),
                "outputs": kind.outputs(),
                "defaults": settings,
                "runs_at_start": kind.is_source(),
            })
        })
        .collect();

    json!({
        "blocks": blocks,
        "notes": [
            "A block runs when every connected input has a value, and again whenever one receives another. Blocks that run at start do so when none of their inputs are connected.",
            "An http_request block also has an input for each {{variable}} of its request (flows.get lists them). Its send input is an optional trigger, which also fills a {{send}} variable. It sends {body, http: {status, headers, time}, tests, binary} from success for 2xx statuses and from fail otherwise.",
            "evaluate, if and condition expressions are FQL (JSONata); their variables are fields of the input, so write `value1.body.id`, not `$value1`.",
            "for and repeat send each item in its own iteration; a collect block downstream gathers the iteration results into a list once the loop ends.",
            "Variables of condition become outputs condition1, condition2, …; list items become inputs item1, item2, ….",
        ],
    })
}

fn evaluate(source: &str, input: Option<Value>, values: HashMap<String, Value>) -> Result<Value> {
    let mut bindings = fql::Bindings::default();
    for (name, value) in values {
        bindings.insert(name, value);
    }

    let expression = fql::Expression::parse(source).map_err(|error| anyhow!("{error}"))?;
    let result = expression
        .evaluate(input.as_ref(), &bindings)
        .map_err(|error| anyhow!("{error}"))?;

    Ok(match result {
        Some(result) => json!({"defined": true, "result": result}),
        None => json!({"defined": false, "result": null}),
    })
}

/// The last run of a block.
#[derive(Default)]
struct BlockState {
    runs: usize,
    run: Option<flow::BlockRun>,
}

/// What a run reported so far: each block's latest run and the first logs.
/// Earlier runs are dropped as they are replaced, so long loops keep only
/// what the result shows.
#[derive(Default)]
struct Report {
    states: HashMap<String, BlockState>,
    logs: Vec<Value>,
    logged: usize,
    outputs: Map<String, Value>,
}

impl Report {
    fn record(&mut self, flow: &Flow, event: RunEvent) {
        match event {
            RunEvent::Started { .. } => {}
            RunEvent::Log { block, value, at } => {
                self.logged += 1;
                if self.logs.len() < MAX_LOGS {
                    self.logs.push(
                        json!({"block": block, "value": value, "at_ms": at.as_secs_f64() * 1000.}),
                    );
                }
            }
            RunEvent::Finished(run) => {
                if matches!(
                    flow.block(&run.block).map(|block| &block.kind),
                    Some(BlockKind::Output { .. })
                ) {
                    for (name, value) in &run.inputs {
                        self.outputs.insert(name.clone(), (**value).clone());
                    }
                }

                let state = self.states.entry(run.block.clone()).or_default();
                state.runs += 1;
                state.run = Some(run);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    root: &Path,
    preferences: &preferences::PreferencesFile,
    cookies: &Path,
    path: &Path,
    input: Option<Value>,
    trust_scripts: bool,
    variables: HashMap<String, String>,
    timeout_ms: Option<u64>,
) -> Result<Value> {
    let registry = load(root)?;
    let entry = registry.flow(path).context("Unknown saved flow path")?;
    entry.flow.check().map_err(|error| anyhow!(error))?;

    let mut requests = HashMap::new();
    let mut sessions: HashMap<PathBuf, environment::EnvironmentSession> = HashMap::new();
    let mut scripted = false;
    for block in &entry.flow.blocks {
        let BlockKind::HttpRequest { request: id } = &block.kind else {
            continue;
        };
        // The run reports a missing request on its block.
        let Some((collection, file)) = registry.request_by_id(id) else {
            continue;
        };
        let Request::Http(request) = &file.request else {
            bail!(
                "Block \"{}\" sends \"{}\", which is not an HTTP request; flows send HTTP requests",
                block.id,
                file.name
            );
        };

        scripted |= !request.scripts.is_empty() || !collection.scripts().is_empty();
        let mut request = request.clone();
        request.body = request
            .body
            .map(|body| body.resolved_from(&collection.path));
        requests.insert(
            id.clone(),
            SavedRequest {
                name: file.name.clone(),
                request,
                collection: collection.local_env().entries.clone(),
                // Variables passed to the command override the collection's,
                // like an active environment.
                environment: variables.clone(),
                variables_error: None,
                scripts: Ok(collection.scripts().clone()),
                auth: collection.auth().clone(),
                session: sessions.entry(collection.path.clone()).or_default().clone(),
            },
        );
    }
    if scripted && !trust_scripts {
        bail!(
            "Read the scripts of the flow's requests and of their collections, then set trust_scripts=true to approve this run"
        );
    }

    let settings = preferences.request_preferences().await?;
    let jar = crate::execution::cookie_jar(cookies, &settings)?;
    let mut executor = RequestExecutor::new(&settings)?;
    if let Some(jar) = &jar {
        executor = executor.with_cookie_jar(jar.clone());
    }

    let options = RunOptions {
        input,
        requests,
        executor,
    };
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(300_000));
    let mut report = Report::default();
    let summary = smol::future::or(
        async {
            let flow = entry.flow.clone();
            Some(flow::run(flow, options, |event| report.record(&entry.flow, event)).await)
        },
        async {
            smol::Timer::after(timeout).await;
            None
        },
    )
    .await;
    crate::execution::save(jar.as_ref())?;

    Ok(run_json(path, &entry.flow, summary, report, timeout))
}

fn run_json(
    path: &Path,
    flow: &Flow,
    summary: Option<flow::RunSummary>,
    mut report: Report,
    timeout: Duration,
) -> Value {
    let values = |values: &[(String, Arc<Value>)]| {
        values
            .iter()
            .map(|(name, value)| (name.clone(), (**value).clone()))
            .collect::<Map<_, _>>()
    };
    let blocks: Vec<Value> = flow
        .blocks
        .iter()
        .map(|block| {
            let state = report.states.remove(&block.id).unwrap_or_default();
            let run = state.run.as_ref();

            json!({
                "id": block.id,
                "type": serde_json::to_value(&block.kind).ok().and_then(|kind| kind.get("type").cloned()),
                "title": block.title(),
                "runs": state.runs,
                "elapsed_ms": run.map(|run| run.elapsed.as_secs_f64() * 1000.),
                "inputs": run.map(|run| values(&run.inputs)),
                "outputs": run.map(|run| values(&run.outputs)),
                "error": run.and_then(|run| run.error.clone()),
                "notice": run.and_then(|run| run.notice.clone()),
            })
        })
        .collect();

    let (status, stopped, block_runs, failures, elapsed) = match &summary {
        Some(summary) => (
            if summary.stopped.is_some() {
                "stopped"
            } else if summary.failures > 0 {
                "failed"
            } else {
                "succeeded"
            },
            summary.stopped.clone(),
            summary.block_runs,
            summary.failures,
            summary.elapsed,
        ),
        None => (
            "stopped",
            Some(format!(
                "Stopped after {} ms; set timeout_ms to wait longer",
                timeout.as_millis()
            )),
            blocks
                .iter()
                .map(|block| block["runs"].as_u64().unwrap_or(0) as usize)
                .sum(),
            blocks
                .iter()
                .filter(|block| !block["error"].is_null())
                .count(),
            timeout,
        ),
    };

    json!({
        "path": path,
        "status": status,
        "stopped": stopped,
        "outputs": summary.map(|summary| summary.outputs).unwrap_or(report.outputs),
        "elapsed_ms": elapsed.as_secs_f64() * 1000.,
        "block_runs": block_runs,
        "failures": failures,
        "blocks": blocks,
        "logs": report.logs,
        "logs_truncated": report.logged > MAX_LOGS,
    })
}
