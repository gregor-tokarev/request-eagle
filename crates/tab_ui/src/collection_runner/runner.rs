use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use collection::{Collection, Collections, FileEntry, SharedSettings};
use environment::{EnvironmentSession, EnvironmentSessions};
use gpui_kit::component::input::InputState;
use gpui_kit::*;
use preferences::Preferences;
use request::{
    Auth, CookieJar, Execution, ExecutionFailure, ExecutionInfo, LocalVariables, Request,
    RequestExecutor, RequestScripts, RequestVariables,
};

use super::data_file::{self, DataRow};
use super::run::{
    Cursor, KEPT_BODY_LIMIT, Position, RunRequest, RunResult, Totals, chosen_request,
};
use crate::Environments;
use crate::cookies::Cookies;
use crate::response_view::ResponseView;
use crate::variables::read_entries;

/// A request of the run sequence, which runs while it is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// What requests that inherit their authorization send.
    auth: Auth,
    /// The run's own jar, and the app's jar that keeps its cookies afterward.
    cookies: Option<(CookieJar, CookieJar)>,
    /// `pm.variables`, which last for the whole run, as in Postman.
    locals: LocalVariables,
}

pub(super) struct Run {
    pub requests: Vec<RunRequest>,
    cursor: Cursor,
    pub results: Vec<RunResult>,
    pub totals: Totals,
    /// How many more bytes of response bodies the run keeps.
    kept_bytes: usize,
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
    /// The environment the run used, if any.
    pub environment: Option<SharedString>,
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

    /// The request being sent now.
    pub fn in_flight(&self) -> Option<&RunRequest> {
        self.sending
            .then(|| self.cursor.next())
            .flatten()
            .map(|position| &self.requests[position.index])
    }

    /// The iteration being sent, or the last one once the run ends.
    pub fn current_iteration(&self) -> usize {
        self.cursor
            .next()
            .or_else(|| self.results.last().map(|result| result.position))
            .map_or(0, |position| position.iteration)
    }

    /// How many requests the run sends when no script chooses another.
    pub fn planned(&self) -> usize {
        self.requests.len() * self.iterations
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

/// A run whose selected requests and collection files are being read off the
/// UI thread before execution starts.
pub(super) struct Preparing {
    pub selected: Vec<RunRequest>,
    pub collection: PathBuf,
    /// Collection variables and shared settings captured when the run started,
    /// kept if the collection is deleted in the app while preparing.
    pub collection_values: Values,
    pub collection_settings: SharedSettings,
    pub environment: Option<SharedString>,
    pub environment_path: Option<PathBuf>,
    /// Whether the collection or a selected request moved while the current
    /// background read was in flight.
    pub relocated: bool,
    /// Whether the collection itself was deleted from disk in the app while
    /// preparing.
    pub collection_removed: bool,
    /// Indices into `selected` for requests whose file existed on disk right
    /// before an explicit in-app deletion while preparing.
    pub removed: HashSet<usize>,
    /// Indices into `selected` whose paths were updated by an explicit in-app
    /// move or rename while preparing, kept out of ID-only fallback matching.
    pub explicitly_moved: HashSet<usize>,
    task: Option<Task<()>>,
}

impl Preparing {
    pub(super) fn new(
        selected: Vec<RunRequest>,
        collection: PathBuf,
        collection_values: Values,
        collection_settings: SharedSettings,
        environment: Option<SharedString>,
        environment_path: Option<PathBuf>,
    ) -> Self {
        Self {
            selected,
            collection,
            collection_values,
            collection_settings,
            environment,
            environment_path,
            relocated: false,
            collection_removed: false,
            removed: HashSet::new(),
            explicitly_moved: HashSet::new(),
            task: None,
        }
    }

    pub(super) fn relocate(
        &mut self,
        previous: &Path,
        path: &Path,
        collection: &Path,
        inputs: Option<(Values, SharedSettings)>,
    ) {
        if self.collection != collection {
            self.collection = collection.to_path_buf();
            self.collection_removed = false;
            if let Some((values, settings)) = inputs {
                self.collection_values = values;
                self.collection_settings = settings;
            }
            self.relocated = true;
        }

        self.relocate_paths(previous, path);
    }

    pub(super) fn relocate_paths(&mut self, previous: &Path, path: &Path) {
        for (index, request) in self.selected.iter_mut().enumerate() {
            if let Some(new_path) = rebase_moved_path(&request.path, previous, path)
                && new_path != request.path
            {
                request.path = new_path;
                self.explicitly_moved.insert(index);
                self.relocated = true;
            }
        }
    }

    pub(super) fn note_deleted(
        &mut self,
        deleted_path: &Path,
        removed_files: &HashSet<PathBuf>,
        detached: bool,
    ) {
        if !detached && self.collection.starts_with(deleted_path) {
            self.collection_removed = true;
        }

        for (index, request) in self.selected.iter().enumerate() {
            if removed_files.contains(&request.path) {
                self.removed.insert(index);
            }
        }
    }

    pub(super) fn follow_refresh(
        &mut self,
        previous_sequence: &[SequenceItem],
        original: &[RunRequest],
    ) {
        let by_path_and_id: HashSet<(&Path, &str)> = original
            .iter()
            .map(|request| (request.path.as_path(), request.id.as_ref()))
            .collect();
        let previous_paths: HashSet<&Path> = previous_sequence
            .iter()
            .map(|item| item.request.path.as_path())
            .collect();
        let mut previous_id_counts: HashMap<&str, usize> = HashMap::new();
        for item in previous_sequence {
            *previous_id_counts
                .entry(item.request.id.as_ref())
                .or_default() += 1;
        }
        let mut original_id_counts: HashMap<&str, usize> = HashMap::new();
        for request in original {
            *original_id_counts.entry(request.id.as_ref()).or_default() += 1;
        }
        let unique_by_id: HashMap<&str, &RunRequest> = original
            .iter()
            .filter(|request| original_id_counts.get(request.id.as_ref()) == Some(&1))
            .map(|request| (request.id.as_ref(), request))
            .collect();

        for (index, request) in self.selected.iter_mut().enumerate() {
            if self.removed.contains(&index)
                || self.explicitly_moved.contains(&index)
                || by_path_and_id.contains(&(request.path.as_path(), request.id.as_ref()))
            {
                continue;
            }

            let id = request.id.as_ref();
            if previous_id_counts.get(id) == Some(&1)
                && let Some(latest) = unique_by_id.get(id)
                && latest.path != request.path
                && !previous_paths.contains(latest.path.as_path())
            {
                request.path = latest.path.clone();
                request.name = latest.name.clone();
                request.folders = latest.folders.clone();
                self.relocated = true;
            }
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
    /// Set once the collection or folder is deleted while a run is active, so
    /// a replacement created at the same path opens a fresh runner.
    pub(super) detached: bool,
    pub(super) sequence: Vec<SequenceItem>,
    pub(super) sequence_scroll: UniformListScrollHandle,
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
    /// Reads the saved requests before a run starts.
    pub(super) preparing: Option<Preparing>,
    /// Why the last run could not start.
    pub(super) start_error: Option<SharedString>,
    /// Where the run's results were exported, or why they could not be.
    pub(super) exported: Option<Result<PathBuf, SharedString>>,
    pub(super) results: super::results::ResultsState,
    pub(super) detail: Option<Entity<ResponseView>>,
    pub(super) detail_task: Option<Task<()>>,
    /// Whether a result without a kept response shows its console rather
    /// than its tests.
    pub(super) detail_console: bool,
    collections: Entity<Collections>,
    sessions: EnvironmentSessions,
    environments: Entity<Environments>,
    _subscriptions: Vec<Subscription>,
}

impl CollectionRunner {
    /// `requests` are the collection's or folder's saved requests in tree
    /// order, each with the folders that contain it.
    pub fn new(
        path: PathBuf,
        collection: PathBuf,
        requests: impl IntoIterator<Item = (FileEntry, Vec<SharedString>)>,
        collections: Entity<Collections>,
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
            name: file_name(&path),
            path,
            collection_name: file_name(&collection),
            collection,
            detached: false,
            sequence: original
                .iter()
                .cloned()
                .map(|request| SequenceItem {
                    request,
                    selected: true,
                })
                .collect(),
            sequence_scroll: UniformListScrollHandle::new(),
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
            preparing: None,
            start_error: None,
            exported: None,
            results: Default::default(),
            detail: None,
            detail_task: None,
            detail_console: false,
            collections,
            sessions,
            environments,
            _subscriptions: subscriptions,
        }
    }

    /// The collection's or folder's name, which the tab shows.
    pub fn name(&self) -> &SharedString {
        &self.name
    }

    /// The collection's name, followed by the folder's when a folder runs.
    pub(super) fn title(&self) -> SharedString {
        if self.path == self.collection {
            self.name.clone()
        } else {
            format!("{} › {}", self.collection_name, self.name).into()
        }
    }

    /// How many of a request's folders are the run's own: the folder that
    /// runs and the folders it is in.
    pub(super) fn folder_depth(&self) -> usize {
        self.path
            .strip_prefix(&self.collection)
            .map_or(0, |folder| folder.components().count())
    }

    /// Whether a run is preparing, running or paused.
    pub fn is_running(&self) -> bool {
        self.preparing.is_some() || self.run.as_ref().is_some_and(Run::is_active)
    }

    /// Whether the collection or folder was deleted while a run was active.
    pub fn is_detached(&self) -> bool {
        self.detached
    }

    /// Detach this runner from its deleted collection or folder while an
    /// active run finishes. Future runs are disabled here, and a replacement
    /// collection created at the same path opens a fresh runner.
    pub fn detach(&mut self, cx: &mut Context<Self>) {
        if self.detached {
            return;
        }

        self.detached = true;
        self.sequence.clear();
        self.original.clear();
        self.other_protocols = 0;
        self.start_error = None;
        cx.notify();
    }

    /// Show the collection's or folder's requests as the sidebar has them
    /// now. While the sequence matches the collection's order with every
    /// request selected, it follows the sidebar order. Otherwise kept
    /// requests keep their place and selection, and new ones are added at
    /// the end, selected.
    pub fn refresh(
        &mut self,
        requests: impl IntoIterator<Item = (FileEntry, Vec<SharedString>)>,
        relocated: Option<(&Path, &Path)>,
        cx: &mut Context<Self>,
    ) {
        if self.detached {
            return;
        }

        let (original, other_protocols) = runnable(requests);
        if let Some(preparing) = &mut self.preparing {
            preparing.follow_refresh(&self.sequence, &original);
        }

        let sequence = merge_sequence(&self.sequence, &self.original, &original, relocated);

        if self.sequence == sequence
            && self.original == original
            && self.other_protocols == other_protocols
        {
            return;
        }

        if self.sequence != sequence {
            self.start_error = None;
        }

        self.sequence = sequence;
        self.original = original;
        self.other_protocols = other_protocols;
        cx.notify();
    }

    /// Follow a collection or folder renamed or moved in the sidebar.
    /// `collection` is the directory of the collection it is in now.
    pub fn relocate(
        &mut self,
        previous: &Path,
        path: &Path,
        name: SharedString,
        collection: &Path,
        cx: &mut Context<Self>,
    ) {
        if self.detached {
            return;
        }

        let Some(source) = rebase_moved_path(&self.path, previous, path) else {
            return;
        };

        if self.path == previous {
            self.name = name;
        }
        self.path = source;
        self.collection = collection.to_path_buf();
        self.collection_name = file_name(collection);

        let requests = self
            .sequence
            .iter_mut()
            .map(|item| &mut item.request)
            .chain(self.original.iter_mut());
        for request in requests {
            if let Some(path) = rebase_moved_path(&request.path, previous, path) {
                request.path = path;
            }
        }
        if self.preparing.is_some() {
            let inputs = self.collection_inputs(collection, cx);
            if let Some(preparing) = &mut self.preparing {
                preparing.relocate(previous, path, collection, inputs);
            }
        }
        cx.notify();
    }

    /// Rebase in-flight preparation request paths after an in-app move or
    /// rename, including moves out of this runner's collection or folder.
    pub fn relocate_preparing(&mut self, previous: &Path, path: &Path) {
        if let Some(preparing) = &mut self.preparing {
            preparing.relocate_paths(previous, path);
        }
    }

    /// Record an explicit in-app disk deletion so an in-flight preparation
    /// keeps the captured requests that actually existed on disk right before
    /// deletion, plus collection settings when the collection itself was
    /// deleted.
    pub fn note_in_app_delete(&mut self, deleted_path: &Path, removed_files: &HashSet<PathBuf>) {
        if let Some(preparing) = &mut self.preparing {
            preparing.note_deleted(deleted_path, removed_files, self.detached);
        }
    }

    fn collection_inputs(&self, collection: &Path, cx: &App) -> Option<(Values, SharedSettings)> {
        let collection = self.collections.read(cx).collection(collection)?;

        Some((
            collection.local_env().entries.clone(),
            SharedSettings {
                scripts: collection.scripts().clone(),
                auth: collection.auth().clone(),
            },
        ))
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

    /// Whether the sequence is the collection's order with every request
    /// selected, which Reset restores.
    pub(super) fn is_original(&self) -> bool {
        is_original_sequence(&self.sequence, &self.original)
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
    /// Each runs as it is saved now, which is read off the UI thread first.
    pub(super) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.detached || self.is_running() {
            return;
        }

        let selected = self
            .sequence
            .iter()
            .filter(|item| item.selected)
            .map(|item| item.request.clone())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return;
        }

        analytics::capture(
            "collection_run_started",
            serde_json::json!({ "requests": selected.len() }),
            cx,
        );

        // What the run uses is taken now, so changing the environment or
        // moving or deleting the source while it prepares changes nothing
        // about it.
        let collection = self.collection.clone();
        let (collection_values, collection_settings) =
            self.collection_inputs(&collection, cx).unwrap_or_default();
        let (environment, environment_path) = {
            let environments = self.environments.read(cx);
            (environments.active().cloned(), environments.active_path())
        };

        self.start_error = None;
        self.preparing = Some(Preparing::new(
            selected,
            collection,
            collection_values,
            collection_settings,
            environment,
            environment_path,
        ));
        self.spawn_snapshot(window, cx);
        cx.notify();
    }

    fn spawn_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preparing) = &mut self.preparing else {
            return;
        };

        let selected = preparing.selected.clone();
        let collection = preparing.collection.clone();
        let environment = preparing.environment.clone();
        let environment_path = preparing.environment_path.clone();

        let snapshot = cx.background_spawn(async move {
            let loaded = selected
                .iter()
                .map(|request| saved(request, &collection))
                .collect();

            Snapshot {
                loaded,
                collection_values: read_entries(&collection.join("environment.toml")),
                environment_values: environment_path
                    .as_deref()
                    .map_or_else(|| Ok(HashMap::new()), read_entries),
                settings: Collection::load_settings(&collection).map_err(|error| error.to_string()),
                collection,
                environment,
            }
        });

        preparing.task = Some(cx.spawn_in(window, async move |this, cx| {
            let snapshot = snapshot.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_preparing(snapshot, window, cx);
            });
        }));
    }

    fn finish_preparing(
        &mut self,
        snapshot: Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preparing) = self.preparing.as_mut() else {
            return;
        };
        if preparing.relocated {
            preparing.relocated = false;
            self.spawn_snapshot(window, cx);
            return;
        }

        let Some(preparing) = self.preparing.take() else {
            return;
        };
        let (requests, missing) = resolve_prepared_requests(
            &preparing.selected,
            snapshot.loaded,
            &preparing.removed,
            &snapshot.collection,
        );
        let (collection_values, environment_values, environment_error, settings) =
            resolve_prepared_inputs(
                preparing.collection_removed,
                preparing.collection_values,
                preparing.collection_settings,
                snapshot.collection_values,
                snapshot.environment_values,
                snapshot.settings,
            );

        self.begin(
            PreparedRun {
                requests,
                missing,
                collection_values,
                environment_values,
                environment_error,
                settings,
                collection: snapshot.collection,
                environment: snapshot.environment,
            },
            window,
            cx,
        );
    }

    fn begin(&mut self, prepared: PreparedRun, window: &mut Window, cx: &mut Context<Self>) {
        let PreparedRun {
            requests,
            missing,
            collection_values,
            environment_values,
            environment_error,
            settings,
            collection,
            environment,
        } = prepared;
        if requests.is_empty() {
            self.start_error = Some("The selected requests are no longer saved.".into());
            cx.notify();
            return;
        }

        let options = self.options;
        let preferences = cx
            .try_global::<Preferences>()
            .map(|preferences| preferences.request.clone())
            .unwrap_or_default();
        let collection_environment = collection.join("environment.toml");

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
        self.exported = None;
        self.run = Some(Run {
            cursor: Cursor::new(requests.len(), iterations),
            requests,
            results: Vec::new(),
            totals: Totals::default(),
            kept_bytes: KEPT_BODY_LIMIT,
            notes: Vec::new(),
            missing,
            status: RunStatus::Running,
            iterations,
            delay: self.delay_value(cx),
            data: self.data.as_ref().map(|data| data.rows.clone()),
            options,
            environment,
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
                auth: settings
                    .as_ref()
                    .map(|settings| settings.auth.clone())
                    .unwrap_or_default(),
                scripts: settings.map(|settings| settings.scripts),
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
        if self.detached || self.run.as_ref().is_some_and(Run::is_active) {
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
    ) -> Option<(Position, Task<Result<Execution, ExecutionFailure>>)> {
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
        .with_collection_auth(context.auth.clone())
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
        // The header names the request being sent.
        cx.notify();

        Some((position, cx.background_spawn(send)))
    }

    /// Keep a request's result and move on. Returns whether the run goes on.
    fn record(
        &mut self,
        position: Position,
        result: Result<Execution, ExecutionFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        run.sending = false;

        let result = RunResult::new(
            position,
            run.requests[position.index].request.method,
            result,
            run.options.persist_responses.then_some(&mut run.kept_bytes),
            !run.options.logs_off,
        );
        run.totals.add(&result);
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

/// Variable values by name.
type Values = HashMap<String, String>;

/// What a run reads from the collection before it starts.
struct Snapshot {
    /// Each selected request as its file held it, or `None` when it was no
    /// longer saved at that path as that HTTP request.
    loaded: Vec<Option<RunRequest>>,
    /// The collection's variables as `environment.toml` held them.
    collection_values: Result<Values, String>,
    /// The active global environment's variables as its file held them.
    environment_values: Result<Values, String>,
    /// The scripts and authorization the collection shares with requests.
    settings: Result<SharedSettings, String>,
    /// Whose session the run changes.
    collection: PathBuf,
    /// The name of the environment whose values `environment_values` holds.
    environment: Option<SharedString>,
}

/// The resolved inputs passed into `CollectionRunner::begin`.
struct PreparedRun {
    requests: Vec<RunRequest>,
    missing: Vec<SharedString>,
    collection_values: Values,
    environment_values: Values,
    environment_error: Option<String>,
    settings: Result<SharedSettings, String>,
    collection: PathBuf,
    environment: Option<SharedString>,
}

fn rebase_moved_path(old: &Path, previous: &Path, path: &Path) -> Option<PathBuf> {
    let inner = old.strip_prefix(previous).ok()?;
    // Joining an empty path would add a trailing separator.
    Some(if inner.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        path.join(inner)
    })
}

/// Combine the requests read from disk with the selection captured when the
/// run started. Requests whose files existed on disk right before an explicit
/// in-app deletion while preparing keep their captured copy in place; requests
/// already missing on disk are reported in `missing`.
pub(super) fn resolve_prepared_requests(
    selected: &[RunRequest],
    loaded: Vec<Option<RunRequest>>,
    removed: &HashSet<usize>,
    collection: &Path,
) -> (Vec<RunRequest>, Vec<SharedString>) {
    let mut requests = Vec::with_capacity(selected.len());
    let mut missing = Vec::new();

    for (index, (captured, from_disk)) in selected.iter().zip(loaded).enumerate() {
        if let Some(request) = from_disk {
            requests.push(request);
        } else if removed.contains(&index) {
            requests.push(RunRequest {
                request: match Request::Http(captured.request.clone()).resolved_from(collection) {
                    Request::Http(request) => request,
                    _ => unreachable!("resolving keeps the protocol"),
                },
                ..captured.clone()
            });
        } else {
            missing.push(captured.name.clone());
        }
    }

    (requests, missing)
}

/// Choose the collection variables, global environment variables, variable error
/// and shared settings once the background read finishes. When the collection
/// was deleted in the app while preparing, the captured collection variables
/// and shared settings are kept so the run starts with the settings in effect
/// when Run was clicked.
pub(super) fn resolve_prepared_inputs(
    collection_removed: bool,
    captured_values: Values,
    captured_settings: SharedSettings,
    disk_collection_values: Result<Values, String>,
    disk_environment_values: Result<Values, String>,
    disk_settings: Result<SharedSettings, String>,
) -> (
    Values,
    Values,
    Option<String>,
    Result<SharedSettings, String>,
) {
    let collection_values = if collection_removed {
        Ok(captured_values)
    } else {
        disk_collection_values
    };
    let settings = if collection_removed {
        Ok(captured_settings)
    } else {
        disk_settings
    };

    let (collection_values, environment_values, environment_error) =
        match (collection_values, disk_environment_values) {
            (Ok(collection), Ok(environment)) => (collection, environment, None),
            (Err(error), _) | (_, Err(error)) => {
                (Default::default(), Default::default(), Some(error))
            }
        };

    (
        collection_values,
        environment_values,
        environment_error,
        settings,
    )
}

/// Whether `sequence` is `original` in order with every request selected,
/// which Reset restores.
pub(super) fn is_original_sequence(sequence: &[SequenceItem], original: &[RunRequest]) -> bool {
    sequence.len() == original.len()
        && sequence.iter().zip(original).all(|(item, request)| {
            item.selected && item.request.path == request.path && item.request.id == request.id
        })
}

/// Merge `original` (the collection's or folder's runnable requests in sidebar
/// order) into `sequence`.
///
/// While the sequence has not been customized, it follows the sidebar order
/// with every request selected. Otherwise kept requests keep their place and
/// selection with updated metadata, removed requests drop out, and new ones
/// are added at the end, selected.
///
/// Existing rows match by `(path, id)` first (rebasing `path` with `relocated`
/// after an explicit in-app move), so two files sharing an ID never replace
/// each other. An external move falls back to matching by ID only when that ID
/// is unique on both sides.
pub(super) fn merge_sequence(
    sequence: &[SequenceItem],
    previous_original: &[RunRequest],
    original: &[RunRequest],
    relocated: Option<(&Path, &Path)>,
) -> Vec<SequenceItem> {
    if is_original_sequence(sequence, previous_original) {
        return original
            .iter()
            .cloned()
            .map(|request| SequenceItem {
                request,
                selected: true,
            })
            .collect();
    }

    let by_path_and_id: HashMap<(&Path, &str), usize> = original
        .iter()
        .enumerate()
        .map(|(index, request)| ((request.path.as_path(), request.id.as_ref()), index))
        .collect();
    let mut original_id_counts: HashMap<&str, usize> = HashMap::new();
    for request in original {
        *original_id_counts.entry(request.id.as_ref()).or_default() += 1;
    }
    let mut sequence_id_counts: HashMap<&str, usize> = HashMap::new();
    for item in sequence {
        *sequence_id_counts
            .entry(item.request.id.as_ref())
            .or_default() += 1;
    }
    let unique_by_id: HashMap<&str, usize> = original
        .iter()
        .enumerate()
        .filter(|(_, request)| original_id_counts.get(request.id.as_ref()) == Some(&1))
        .map(|(index, request)| (request.id.as_ref(), index))
        .collect();

    let mut matched = vec![false; original.len()];
    let mut resolved: Vec<Option<usize>> = vec![None; sequence.len()];

    for (seq_index, item) in sequence.iter().enumerate() {
        let expected_path = relocated
            .and_then(|(previous, next)| rebase_moved_path(&item.request.path, previous, next))
            .unwrap_or_else(|| item.request.path.clone());
        if let Some(&orig_index) =
            by_path_and_id.get(&(expected_path.as_path(), item.request.id.as_ref()))
            && !matched[orig_index]
        {
            matched[orig_index] = true;
            resolved[seq_index] = Some(orig_index);
        }
    }

    for (seq_index, item) in sequence.iter().enumerate() {
        if resolved[seq_index].is_some() {
            continue;
        }
        let id = item.request.id.as_ref();
        if sequence_id_counts.get(id) == Some(&1)
            && let Some(&orig_index) = unique_by_id.get(id)
            && !matched[orig_index]
        {
            matched[orig_index] = true;
            resolved[seq_index] = Some(orig_index);
        }
    }

    let mut updated: Vec<SequenceItem> = sequence
        .iter()
        .zip(resolved)
        .filter_map(|(item, orig_index)| {
            Some(SequenceItem {
                request: original[orig_index?].clone(),
                selected: item.selected,
            })
        })
        .collect();
    updated.extend(
        original
            .iter()
            .enumerate()
            .filter(|(index, _)| !matched[*index])
            .map(|(_, request)| SequenceItem {
                request: request.clone(),
                selected: true,
            }),
    );

    updated
}

/// The HTTP requests that run, and how many requests of other protocols
/// are left out.
pub(super) fn runnable(
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
