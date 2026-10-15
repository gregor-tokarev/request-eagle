use crate::command::summary;

#[test]
fn common_failures_read_as_what_they_mean() {
    let unreachable = "fatal: '/tmp/remote.git' does not appear to be a git repository\n\
                       fatal: Could not read from remote repository.\n\n\
                       Please make sure you have the correct access rights\n\
                       and the repository exists.";
    let rejected = "To github.com:team/api.git\n ! [rejected]        main -> main (fetch first)\n\
                    error: failed to push some refs to 'github.com:team/api.git'";

    assert_eq!(summary(unreachable), "Can't reach the remote.");
    assert_eq!(summary(rejected), "The remote has new commits. Pull first.");
    assert_eq!(
        summary(
            "remote: Invalid username or token.\nfatal: Authentication failed for 'https://github.com/team/api.git/'"
        ),
        "The remote refused access. Sign in to your Git host first."
    );
}

#[test]
fn other_failures_show_their_first_line_without_its_prefix() {
    assert_eq!(
        summary("\nfatal: pathspec 'api' did not match any files\nmore"),
        "Pathspec 'api' did not match any files"
    );
}

#[test]
fn long_failures_are_cut_short() {
    let summary = summary(&format!("error: {}", "a".repeat(300)));

    assert_eq!(summary.chars().count(), 100);
    assert!(summary.starts_with('A'));
    assert!(summary.ends_with('…'));
}
