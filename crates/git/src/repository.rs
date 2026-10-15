use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::command::{GitError, run};
use crate::status::{Change, Interrupted, Status, parse};

/// Paths go to Git on its standard input, separated by NUL.
const FROM_INPUT: [&str; 2] = ["--pathspec-from-file=-", "--pathspec-file-nul"];

/// How long reading the status and the like may take before Git is stopped.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Pulling and pushing wait on the network, and committing on the hooks the
/// repository runs first.
const LONG_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// A Git repository, found by the folder that holds its `.git`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repository {
    root: PathBuf,
}

impl Repository {
    /// The repository whose working tree holds `path`. It is found by its
    /// `.git`, without running Git, so nothing runs for folders outside
    /// repositories. `path` does not have to exist anymore.
    pub fn containing(path: &Path) -> Option<Self> {
        real_path(path)
            .ancestors()
            .find(|folder| folder.join(".git").exists())
            .map(|root| Self {
                root: root.to_path_buf(),
            })
    }

    /// Makes a new repository in `path`, creating the folder if needed. Its
    /// first branch is `main`, unless the user chose another name in their
    /// Git settings.
    pub fn init(path: &Path) -> Result<Self, GitError> {
        fs::create_dir_all(path)?;
        let chosen = run(
            path,
            &[
                OsStr::new("config"),
                OsStr::new("--get"),
                OsStr::new("init.defaultBranch"),
            ],
            None,
            TIMEOUT,
            true,
        )
        .is_ok();
        let mut args = vec![OsStr::new("init")];
        if !chosen {
            args.push(OsStr::new("--initial-branch=main"));
        }
        run(path, &args, None, TIMEOUT, true)?;

        Ok(Self {
            root: real_path(path),
        })
    }

    /// The folder of the working tree.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The working tree's folder name, which names the repository.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .unwrap_or(self.root.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// The branch, how it compares with its upstream, and the changes not
    /// committed in the folders and files of `scope`.
    pub fn status(&self, scope: &[PathBuf]) -> Result<Status, GitError> {
        let mut args = vec![
            OsStr::new("status"),
            OsStr::new("--porcelain=v2"),
            OsStr::new("--branch"),
            OsStr::new("-z"),
            // Each new file, rather than only the new folder that holds it.
            OsStr::new("--untracked-files=all"),
            OsStr::new("--"),
        ];
        let paths = self.relative(scope);
        args.extend(paths.iter().map(|path| path.as_os_str()));

        let mut status = parse(&self.git(&args, None, TIMEOUT)?, &self.root);
        status.remotes = self
            .git(&[OsStr::new("remote")], None, TIMEOUT)?
            .lines()
            .map(str::to_owned)
            .collect();
        status.interrupted = self.interrupted()?;
        status.conflicted |= self.has_conflicts()?;

        Ok(status)
    }

    /// Commits every change in `scope` and nothing else, leaving changes
    /// elsewhere, such as a project's code, as they are, staged or not.
    /// Returns whether there was anything to commit.
    pub fn commit(&self, scope: &[PathBuf], message: &str) -> Result<bool, GitError> {
        let changes = self.status(scope)?.changes;
        if changes.is_empty() {
            return Ok(false);
        }

        // New files must be known to Git before a commit can take them. A
        // staged deletion or rename is in neither the working tree nor what
        // is staged anymore, so adding it would fail.
        let unstaged: Vec<_> = changes
            .iter()
            .filter(|change| change.unstaged)
            .map(|change| Change {
                original: None,
                ..change.clone()
            })
            .collect();
        if !unstaged.is_empty() {
            let mut add = vec![OsStr::new("add"), OsStr::new("--all")];
            add.extend(FROM_INPUT.map(OsStr::new));
            self.git(&add, Some(&self.pathspecs(&unstaged)), TIMEOUT)?;
        }

        // Adding can leave nothing to commit, as for a file added and then
        // deleted, which a commit could not find.
        let changes = self.status(scope)?.changes;
        if changes.is_empty() {
            return Ok(false);
        }

        // Given paths, a commit takes only those and leaves what else is
        // staged for later.
        let mut commit = vec![
            OsStr::new("commit"),
            OsStr::new("--message"),
            OsStr::new(message),
            OsStr::new("--only"),
        ];
        commit.extend(FROM_INPUT.map(OsStr::new));
        self.git(&commit, Some(&self.pathspecs(&changes)), LONG_TIMEOUT)?;

        Ok(true)
    }

    /// Takes the upstream's new commits. Local changes are put aside and
    /// back around it. Unless the user chose how in their Git settings, the
    /// local commits are replayed on top, which keeps the history straight.
    pub fn pull(&self) -> Result<(), GitError> {
        if self.upstream()?.is_none() {
            return Err(GitError::NoUpstream);
        }

        let mut args = vec![OsStr::new("pull"), OsStr::new("--autostash")];
        let chosen = self
            .git(
                &[
                    OsStr::new("config"),
                    OsStr::new("--get"),
                    OsStr::new("pull.rebase"),
                ],
                None,
                TIMEOUT,
            )
            .is_ok();
        if !chosen {
            args.push(OsStr::new("--rebase"));
        }

        match self.git(&args, None, LONG_TIMEOUT) {
            // Putting local changes back can conflict too, and Git still
            // reports success then.
            Ok(_) if self.has_conflicts()? => Err(GitError::Conflicts),
            Ok(_) => Ok(()),
            // What Git says then names commits rather than the conflict.
            Err(GitError::Failed(_)) if self.interrupted()?.is_some() => Err(GitError::Conflicts),
            Err(error) => Err(error),
        }
    }

    /// Pushes the branch. A branch without an upstream yet is pushed to the
    /// remote named `origin`, or the only other, and tracks it from then on.
    pub fn push(&self) -> Result<(), GitError> {
        if self.upstream()?.is_some() {
            return self
                .git(&[OsStr::new("push")], None, LONG_TIMEOUT)
                .map(drop);
        }

        let remotes = self.git(&[OsStr::new("remote")], None, TIMEOUT)?;
        let remote = remotes
            .lines()
            .find(|remote| *remote == "origin")
            .or_else(|| remotes.lines().next())
            .ok_or(GitError::NoRemote)?;

        self.git(
            &[
                OsStr::new("push"),
                OsStr::new("--set-upstream"),
                OsStr::new(remote),
                OsStr::new("HEAD"),
            ],
            None,
            LONG_TIMEOUT,
        )
        .map(drop)
    }

    /// Learns about the upstream's new commits without taking them. It runs
    /// unasked, so nothing asks for a password meanwhile.
    pub fn fetch(&self) -> Result<(), GitError> {
        run(
            &self.root,
            &[OsStr::new("fetch"), OsStr::new("--quiet")],
            None,
            TIMEOUT,
            false,
        )
        .map(drop)
    }

    pub fn add_remote(&self, name: &str, url: &str) -> Result<(), GitError> {
        self.git(
            &[
                OsStr::new("remote"),
                OsStr::new("add"),
                OsStr::new(name),
                OsStr::new(url),
            ],
            None,
            TIMEOUT,
        )
        .map(drop)
    }

    pub fn remove_remote(&self, name: &str) -> Result<(), GitError> {
        self.git(
            &[OsStr::new("remote"), OsStr::new("remove"), OsStr::new(name)],
            None,
            TIMEOUT,
        )
        .map(drop)
    }

    fn upstream(&self) -> Result<Option<String>, GitError> {
        let upstream = self.git(
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--abbrev-ref"),
                OsStr::new("--symbolic-full-name"),
                OsStr::new("@{upstream}"),
            ],
            None,
            TIMEOUT,
        );

        match upstream {
            Ok(upstream) => Ok(Some(upstream.trim().to_owned())),
            Err(GitError::Failed(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Whether files are left with conflicting changes in them.
    fn has_conflicts(&self) -> Result<bool, GitError> {
        let conflicted = self.git(
            &[
                OsStr::new("diff"),
                OsStr::new("--name-only"),
                OsStr::new("--diff-filter=U"),
            ],
            None,
            TIMEOUT,
        )?;

        Ok(!conflicted.trim().is_empty())
    }

    /// A merge or rebase that stopped and waits to be finished.
    fn interrupted(&self) -> Result<Option<Interrupted>, GitError> {
        let paths = self.git(
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--git-path"),
                OsStr::new("MERGE_HEAD"),
                OsStr::new("--git-path"),
                OsStr::new("rebase-merge"),
                OsStr::new("--git-path"),
                OsStr::new("rebase-apply"),
            ],
            None,
            TIMEOUT,
        )?;
        let exists: Vec<bool> = paths
            .lines()
            .map(|path| self.root.join(path).exists())
            .collect();

        Ok(match exists.as_slice() {
            [true, ..] => Some(Interrupted::Merge),
            [_, true, ..] | [_, _, true] => Some(Interrupted::Rebase),
            _ => None,
        })
    }

    /// `paths` relative to the working tree, as Git runs in it. Absolute
    /// paths could name the working tree through a link that Git does not
    /// follow.
    fn relative(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        paths
            .iter()
            .filter_map(|path| {
                let path = real_path(path);
                let path = path.strip_prefix(&self.root).ok()?;

                Some(if path.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    path.to_path_buf()
                })
            })
            .collect()
    }

    /// The changed paths, and those renamed files had, for Git's standard
    /// input, as there can be more than a command line holds.
    fn pathspecs(&self, changes: &[Change]) -> Vec<u8> {
        let mut paths = Vec::new();
        for change in changes {
            for path in std::iter::once(&change.path).chain(&change.original) {
                if let Ok(path) = path.strip_prefix(&self.root) {
                    paths.extend_from_slice(path.as_os_str().as_encoded_bytes());
                    paths.push(0);
                }
            }
        }

        paths
    }

    fn git(
        &self,
        args: &[&OsStr],
        input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<String, GitError> {
        run(&self.root, args, input, timeout, true)
    }
}

/// `path` with the links in it followed, as Git sees it, so that a folder
/// reached through a link is found in its repository and compares with the
/// paths of changes. The part of `path` that does not exist stays as it is.
pub fn real_path(path: &Path) -> PathBuf {
    // Followed links come back as verbatim paths on Windows, which Git does
    // not take, and links are rare there.
    if cfg!(windows) {
        return path.to_path_buf();
    }

    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(mut real) = fs::canonicalize(existing) {
            real.extend(missing.iter().rev());
            return real;
        }

        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}
