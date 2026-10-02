use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use collection::{Collection, FileEntry};
use environment::{EnvironmentSession, EnvironmentSessions};
use gpui_kit::component::input::InputState;
use gpui_kit::*;
use preferences::Preferences;
use request::{
    CookieJar, Execution, ExecutionError, ExecutionInfo, LocalVariables, Request, RequestExecutor,
    RequestScripts, RequestVariables,
};

use super::data_file::{self, DataRow};
use super::run::{Cursor, Position, RunRequest, RunResult, chosen_request};
use crate::Environments;
use crate::cookies::Cookies;
use crate::response_view::ResponseView;
use crate::variables::read_entries;

/// A request of the run sequence, which runs while it is selected.
#[derive(Clone)]
pub(super) struct SequenceItem {
    pub request: RunRequest,
    pub selected: bool,
}

/// A data file whose rows give each iteration its values.
pub(super) struct DataFile {
    pub name: SharedString,
    pub rows: Arc<[DataRow]>,
}

/// The run configuration's advanced settings, with the defaults Postman's
/// runner shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RunOptions {
    /// Keep each response's headers and body to show after the run.
    pub persist_responses: bool,
    /// Leave out the scripts' console output.
    pub logs_off: bool,
    /// End the run when a request cannot be sent or a script fails.
    pub stop_on_error: bool,
    /// Keep the variables that scripts change for the rest of the session.
    pub keep_variables: bool,
    /// Start with an empty cookie jar.
    pub without_cookies: bool,
    /// Keep the cookies that the run's responses set.
    pub save_cookies: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            persist_responses: true,
            logs_off: false,
            stop_on_error: true,
            keep_variables: true,
            without_cookies: false,
            save_cookies: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RunnerPage {
    Setup,
    Results,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunStatus {
    Running,
    Paused,
    Stopped,
    Complete,
}

/// What a run reads and changes as it sends its requests.
struct RunContext {
    executor: RequestExecutor,
    session: EnvironmentSession,
    collection_values: HashMap<String, String>,
    environment_values: HashMap<String, String>,
    environment_error: Option<String>,
    scripts: Result<RequestScripts, String>,
    /// The run's own jar, and the app's jar that keeps its cookies afterward.
    cookies: Option<(CookieJar, CookieJar)>,
    /// `pm.variables`, which last for the whole run, as in Postman.
    locals: LocalVariables,
}

pub(super) struct Run {
    pub requests: Vec<RunRequest>,
    cursor: Cursor,
    pub results: Vec<RunResult>,
    /// Why iterations ended early, after the index of the result that ended
    /// them, shown with the console log.
    pub notes: Vec<(usize, SharedString)>,
    /// Selected requests that were deleted before the run started.
    pub missing: Vec<SharedString>,
    pub status: RunStatus,
    pub iterations: usize,
    delay: Duration,
    data: Option<Arc<[DataRow]>>,
    pub options: RunOptions,
    pub environment: SharedString,
    pub started_at: chrono::DateTime<chrono::Local>,
    /// Time spent running, leaving out pauses.
    elapsed: Duration,
    resumed_at: Option<Instant>,
    /// Whether a request is being sent.
    sending: bool,
    context: RunContext,
    task: Option<Task<()>>,
}

impl Run {
    pub fn duration(&self) -> Duration {
        self.elapsed
            + self
                .resumed_at
                .map_or(Duration::ZERO, |resumed| resumed.elapsed())
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, RunStatus::Running | RunStatus::Paused)
    }

    /// The iteration's data file row. Iterations past the last row reuse it.
    fn data_row(&self, iteration: usize) -> DataRow {
        self.data
            .as_ref()
            .and_then(|rows| rows.get(iteration).or(rows.last()))
            .cloned()
            .unwrap_or_default()
    }

    fn pause_clock(&mut self) {
        if let Some(resumed) = self.resumed_at.take() {
            self.elapsed += resumed.elapsed();
        }
    }
}

/// A collection's or folder's requests, run one after another in the order
/// the run sequence gives them, as Postman's Collection Runner does.
pub struct CollectionRunner {
    /// The collection or folder whose requests run.
    pub path: PathBuf,
    pub(super) name: SharedString,
    /// The directory of the collection, whose variables and scripts apply.
    pub(super) collection: PathBuf,
    pub(super) collection_name: SharedString,
    pub(super) sequence: Vec<SequenceItem>,
    /// The sequence as the collection orders it, which Reset restores.
    original: Vec<RunRequest>,
    /// gRPC and WebSocket requests, which do not run.
    pub(super) other_protocols: usize,
    pub(super) iterations: Option<Entity<InputState>>,
    pub(super) delay: Option<Entity<InputState>>,
    pub(super) data: Option<DataFile>,
    pub(super) data_error: Option<SharedString>,
    pub(super) options: RunOptions,
    pub(super) advanced: bool,
    pub(super) page: RunnerPage,
    pub(super) run: Option<Run>,
    /// Why the last run could not start.
    pub(super) start_error: Option<SharedString>,
    /// Where the run's results were exported, or why they could not be.
    pub(super) exported: Option<Result<PathBuf, SharedString>>,
    pub(super) results: super::results::ResultsState,
    pub(super) detail: Option<Entity<ResponseView>>,
    pub(super) detail_task: Option<Task<()>>,
    sessions: EnvironmentSessions,
    environments: Entity<Environments>,
    _subscriptions: Vec<Subscription>,
}

impl CollectionRunner {
    /// `requests` are the collection's or folder's saved requests in tree
    /// order, each with the folders that contain it.
    pub fn new(
        path: PathBuf,
        name: SharedString,
        collection: PathBuf,
        requests: impl IntoIterator<Item = (FileEntry, Vec<SharedString>)>,
        sessions: EnvironmentSessions,
        environments: Entity<Environments>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (original, other_protocols) = runnable(requests);
        let subscriptions = vec![
            // The summary names the environment the run uses.
            cx.observe(&environments, |_, _, cx| cx.notify()),
            // A run that ends with its tab still saves its cookies.
            cx.on_release(|this: &mut Self, cx| {
                if let Some((jar, kept)) =
                    this.run.as_mut().and_then(|run| run.context.cookies.take())
                {
                    kept.extend(&jar);
                    Cookies::changed(cx);
                }
            }),
        ];

        Self {
            path,
            name,
            collection_name: file_name(&collection),
            collection,
            sequence: original
                .iter()
                .cloned()
                .map(|request| SequenceItem {
                    request,
                    selected: true,
                })
                .collect(),
            original,
            other_protocols,
            iterations: None,
            delay: None,
            data: None,
            data_error: None,
            options: RunOptions::default(),
            advanced: true,
            page: RunnerPage::Setup,
            run: None,
            start_error: None,
            exported: None,
            results: Default::default(),
            detail: None,
            detail_task: None,
            sessions,
            environments,
            _subscriptions: subscriptions,
        }
    }

    /// The collection's or folder's name, which the tab shows.
    pub fn name(&self) -> &SharedString {
        &self.name
    }

    /// Show the collection's or folder's requests as the sidebar has them
    /// now. Requests keep their place and selection; new ones are added at
    /// the end, selected.
    pub fn refresh(
        &mut self,
        requests: impl IntoIterator<Item = (FileEntry, Vec<SharedString>)>,
        cx: &mut Context<Self>,
    ) {
        let (original, other_protocols) = runnable(requests);
        let mut sequence = self
            .sequence
            .iter()
            .filter_map(|item| {
                let request = original
                    .iter()
                    .find(|request| request.id == item.request.id)?;
                Some(SequenceItem {
                    request: request.clone(),
                    selected: item.selected,
                })
            })
            .collect::<Vec<_>>();
        for request in &original {
            if !sequence.iter().any(|item| item.request.id == request.id) {
                sequence.push(SequenceItem {
                    request: request.clone(),
                    selected: true,
                });
            }
        }

        self.sequence = sequence;
        self.original = original;
        self.other_protocols = other_protocols;
        cx.notify();
    }

    /// Follow a request renamed or moved in the sidebar.
    pub fn relocate_request(
        &mut self,
        id: &str,
        path: &Path,
        name: SharedString,
        folders: Vec<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let requests = self
            .sequence
            .iter_mut()
            .map(|item| &mut item.request)
            .chain(self.original.iter_mut());

        for request in requests.filter(|request| request.id.as_ref() == id) {
            request.path = path.to_path_buf();
            request.name = name.clone();
            request.folders = folders.clone();
        }
        cx.notify();
    }

    /// Follow a collection renamed in the sidebar.
    pub fn relocate(&mut self, previous: &Path, collection: &Path, cx: &mut Context<Self>) {
        let Ok(inner) = self.path.strip_prefix(previous) else {
            return;
        };

        if self.path == previous {
            self.name = file_name(collection);
        }
        self.path = collection.join(inner);
        self.collection = collection.to_path_buf();
        self.collection_name = file_name(collection);

        let moved = |path: &mut PathBuf| {
            if let Ok(inner) = path.strip_prefix(previous) {
                *path = collection.join(inner);
            }
        };
        for item in &mut self.sequence {
            moved(&mut item.request.path);
        }
        for request in &mut self.original {
            moved(&mut request.path);
        }
        cx.notify();
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.iterations_state(window, cx);
        self.delay_state(window, cx);
    }

    pub(super) fn iterations_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.iterations {
            return input.clone();
        }

        let input = number_input("1", 1., window, cx);
        self.iterations = Some(input.clone());

        input
    }

    pub(super) fn delay_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        self.delay
            .get_or_insert_with(|| number_input("0", 0., window, cx))
            .clone()
    }

    /// How many times the sequence runs. An empty or zero field runs it once.
    pub(super) fn iteration_count(&self, cx: &App) -> usize {
        self.iterations
            .as_ref()
            .and_then(|input| input.read(cx).value().trim().parse::<usize>().ok())
            .filter(|&count| count > 0)
            .unwrap_or(1)
    }

    fn delay_value(&self, cx: &App) -> Duration {
        Duration::from_millis(
            self.delay
                .as_ref()
                .and_then(|input| input.read(cx).value().trim().parse().ok())
                .unwrap_or(0),
        )
    }

    pub(super) fn selected_count(&self) -> usize {
        self.sequence.iter().filter(|item| item.selected).count()
    }

    pub(super) fn select_all(&mut self, selected: bool, cx: &mut Context<Self>) {
        for item in &mut self.sequence {
            item.selected = selected;
        }
        cx.notify();
    }

    /// Restore the collection's order with every request selected.
    pub(super) fn reset_sequence(&mut self, cx: &mut Context<Self>) {
        self.sequence = self
            .original
            .iter()
            .cloned()
            .map(|request| SequenceItem {
                request,
                selected: true,
            })
            .collect();
        cx.notify();
    }

    pub(super) fn move_item(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.sequence.len() || to >= self.sequence.len() {
            return;
        }

        let item = self.sequence.remove(from);
        self.sequence.insert(to, item);
        cx.notify();
    }

    /// Choose a CSV or JSON file whose rows give each iteration its values.
    pub(super) fn choose_data_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let read = path.clone();
            let rows = cx
                .background_spawn(async move {
                    std::fs::read(&read)
                        .map_err(|error| format!("The data file cannot be read: {error}"))
                        .and_then(|bytes| data_file::parse(&read, &bytes))
                })
                .await;

            let _ = this.update_in(cx, |this, window, cx| {
                this.set_data_file(&path, rows, window, cx)
            });
        })
        .detach();
    }

    /// Use the file's rows, and run as many iterations as it has, as Postman
    /// does.
    pub(super) fn set_data_file(
        &mut self,
        path: &Path,
        rows: Result<Vec<DataRow>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match rows {
            Ok(rows) => {
                let count = rows.len();
                self.data = Some(DataFile {
                    name: path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                    rows: rows.into(),
                });
                self.data_error = None;
                self.iterations_state(window, cx).update(cx, |input, cx| {
                    input.set_value(count.to_string(), window, cx)
                });
            }
            Err(error) => {
                self.data = None;
                self.data_error = Some(error.into());
            }
        }
        cx.notify();
    }

    pub(super) fn remove_data_file(&mut self, cx: &mut Context<Self>) {
        self.data = None;
        self.data_error = None;
        cx.notify();
    }

    /// Start a run of the selected requests with the run configuration.
    /// Each runs as it is saved now.
    pub(super) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run.as_ref().is_some_and(Run::is_active) {
            return;
        }

        let mut requests = Vec::new();
        let mut missing = Vec::new();
        for item in self.sequence.iter().filter(|item| item.selected) {
            match saved(&item.request, &self.collection) {
                Some(request) => requests.push(request),
                None => missing.push(item.request.name.clone()),
            }
        }
        if requests.is_empty() {
            self.start_error =
                (!missing.is_empty()).then(|| "The selected requests are no longer saved.".into());
            cx.notify();
            return;
        }

        let options = self.options;
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();
        let (environment, environment_path) = {
            let environments = self.environments.read(cx);
            (environments.active().cloned(), environments.active_path())
        };
        let collection_environment = self.collection.join("environment.toml");
        let files = read_entries(&collection_environment).and_then(|collection| {
            let environment = environment_path
                .as_deref()
                .map_or_else(|| Ok(HashMap::new()), read_entries)?;
            Ok((collection, environment))
        });
        let ((collection_values, environment_values), environment_error) = match files {
            Ok(files) => (files, None),
            Err(error) => (Default::default(), Some(error)),
        };

        // Without Keep variable values, the run changes a copy of the session.
        let session = self.sessions.for_path(Some(&collection_environment));
        let session = if options.keep_variables {
            session
        } else {
            session.fork()
        };

        // The run's jar: the app's own, a copy whose changes are dropped, or
        // an empty jar whose cookies the app's jar takes afterward.
        let cookies = preferences.cookie_jar.then(|| Cookies::jar(cx));
        let (jar, kept) = match cookies {
            None => (None, None),
            Some(shared) if options.without_cookies => {
                let jar = CookieJar::new();
                let kept = options.save_cookies.then(|| (jar.clone(), shared));
                (Some(jar), kept)
            }
            Some(shared) if options.save_cookies => (Some(shared), None),
            Some(shared) => (Some(shared.copy()), None),
        };

        let executor = match RequestExecutor::new(&preferences) {
            Ok(executor) => match jar {
                Some(jar) => executor.with_cookie_jar(jar),
                None => executor,
            },
            Err(error) => {
                self.start_error = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };

        let iterations = self.iteration_count(cx);
        self.start_error = None;
        self.exported = None;
        self.run = Some(Run {
            cursor: Cursor::new(requests.len(), iterations),
            requests,
            results: Vec::new(),
            notes: Vec::new(),
            missing,
            status: RunStatus::Running,
            iterations,
            delay: self.delay_value(cx),
            data: self.data.as_ref().map(|data| data.rows.clone()),
            options,
            environment: environment.unwrap_or_else(|| "none".into()),
            started_at: chrono::Local::now(),
            elapsed: Duration::ZERO,
            resumed_at: None,
            sending: false,
            context: RunContext {
                executor,
                session,
                collection_values,
                environment_values,
                environment_error,
                scripts: Collection::load_scripts(&self.collection)
                    .map_err(|error| error.to_string()),
                cookies: kept,
                locals: LocalVariables::default(),
            },
            task: None,
        });
        self.page = RunnerPage::Results;
        self.close_detail(cx);
        self.reset_results(cx);
        self.resume(window, cx);
    }

    /// Run the selected requests again with the same configuration.
    pub(super) fn run_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start(window, cx);
    }

    /// Show the run configuration to start another run.
    pub(super) fn new_run(&mut self, cx: &mut Context<Self>) {
        if self.run.as_ref().is_some_and(Run::is_active) {
            return;
        }

        self.page = RunnerPage::Setup;
        cx.notify();
    }

    pub(super) fn resume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = &mut self.run else {
            return;
        };
        if !matches!(run.status, RunStatus::Running | RunStatus::Paused) {
            return;
        }

        run.status = RunStatus::Running;
        run.resumed_at.get_or_insert_with(Instant::now);

        // A request sent before pausing finishes in the task that sent it,
        // which then goes on.
        if run.sending {
            cx.notify();
            return;
        }

        run.task = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let Ok(Some(delay)) = this.update(cx, |this, _| this.delay()) else {
                    return;
                };
                if !delay.is_zero() {
                    cx.background_executor().timer(delay).await;
                }

                // Pausing during the delay keeps the request from going out.
                let Ok(Some((position, send))) = this.update(cx, |this, cx| this.send(cx)) else {
                    return;
                };
                let result = send.await;

                let Ok(more) = this.update_in(cx, |this, window, cx| {
                    this.record(position, result, window, cx)
                }) else {
                    return;
                };
                if !more {
                    return;
                }
            }
        }));
        cx.notify();
    }

    /// Finish the request being sent, then wait.
    pub(super) fn pause(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = &mut self.run
            && run.status == RunStatus::Running
        {
            run.status = RunStatus::Paused;
            run.pause_clock();
            cx.notify();
        }
    }

    /// End the run, cancelling the request being sent.
    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if self.run.as_ref().is_some_and(Run::is_active) {
            self.finish(RunStatus::Stopped, cx);
        }
    }

    /// How long to wait before the next request, while the run goes on.
    /// Like Postman, the delay comes between requests.
    fn delay(&self) -> Option<Duration> {
        let run = self.run.as_ref()?;
        if run.status != RunStatus::Running || run.cursor.next().is_none() {
            return None;
        }

        Some(if run.results.is_empty() {
            Duration::ZERO
        } else {
            run.delay
        })
    }

    fn send(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(Position, Task<Result<Execution, ExecutionError>>)> {
        let run = self.run.as_mut()?;
        if run.status != RunStatus::Running {
            return None;
        }

        let position = run.cursor.next()?;
        let request = &run.requests[position.index];
        let context = &run.context;
        let variables = RequestVariables::with_environment_session(
            context.collection_values.clone(),
            context.environment_values.clone(),
            context.environment_error.clone(),
            context.session.clone(),
        )
        .with_collection_scripts(context.scripts.clone())
        .with_local_variables(context.locals.clone())
        .with_iteration_data(run.data_row(position.iteration))
        .with_info(ExecutionInfo {
            request_name: request.name.to_string(),
            request_id: request.id.to_string(),
            iteration: position.iteration,
            iteration_count: run.iterations,
        });
        let send = context.executor.execute(request.request.clone(), variables);
        run.sending = true;

        Some((position, cx.background_spawn(send)))
    }

    /// Keep a request's result and move on. Returns whether the run goes on.
    fn record(
        &mut self,
        position: Position,
        result: Result<Execution, ExecutionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        run.sending = false;

        let result = RunResult::new(
            position,
            result,
            run.options.persist_responses,
            !run.options.logs_off,
        );
        if result.is_error() && run.options.stop_on_error {
            run.cursor.finish();
        } else if let Some(note) = run
            .cursor
            .advance(chosen_request(&result.scripts), &run.requests)
        {
            run.notes.push((run.results.len(), note.into()));
        }
        run.results.push(result);
        let more = run.cursor.next().is_some();
        let running = run.status == RunStatus::Running;

        self.result_added(cx);
        // Responses may have set cookies, and scripts changed variables that
        // other tabs show.
        Cookies::changed(cx);
        window.refresh();

        if !more {
            self.finish(RunStatus::Complete, cx);
            return false;
        }

        cx.notify();
        running
    }

    fn finish(&mut self, status: RunStatus, cx: &mut Context<Self>) {
        let Some(run) = &mut self.run else {
            return;
        };

        run.status = status;
        run.sending = false;
        run.pause_clock();
        // Ending the task cancels a request that is being sent.
        run.task = None;

        if let Some((jar, kept)) = run.context.cookies.take() {
            kept.extend(&jar);
        }
        Cookies::changed(cx);
        cx.notify();
    }
}

/// The HTTP requests that run, and how many requests of other protocols
/// are left out.
fn runnable(
    requests: impl IntoIterator<Item = (FileEntry, Vec<SharedString>)>,
) -> (Vec<RunRequest>, usize) {
    let mut other_protocols = 0;
    let requests = requests
        .into_iter()
        .filter_map(|(file, folders)| match file.request {
            Request::Http(request) => Some(RunRequest {
                path: file.path,
                id: file.id.into(),
                name: file.name.into(),
                folders,
                request,
            }),
            _ => {
                other_protocols += 1;
                None
            }
        })
        .collect();

    (requests, other_protocols)
}

/// The request as its file holds it now, with its files resolved from the
/// collection. None once it is no longer saved as this HTTP request.
fn saved(request: &RunRequest, collection: &Path) -> Option<RunRequest> {
    let Ok(FileEntry {
        id,
        name,
        request: Request::Http(saved),
        ..
    }) = FileEntry::from_path(&request.path)
    else {
        return None;
    };
    if id != request.id.as_ref() {
        return None;
    }

    Some(RunRequest {
        name: name.into(),
        request: match Request::Http(saved).resolved_from(collection) {
            Request::Http(request) => request,
            _ => unreachable!("resolving keeps the protocol"),
        },
        ..request.clone()
    })
}

fn file_name(path: &Path) -> SharedString {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
        .into()
}

fn number_input(
    value: &str,
    min: f64,
    window: &mut Window,
    cx: &mut Context<CollectionRunner>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value)
            .validate(|value, _| value.bytes().all(|byte| byte.is_ascii_digit()))
            .min(min)
    })
}

impl Render for CollectionRunner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            RunnerPage::Setup => self.setup(window, cx),
            RunnerPage::Results => self.results_page(cx),
        }
    }
}
