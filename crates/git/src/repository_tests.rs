use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;

use crate::{ChangeKind, GitError, Interrupted, Repository, real_path};

const REQUEST: &str = "id = 'health'\nname = 'Health'\n";

/// Runs Git for a test's own setup and checks.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap()
}

/// A repository whose settings do not depend on the machine's, with a
/// collection beside a project's code, both committed.
fn project(directory: &Path) -> Repository {
    fs::create_dir_all(directory.join("collections/API")).unwrap();
    fs::create_dir_all(directory.join("src")).unwrap();
    fs::write(directory.join("collections/API/Health.toml"), REQUEST).unwrap();
    fs::write(directory.join("src/main.rs"), "fn main() {}\n").unwrap();

    git(directory, &["init", "--initial-branch=main"]);
    configure(directory);
    git(directory, &["add", "--all"]);
    git(directory, &["commit", "--message", "Start"]);

    Repository::containing(directory).unwrap()
}

fn configure(directory: &Path) {
    git(directory, &["config", "user.name", "Tester"]);
    git(directory, &["config", "user.email", "tester@example.com"]);
    git(directory, &["config", "commit.gpgsign", "false"]);
}

/// A project pushed to a bare remote, and a second clone of it, as on a
/// teammate's machine.
fn shared() -> (TempDir, Repository, Repository) {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote.git");
    git(
        root.path(),
        &["init", "--bare", "--initial-branch=main", "remote.git"],
    );

    let mine = project(&root.path().join("mine"));
    mine.add_remote("origin", remote.to_str().unwrap()).unwrap();
    mine.push().unwrap();

    git(root.path(), &["clone", remote.to_str().unwrap(), "theirs"]);
    configure(&root.path().join("theirs"));
    let theirs = Repository::containing(&root.path().join("theirs")).unwrap();

    (root, mine, theirs)
}

fn collections(repository: &Repository) -> Vec<PathBuf> {
    vec![repository.root().join("collections")]
}

#[test]
fn a_repository_is_found_from_any_path_in_its_working_tree() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());

    let nested = Repository::containing(&root.path().join("collections/API/Gone/Old.toml"));

    assert_eq!(nested, Some(repository));
    assert_eq!(
        Repository::containing(root.path().parent().unwrap()),
        None,
        "the temporary directory is in no repository"
    );
}

#[test]
fn the_status_lists_only_the_changes_asked_about() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());
    fs::write(
        root.path().join("collections/API/Health.toml"),
        "id = 'changed'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("collections/API/New request.toml"),
        REQUEST,
    )
    .unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() { todo!() }\n").unwrap();

    let status = repository.status(&collections(&repository)).unwrap();

    let changes: Vec<_> = status
        .changes
        .iter()
        .map(|change| {
            let path = change.path.strip_prefix(real_path(root.path())).unwrap();
            (path.to_path_buf(), change.kind)
        })
        .collect();
    assert_eq!(
        changes,
        [
            (
                PathBuf::from("collections/API/Health.toml"),
                ChangeKind::Modified
            ),
            (
                PathBuf::from("collections/API/New request.toml"),
                ChangeKind::Added
            ),
        ]
    );
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(status.head.is_some());
    assert!(status.remotes.is_empty());
    assert_eq!(status.interrupted, None);
}

#[test]
fn committing_takes_only_the_collections_and_leaves_the_code_staged() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());
    let collections_path = root.path().join("collections");
    fs::write(collections_path.join("API/Health.toml"), "id = 'changed'\n").unwrap();
    fs::create_dir_all(collections_path.join("API/Users")).unwrap();
    fs::write(collections_path.join("API/Users/List [all].toml"), REQUEST).unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() { todo!() }\n").unwrap();
    git(root.path(), &["add", "src/main.rs"]);

    assert!(
        repository
            .commit(&collections(&repository), "Update API")
            .unwrap()
    );

    let committed = git(root.path(), &["show", "--name-only", "--format=%s", "HEAD"]);
    assert_eq!(
        committed
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>(),
        [
            "Update API",
            "collections/API/Health.toml",
            "collections/API/Users/List [all].toml"
        ]
    );
    assert_eq!(
        git(root.path(), &["diff", "--cached", "--name-only"]).trim(),
        "src/main.rs"
    );
    assert!(
        !repository
            .commit(&collections(&repository), "Nothing")
            .unwrap(),
        "nothing is left to commit"
    );
}

#[test]
fn a_file_added_and_then_deleted_leaves_nothing_to_commit() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());
    let added = root.path().join("collections/API/Added.toml");
    fs::write(&added, REQUEST).unwrap();
    git(root.path(), &["add", "--all"]);
    fs::remove_file(&added).unwrap();

    assert!(
        !repository
            .commit(&collections(&repository), "Nothing")
            .unwrap()
    );
}

#[test]
fn deletions_and_renames_staged_already_are_committed() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());
    fs::write(root.path().join("collections/API/Other.toml"), REQUEST).unwrap();
    git(root.path(), &["add", "--all"]);
    git(root.path(), &["commit", "--message", "Other"]);
    git(
        root.path(),
        &["rm", "--quiet", "collections/API/Other.toml"],
    );
    git(
        root.path(),
        &[
            "mv",
            "collections/API/Health.toml",
            "collections/API/Status.toml",
        ],
    );

    assert!(
        repository
            .commit(&collections(&repository), "Tidy")
            .unwrap()
    );
    assert_eq!(
        git(root.path(), &["ls-files", "collections"]).trim(),
        "collections/API/Status.toml"
    );
}

// Links are followed only where Git takes the paths that come out of it.
#[cfg(unix)]
#[test]
fn a_folder_reached_through_a_link_is_found_in_its_repository() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(&root.path().join("project"));
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(root.path().join("project/collections"), &alias).unwrap();
    fs::write(alias.join("API/Health.toml"), "id = 'changed'\n").unwrap();

    assert_eq!(Repository::containing(&alias), Some(repository.clone()));
    let status = repository.status(&[alias]).unwrap();
    assert_eq!(status.changes.len(), 1);
}

#[test]
fn a_removed_collection_is_committed_as_removed() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());
    let api = root.path().join("collections/API");
    fs::remove_dir_all(&api).unwrap();

    // The collection is gone, so its folder is asked about by name.
    assert!(repository.commit(&[api], "Remove API").unwrap());

    assert_eq!(
        git(root.path(), &["ls-files", "collections"]).trim(),
        "",
        "{}",
        git(root.path(), &["status"])
    );
}

#[test]
fn pushing_tracks_the_remote_and_pulling_keeps_local_changes() {
    let (_root, mine, theirs) = shared();
    let status = mine.status(&collections(&mine)).unwrap();
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!(status.remotes, ["origin"]);

    fs::write(theirs.root().join("collections/API/Theirs.toml"), REQUEST).unwrap();
    theirs.commit(&collections(&theirs), "Add theirs").unwrap();
    theirs.push().unwrap();

    fs::write(mine.root().join("collections/API/Mine.toml"), REQUEST).unwrap();
    mine.commit(&collections(&mine), "Add mine").unwrap();
    // A project's work in progress is put aside and back.
    fs::write(mine.root().join("src/main.rs"), "fn main() { todo!() }\n").unwrap();
    mine.fetch().unwrap();
    let status = mine.status(&collections(&mine)).unwrap();
    assert_eq!((status.ahead, status.behind), (1, 1));

    mine.pull().unwrap();
    mine.push().unwrap();

    let status = mine.status(&collections(&mine)).unwrap();
    assert_eq!((status.ahead, status.behind), (0, 0));
    assert!(mine.root().join("collections/API/Theirs.toml").exists());
    assert_eq!(
        fs::read_to_string(mine.root().join("src/main.rs")).unwrap(),
        "fn main() { todo!() }\n"
    );
    let history = git(mine.root(), &["log", "--format=%s"]);
    assert_eq!(
        history.lines().collect::<Vec<_>>(),
        ["Add mine", "Add theirs", "Start"],
        "local commits are replayed on top"
    );
}

#[test]
fn a_conflicting_pull_stops_and_shows_the_conflict() {
    let (_root, mine, theirs) = shared();
    let health = "collections/API/Health.toml";
    fs::write(theirs.root().join(health), "id = 'theirs'\n").unwrap();
    theirs.commit(&collections(&theirs), "Theirs").unwrap();
    theirs.push().unwrap();
    fs::write(mine.root().join(health), "id = 'mine'\n").unwrap();
    mine.commit(&collections(&mine), "Mine").unwrap();

    assert_eq!(mine.pull(), Err(GitError::Conflicts));

    let status = mine.status(&collections(&mine)).unwrap();
    assert_eq!(status.interrupted, Some(Interrupted::Rebase));
    assert!(status.conflicted);
}

#[test]
fn local_changes_that_conflict_when_put_back_are_reported() {
    let (_root, mine, theirs) = shared();
    let health = "collections/API/Health.toml";
    fs::write(theirs.root().join(health), "id = 'theirs'\n").unwrap();
    theirs.commit(&collections(&theirs), "Theirs").unwrap();
    theirs.push().unwrap();
    // Not committed, so it is put aside during the pull.
    fs::write(mine.root().join(health), "id = 'mine'\n").unwrap();

    assert_eq!(mine.pull(), Err(GitError::Conflicts));
    assert!(mine.status(&collections(&mine)).unwrap().conflicted);
}

#[test]
fn conflicts_outside_the_paths_asked_about_are_reported() {
    let (_root, mine, theirs) = shared();
    fs::write(
        theirs.root().join("src/main.rs"),
        "fn main() { theirs() }\n",
    )
    .unwrap();
    git(
        theirs.root(),
        &["commit", "--quiet", "--all", "--message", "Theirs"],
    );
    theirs.push().unwrap();
    fs::write(mine.root().join("src/main.rs"), "fn main() { mine() }\n").unwrap();

    assert_eq!(mine.pull(), Err(GitError::Conflicts));
    let status = mine.status(&collections(&mine)).unwrap();
    assert!(status.changes.is_empty());
    assert!(status.conflicted);
}

#[test]
fn without_a_remote_there_is_nowhere_to_push_or_pull_from() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());

    assert_eq!(repository.push(), Err(GitError::NoRemote));
    assert_eq!(repository.pull(), Err(GitError::NoUpstream));
}

#[test]
fn a_remote_that_cannot_be_pushed_to_can_be_removed() {
    let root = tempfile::tempdir().unwrap();
    let repository = project(root.path());
    let missing = root.path().join("missing.git");

    repository
        .add_remote("origin", missing.to_str().unwrap())
        .unwrap();
    assert!(matches!(repository.push(), Err(GitError::Failed(_))));
    repository.remove_remote("origin").unwrap();

    let status = repository.status(&collections(&repository)).unwrap();
    assert!(status.remotes.is_empty());
}

#[test]
fn init_makes_a_repository_in_a_new_folder() {
    let root = tempfile::tempdir().unwrap();
    let collections = root.path().join("collections");

    let repository = Repository::init(&collections).unwrap();

    assert_eq!(repository.root(), real_path(&collections));
    let status = repository
        .status(std::slice::from_ref(&collections))
        .unwrap();
    assert!(status.branch.is_some());
    assert_eq!(Repository::containing(&collections), Some(repository));
}
