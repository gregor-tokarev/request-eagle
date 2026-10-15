use crate::shell_path::{marked, merge};

#[test]
fn reads_the_path_past_what_a_profile_prints() {
    assert_eq!(
        marked("Welcome back!\n__REQUEST_EAGLE_PATH__/opt/bin:/usr/bin__REQUEST_EAGLE_PATH__"),
        Some("/opt/bin:/usr/bin")
    );
    assert_eq!(marked("no markers here"), None);
    assert_eq!(marked("__REQUEST_EAGLE_PATH____REQUEST_EAGLE_PATH__"), None);
}

#[cfg(unix)]
#[test]
fn keeps_the_login_shells_order_and_adds_the_apps_directories() {
    assert_eq!(
        merge(
            "/Users/me/.local/bin:/opt/homebrew/bin:/usr/bin",
            "/usr/bin:/bin:/opt/homebrew/bin"
        ),
        "/Users/me/.local/bin:/opt/homebrew/bin:/usr/bin:/bin"
    );
    assert_eq!(merge("::/usr/bin:", ""), "/usr/bin");
}
