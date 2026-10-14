use std::path::{Path, PathBuf};

use crate::status::parse;
use crate::{Change, ChangeKind, Status};

#[test]
fn reads_the_branch_and_how_it_compares_with_its_upstream() {
    let output = "# branch.oid 1234abcd\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\0";

    let status = parse(output, Path::new("/repo"));

    assert_eq!(
        status,
        Status {
            branch: Some("main".into()),
            head: Some("1234abcd".into()),
            upstream: Some("origin/main".into()),
            ahead: 2,
            behind: 3,
            ..Status::default()
        }
    );
}

#[test]
fn a_new_repository_has_no_commit_and_a_rebase_no_branch() {
    let new = parse(
        "# branch.oid (initial)\0# branch.head main\0",
        Path::new("/repo"),
    );
    let rebasing = parse(
        "# branch.oid 1234abcd\0# branch.head (detached)\0",
        Path::new("/repo"),
    );

    assert_eq!(new.head, None);
    assert_eq!(new.branch.as_deref(), Some("main"));
    assert_eq!(rebasing.branch, None);
}

#[test]
fn reads_each_kind_of_change_with_paths_that_hold_spaces() {
    let root = Path::new("/repo");
    let output = [
        "1 .M N... 100644 100644 100644 abc abc API/Get user.toml",
        "1 A. N... 000000 100644 100644 000 abc API/New.toml",
        "1 .D N... 100644 100644 000000 abc abc API/Old.toml",
        "2 R. N... 100644 100644 100644 abc abc R100 API/Renamed request.toml",
        "API/Original request.toml",
        "u UU N... 100644 100644 100644 100644 abc def ghi API/Both.toml",
        "? API/Untracked file.toml",
        "?  Leading space.toml",
        "! target/ignored",
    ]
    .join("\0");

    let status = parse(&output, root);

    let change = |path: &str, kind| Change {
        path: root.join(path),
        original: None,
        kind,
    };
    assert_eq!(
        status.changes,
        vec![
            change("API/Get user.toml", ChangeKind::Modified),
            change("API/New.toml", ChangeKind::Added),
            change("API/Old.toml", ChangeKind::Deleted),
            Change {
                original: Some(PathBuf::from("/repo/API/Original request.toml")),
                ..change("API/Renamed request.toml", ChangeKind::Renamed)
            },
            change("API/Both.toml", ChangeKind::Conflicted),
            change("API/Untracked file.toml", ChangeKind::Added),
            change(" Leading space.toml", ChangeKind::Added),
        ]
    );
    assert!(status.has_conflicts());
}
