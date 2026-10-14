use std::path::{Path, PathBuf};

/// Where a repository stands: its branch, how it compares with the branch
/// it tracks, and the changes not committed yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The branch checked out, or `None` while no branch is, as in the
    /// middle of a rebase.
    pub branch: Option<String>,
    /// The commit checked out, or `None` before the first commit.
    pub head: Option<String>,
    /// The remote branch the branch tracks, such as `origin/main`.
    pub upstream: Option<String>,
    /// Commits not pushed to the upstream yet.
    pub ahead: u32,
    /// Commits on the upstream not pulled yet, as of the last fetch.
    pub behind: u32,
    /// Changes not committed yet, in the paths that were asked about.
    pub changes: Vec<Change>,
    pub remotes: Vec<String>,
    /// A merge or rebase that stopped, such as on conflicts, and waits to be
    /// finished.
    pub interrupted: Option<Interrupted>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interrupted {
    Merge,
    Rebase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    /// The path a renamed file had.
    pub original: Option<PathBuf>,
    pub kind: ChangeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// A new file, whether Git tracks it yet or not.
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Changed on both sides of a merge or rebase.
    Conflicted,
}

impl Status {
    pub fn has_conflicts(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.kind == ChangeKind::Conflicted)
    }
}

/// Reads `git status --porcelain=v2 --branch -z`. Paths are relative to
/// `root`, the repository's working tree, and come back joined to it.
pub(crate) fn parse(output: &str, root: &Path) -> Status {
    let mut status = Status::default();
    let mut records = output.split('\0').filter(|record| !record.is_empty());

    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            read_header(header, &mut status);
            continue;
        }

        // Only the one space after the kind: a path can start with spaces.
        let (kind, fields) = record.split_at(1);
        let fields = fields.strip_prefix(' ').unwrap_or(fields);
        let change = match kind {
            "1" => fields
                .splitn(8, ' ')
                .collect::<Vec<_>>()
                .split_last()
                .map(|(path, fields)| Change {
                    path: root.join(path),
                    original: None,
                    kind: change_kind(fields[0]),
                }),
            // Followed by the path it was renamed or copied from.
            "2" => fields
                .splitn(9, ' ')
                .collect::<Vec<_>>()
                .split_last()
                .map(|(path, fields)| Change {
                    path: root.join(path),
                    original: records.next().map(|original| root.join(original)),
                    kind: change_kind(fields[0]),
                }),
            "u" => fields.splitn(10, ' ').last().map(|path| Change {
                path: root.join(path),
                original: None,
                kind: ChangeKind::Conflicted,
            }),
            "?" => Some(Change {
                path: root.join(fields),
                original: None,
                kind: ChangeKind::Added,
            }),
            // Ignored files.
            _ => None,
        };

        status.changes.extend(change);
    }

    status
}

fn read_header(header: &str, status: &mut Status) {
    let Some((key, value)) = header.split_once(' ') else {
        return;
    };

    match key {
        "branch.oid" if value != "(initial)" => status.head = Some(value.to_owned()),
        "branch.head" if value != "(detached)" => status.branch = Some(value.to_owned()),
        "branch.upstream" => status.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for count in value.split(' ') {
                if let Some(ahead) = count.strip_prefix('+') {
                    status.ahead = ahead.parse().unwrap_or(0);
                } else if let Some(behind) = count.strip_prefix('-') {
                    status.behind = behind.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

/// The kind of change from Git's two letters: the change staged, then the
/// one in the working tree. The working tree's tells what the file is now.
fn change_kind(letters: &str) -> ChangeKind {
    let mut letters = letters.chars();
    let staged = letters.next().unwrap_or('.');
    let unstaged = letters.next().unwrap_or('.');

    match if unstaged == '.' { staged } else { unstaged } {
        'A' | 'C' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        _ => ChangeKind::Modified,
    }
}
