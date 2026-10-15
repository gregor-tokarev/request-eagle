use crate::check::parse_version;

#[test]
fn reads_the_version_agents_print() {
    assert_eq!(
        parse_version("2.1.300 (Claude Code)\n").as_deref(),
        Some("2.1.300")
    );
    assert_eq!(
        parse_version("codex-cli 0.162.1").as_deref(),
        Some("0.162.1")
    );
    assert_eq!(parse_version("v1.18.32").as_deref(), Some("1.18.32"));
    assert_eq!(parse_version("unknown"), None);
}
