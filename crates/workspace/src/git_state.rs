use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::Duration,
};

use collection::{Collections, CollectionsEvent, directory_name};
use git::{GitError, Repository, Status};
use gpui_kit::*;

/// How long after a change the status is read, so that changes made
/// together are read once.
const REFRESH_DELAY: Duration = Duration::from_millis(300);
/// How long after launch repositories first learn about their upstreams'
/// new commits, and how often after that.
const FIRST_FETCH_DELAY: Duration = Duration::from_secs(10);
const FETCH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// What a repository started in the collections directory leaves out: the
/// CLI's lock, files being written and folder settings that macOS adds.
const IGNORED_FILES: &str = ".cli.lock\n.request-eagle-*.tmp\n.DS_Store\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitAction {
    /// Start a repository in the collections directory.
    Init,
    Commit,
    Pull,
    Push,
    /// Commit, then pull and push.
    Sync,
    /// Add a remote, then commit and push.
    Publish,
}

impl GitAction {
    pub(crate) fn progress(self) -> &'static str {
        match self {
            GitAction::Init => "Starting a repository…",
            GitAction::Commit => "Committing…",
            GitAction::Pull => "Pulling…",
            GitAction::Push => "Pushing…",
            GitAction::Sync => "Syncing…",
            GitAction::Publish => "Publishing…",
        }
    }

    fn name(self) -> &'static str {
        match self {
            GitAction::Init => "init",
            GitAction::Commit => "commit",
            GitAction::Pull => "pull",
            GitAction::Push => "push",
            GitAction::Sync => "sync",
            GitAction::Publish => "publish",
        }
    }
}

/// A Git repository that holds collections.
pub(crate) struct GitRepository {
    pub(crate) repository: Repository,
    /// The collections in its working tree.
    pub(crate) collections: Vec<PathBuf>,
    /// What Request Eagle commits: its collections, the collections
    /// directory when it is in the working tree, and collections that left
    /// it in this session, so that their removal is committed too. Nothing
    /// else, such as a project's code, is.
    scope: Vec<PathBuf>,
    /// `None` until the status is read the first time.
    pub(crate) status: Option<Result<Status, GitError>>,
    /// Why the last action in it failed.
    pub(crate) error: Option<String>,
}

impl GitRepository {
    pub(crate) fn status(&self) -> Option<&Status> {
        self.status.as_ref().and_then(|status| status.as_ref().ok())
    }

    /// Whether a merge or rebase stopped in it, as on conflicts, and waits
    /// for the user to finish it.
    pub(crate) fn is_interrupted(&self) -> bool {
        self.status()
            .is_some_and(|status| status.interrupted.is_some() || status.has_conflicts())
    }

    /// The collection that holds `path`, by its name.
    pub(crate) fn collection_name(&self, path: &Path) -> Option<String> {
        self.collections
            .iter()
            .find(|collection| path.starts_with(collection))
            .map(|collection| directory_name(collection))
    }

    /// Names the commit after the collections it changes.
    fn commit_message(&self) -> String {
        let names: BTreeSet<_> = self
            .status()
            .into_iter()
            .flat_map(|status| &status.changes)
            .filter_map(|change| self.collection_name(&change.path))
            .collect();
        let names: Vec<_> = names.into_iter().collect();

        match names.as_slice() {
            [name] => format!("Update {name}"),
            [first, second] => format!("Update {first} and {second}"),
            [] => "Update collections".to_owned(),
            names => format!("Update {} collections", names.len()),
        }
    }
}

pub(crate) enum GitEvent {
    /// An action finished. `errors` holds why it failed, in each repository
    /// it failed in.
    Finished {
        action: GitAction,
        errors: Vec<String>,
    },
}

/// The Git repositories that hold collections, where they stand, and the
/// actions run in them from the Git panel and the command palette.
pub(crate) struct GitState {
    collections: Entity<Collections>,
    pub(crate) repositories: Vec<GitRepository>,
    /// Collections renamed or removed in this session, by their former
    /// folders.
    departed: Vec<PathBuf>,
    /// The action running now. One runs at a time.
    pub(crate) running: Option<GitAction>,
    /// Repositories whose last action committed, which moves their HEAD
    /// without changing any files.
    committed: Vec<Repository>,
    /// Whether the collections directory holds collections but is in no
    /// repository, so that it can still start one when others hold
    /// collections.
    pub(crate) untracked_directory: bool,
    /// Why starting a repository in the collections directory failed.
    pub(crate) init_error: Option<String>,
    refresh: Option<Task<()>>,
    action: Option<Task<()>>,
    _fetching: Task<()>,
    _collections_subscription: Subscription,
}

impl EventEmitter<GitEvent> for GitState {}

impl GitState {
    pub(crate) fn new(collections: Entity<Collections>, cx: &mut Context<Self>) -> Self {
        let collections_subscription =
            cx.subscribe(&collections, |this, collections, event, cx| {
                match event {
                    CollectionsEvent::CollectionRenamed { previous_path, .. } => {
                        this.departed.push(previous_path.clone());
                    }
                    // A request or folder is in its collection, which is committed
                    // already. A collection is not anymore.
                    CollectionsEvent::Deleted(path)
                        if collections.read(cx).containing(path).is_none() =>
                    {
                        this.departed.push(path.clone());
                    }
                    _ => {}
                }

                this.schedule_refresh(cx);
            });

        // Learn about upstreams' new commits now and then, so that the
        // status tells when there are some to pull.
        let fetching = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_FETCH_DELAY).await;

            loop {
                let Ok(repositories) = this.update(cx, |this, _| this.fetchable()) else {
                    return;
                };

                if !repositories.is_empty() {
                    cx.background_executor()
                        .spawn(async move {
                            for repository in repositories {
                                // Fetching runs unasked, so a failure, such
                                // as being offline, waits for the next one.
                                let _ = repository.fetch();
                            }
                        })
                        .await;

                    if this
                        .update(cx, |this, cx| this.schedule_refresh(cx))
                        .is_err()
                    {
                        return;
                    }
                }

                cx.background_executor().timer(FETCH_INTERVAL).await;
            }
        });

        let mut state = Self {
            collections,
            repositories: Vec::new(),
            departed: Vec::new(),
            running: None,
            committed: Vec::new(),
            untracked_directory: false,
            init_error: None,
            refresh: None,
            action: None,
            _fetching: fetching,
            _collections_subscription: collections_subscription,
        };
        state.schedule_refresh(cx);

        state
    }

    /// Read every repository's status shortly, such as after collections
    /// changed or the window came back to the front.
    pub(crate) fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        // A running action reads the status once it is done.
        if self.running.is_some() {
            return;
        }

        // Replacing the task cancels a refresh that has not run yet.
        self.refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REFRESH_DELAY).await;

            let Ok(repositories) = this.update(cx, |this, cx| this.discover(cx)) else {
                return;
            };
            let statuses = cx
                .background_executor()
                .spawn(async move {
                    repositories
                        .into_iter()
                        .map(|(repository, scope)| {
                            let status = repository.status(&scope);
                            (repository, status)
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            let _ = this.update(cx, |this, cx| this.apply_statuses(statuses, cx));
        }));
    }

    /// Find the repositories that hold collections, keeping what is known
    /// of those found before. Returns each with what to read the status of.
    fn discover(&mut self, cx: &App) -> Vec<(Repository, Vec<PathBuf>)> {
        let registry = self.collections.read(cx).registry();
        let mut previous = std::mem::take(&mut self.repositories);
        let mut repositories: Vec<GitRepository> = Vec::new();

        let mut add = |path: &Path, collection: bool, repositories: &mut Vec<GitRepository>| {
            let Some(repository) = Repository::containing(path) else {
                return;
            };
            let index = match repositories
                .iter()
                .position(|known| known.repository == repository)
            {
                Some(index) => index,
                None => {
                    let known = previous
                        .iter()
                        .position(|known| known.repository == repository)
                        .map(|index| previous.remove(index));
                    repositories.push(GitRepository {
                        repository,
                        collections: Vec::new(),
                        scope: Vec::new(),
                        status: known.as_ref().and_then(|known| known.status.clone()),
                        error: known.and_then(|known| known.error),
                    });
                    repositories.len() - 1
                }
            };

            if collection {
                repositories[index].collections.push(path.to_path_buf());
            }
            repositories[index].scope.push(path.to_path_buf());
        };

        if let Some(directory) = registry.directory() {
            add(directory, false, &mut repositories);
        }
        for collection in registry.collections() {
            add(&collection.path, true, &mut repositories);
        }

        self.untracked_directory = registry.directory().is_some_and(|directory| {
            Repository::containing(directory).is_none()
                && registry
                    .collections()
                    .iter()
                    .any(|collection| collection.path.parent() == Some(directory))
        });

        // Former folders only count while they are gone, as a removed linked
        // collection's folder stays, and in repositories that still hold
        // collections.
        for path in self.departed.iter().filter(|path| !path.exists()) {
            if let Some(repository) = Repository::containing(path)
                && let Some(known) = repositories
                    .iter_mut()
                    .find(|known| known.repository == repository)
            {
                known.scope.push(path.clone());
            }
        }

        let jobs = repositories
            .iter()
            .map(|known| (known.repository.clone(), known.scope.clone()))
            .collect();
        self.repositories = repositories;

        jobs
    }

    fn apply_statuses(
        &mut self,
        statuses: Vec<(Repository, Result<Status, GitError>)>,
        cx: &mut Context<Self>,
    ) {
        let mut moved = false;
        for (repository, status) in statuses {
            let Some(known) = self
                .repositories
                .iter_mut()
                .find(|known| known.repository == repository)
            else {
                continue;
            };

            // Another commit checked out, such as by a pull or switching
            // branches in a terminal, changes the files. One the app just
            // made does not.
            let previous = known.status().and_then(|status| status.head.clone());
            let head = status.as_ref().ok().and_then(|status| status.head.clone());
            let committed = self.committed.contains(&repository);
            moved |= previous.is_some() && head != previous && !committed;

            let interrupted = known.is_interrupted();
            known.status = Some(status);
            // The failure was the conflict, which is resolved now.
            if interrupted && !known.is_interrupted() {
                known.error = None;
            }
        }
        self.committed.clear();

        cx.notify();

        if moved {
            self.collections
                .update(cx, |collections, cx| collections.reload(cx));
        }
    }

    /// The repositories with an upstream to fetch from.
    fn fetchable(&self) -> Vec<Repository> {
        if self.running.is_some() {
            return Vec::new();
        }

        self.repositories
            .iter()
            .filter(|known| {
                known
                    .status()
                    .is_some_and(|status| status.upstream.is_some())
            })
            .map(|known| known.repository.clone())
            .collect()
    }

    /// How many changes each collection has that are not committed yet.
    pub(crate) fn changes_by_collection(&self) -> HashMap<PathBuf, usize> {
        let mut changes = HashMap::new();
        for known in &self.repositories {
            for change in known
                .status()
                .into_iter()
                .flat_map(|status| &status.changes)
            {
                if let Some(collection) = known
                    .collections
                    .iter()
                    .find(|collection| change.path.starts_with(collection))
                {
                    *changes.entry(collection.clone()).or_default() += 1;
                }
            }
        }

        changes
    }

    /// Start a repository in the collections directory, so that Git tracks
    /// every collection kept there.
    pub(crate) fn init(&mut self, cx: &mut Context<Self>) {
        let Some(directory) = self
            .collections
            .read(cx)
            .registry()
            .directory()
            .map(Path::to_path_buf)
        else {
            return;
        };
        if self.running.is_some() {
            return;
        }

        self.running = Some(GitAction::Init);
        self.init_error = None;
        self.refresh = None;
        cx.notify();

        self.action = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let repository = Repository::init(&directory)?;
                    let ignored = directory.join(".gitignore");
                    if !ignored.exists() {
                        std::fs::write(ignored, IGNORED_FILES)?;
                    }

                    Ok::<_, GitError>(repository)
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                let errors: Vec<_> = result
                    .err()
                    .map(|error| error.to_string())
                    .into_iter()
                    .collect();
                this.init_error = errors.first().cloned();
                this.finish(GitAction::Init, errors, cx);
            });
        }));
    }

    /// Run an action in the repository at `root`, or in every repository it
    /// applies to: those with changes to commit, a remote branch to pull
    /// from or a remote to push to. `input` is the commit message for
    /// committing and syncing, generated from the collections changed when
    /// blank, or the remote's URL for publishing. Repositories with a merge
    /// or rebase to finish are left alone.
    pub(crate) fn run(
        &mut self,
        action: GitAction,
        root: Option<&Path>,
        input: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.running.is_some() {
            return;
        }

        let input = input.filter(|input| !input.trim().is_empty());
        let mut errors = Vec::new();
        let mut jobs = Vec::new();
        for known in self.repositories.iter_mut().filter(|known| match root {
            Some(root) => known.repository.root() == root,
            None => known.status().is_some_and(|status| match action {
                GitAction::Commit => !status.changes.is_empty(),
                GitAction::Pull => status.upstream.is_some(),
                _ => !status.remotes.is_empty(),
            }),
        }) {
            if known.is_interrupted() {
                errors.push(format!(
                    "{}: Finish the merge or rebase in progress first.",
                    known.repository.name()
                ));
                continue;
            }

            known.error = None;
            jobs.push(Job {
                repository: known.repository.clone(),
                scope: known.scope.clone(),
                message: match (action, &input) {
                    (GitAction::Commit | GitAction::Sync, Some(message)) => message.clone(),
                    _ => known.commit_message(),
                },
                upstream: known
                    .status()
                    .is_some_and(|status| status.upstream.is_some()),
            });
        }

        if jobs.is_empty() {
            if errors.is_empty() {
                errors.push(
                    match action {
                        GitAction::Commit => "There are no changes to commit.",
                        GitAction::Pull => "No repository tracks a remote branch to pull from.",
                        _ => "No repository has a remote yet. Add one in the Git panel.",
                    }
                    .to_owned(),
                );
            }

            self.finish(action, errors, cx);
            return;
        }

        // A status read now could catch a pull halfway.
        self.running = Some(action);
        self.refresh = None;
        cx.notify();

        let several = jobs.len() > 1;
        self.action = Some(cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    jobs.into_iter()
                        .map(|job| {
                            let result = job.perform(action, input.as_deref());
                            (job.repository, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                for (repository, result) in results {
                    let error = match result {
                        Ok(()) => {
                            if matches!(action, GitAction::Commit | GitAction::Publish) {
                                this.committed.push(repository);
                            }
                            continue;
                        }
                        Err(error) => error,
                    };
                    let error = error.to_string();
                    errors.push(if several {
                        format!("{}: {error}", repository.name())
                    } else {
                        error.clone()
                    });

                    if let Some(known) = this
                        .repositories
                        .iter_mut()
                        .find(|known| known.repository == repository)
                    {
                        known.error = Some(error);
                    }
                }

                this.finish(action, errors, cx);
            });
        }));
    }

    fn finish(&mut self, action: GitAction, errors: Vec<String>, cx: &mut Context<Self>) {
        analytics::capture(
            "git_action_finished",
            serde_json::json!({ "action": action.name(), "succeeded": errors.is_empty() }),
            cx,
        );

        self.running = None;
        self.action = None;
        cx.emit(GitEvent::Finished { action, errors });
        cx.notify();

        self.schedule_refresh(cx);
    }
}

/// An action's work in one repository, run in the background.
struct Job {
    repository: Repository,
    scope: Vec<PathBuf>,
    message: String,
    /// Whether the branch tracks a remote branch to pull from.
    upstream: bool,
}

impl Job {
    fn perform(&self, action: GitAction, input: Option<&str>) -> Result<(), GitError> {
        let repository = &self.repository;

        match action {
            GitAction::Init => Ok(()),
            GitAction::Commit => repository.commit(&self.scope, &self.message).map(drop),
            GitAction::Pull => repository.pull(),
            GitAction::Push => repository.push(),
            GitAction::Sync => {
                repository.commit(&self.scope, &self.message)?;
                if self.upstream {
                    repository.pull()?;
                }

                repository.push()
            }
            GitAction::Publish => {
                let url = input.unwrap_or_default().trim();
                repository.add_remote("origin", url)?;
                repository.commit(&self.scope, &self.message)?;

                repository.push()
            }
        }
    }
}
