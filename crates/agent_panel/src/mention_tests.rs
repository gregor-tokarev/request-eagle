use std::path::PathBuf;

use crate::mention::{Mention, MentionKind, matching, typed_mention};

fn request(name: &str, method: &'static str, url: &str, folders: &[&str]) -> Mention {
    Mention {
        kind: MentionKind::Request {
            method,
            url: url.to_owned(),
            collection: "Shop".to_owned(),
            folders: folders.iter().map(|folder| (*folder).to_owned()).collect(),
        },
        name: name.to_owned().into(),
        path: PathBuf::from(format!("/data/collections/Shop/{name}.toml")),
    }
}

fn flow(name: &str) -> Mention {
    Mention {
        kind: MentionKind::Flow,
        name: name.to_owned().into(),
        path: PathBuf::from(format!("/data/flows/{name}.toml")),
    }
}

fn names(mentions: &[Mention], query: &str) -> Vec<String> {
    matching(mentions, query)
        .into_iter()
        .map(|index| mentions[index].name.to_string())
        .collect()
}

#[test]
fn an_at_sign_starts_a_mention_at_a_word_start() {
    assert_eq!(typed_mention("@", 1, &[]), Some(0..1));
    assert_eq!(typed_mention("run @get", 8, &[]), Some(4..8));
    assert_eq!(typed_mention("run\n@get", 8, &[]), Some(4..8));
    // An address is not a mention.
    assert_eq!(typed_mention("mail me@home", 12, &[]), None);
    assert_eq!(typed_mention("no at sign", 10, &[]), None);
}

#[test]
fn with_the_caret_inside_a_mention_it_is_the_whole_word() {
    assert_eq!(typed_mention("run @get now", 6, &[]), Some(4..8));
    assert_eq!(typed_mention("run @get", 5, &[]), Some(4..8));
}

#[test]
fn a_mention_ends_between_whole_characters() {
    // U+0600 and the space after it are one character as people see it.
    assert_eq!(typed_mention("@get\u{0600} now", 3, &[]), Some(0..4));
}

#[test]
fn a_space_after_the_mention_ends_it() {
    assert_eq!(typed_mention("run @get user", 13, &[]), None);
    assert_eq!(typed_mention("run @get ", 9, &[]), None);
    // Back before the space, it is being typed again.
    assert_eq!(typed_mention("run @get user", 8, &[]), Some(4..8));
}

#[test]
fn a_mention_already_in_the_message_is_not_typed_again() {
    let text = "run @Health now";
    let token = 4..11;

    assert_eq!(typed_mention(text, 11, std::slice::from_ref(&token)), None);
    assert_eq!(typed_mention(text, 11, &[]), Some(4..11));
}

#[test]
fn a_mention_being_typed_ends_where_another_starts() {
    let text = "@ge@Health now";
    let token = 3..10;

    assert_eq!(
        typed_mention(text, 3, std::slice::from_ref(&token)),
        Some(0..3)
    );
}

#[test]
fn a_name_with_line_breaks_shows_on_one_line() {
    assert_eq!(flow("Unicode\u{2028}Flow").label(), "Unicode Flow");
    assert_eq!(flow("Sign\nup\u{2029}now").label(), "Sign up now");
    assert_eq!(flow("Checkout").label(), "Checkout");
}

#[test]
fn names_that_start_with_the_query_come_first() {
    let mentions = [
        request("List users", "GET", "/users", &[]),
        request("Get user", "GET", "/users/1", &[]),
        request("Forget password", "POST", "/password", &[]),
        request("Delete user", "DELETE", "/users/1", &[]),
    ];

    assert_eq!(
        names(&mentions, "get"),
        ["Get user", "Forget password", "List users"],
        "a name that starts with it, then one that has it, then a GET request"
    );
    assert_eq!(
        names(&mentions, "USER"),
        ["List users", "Get user", "Delete user"],
        "case does not matter, and each group keeps its order"
    );
}

#[test]
fn a_word_start_ranks_above_a_match_inside_a_word() {
    let mentions = [
        request("Reorder items", "POST", "/orders", &[]),
        request("Order status", "GET", "/orders/1", &[]),
        request("Cancel order", "POST", "/orders/1/cancel", &[]),
    ];

    assert_eq!(
        names(&mentions, "order"),
        ["Order status", "Cancel order", "Reorder items"]
    );
}

#[test]
fn spaces_in_names_do_not_stop_a_match() {
    let mentions = [
        request("Get user", "GET", "/users/1", &[]),
        request("Get users", "GET", "/users", &[]),
    ];

    assert_eq!(names(&mentions, "getusers"), ["Get users"]);
    assert_eq!(names(&mentions, "tuse"), ["Get user", "Get users"]);
}

#[test]
fn the_method_url_and_folders_match_after_names() {
    let mentions = [
        request("Sign in", "POST", "/auth/login", &["Auth"]),
        request("Health", "GET", "/health", &[]),
        request("Authorize app", "GET", "/oauth/authorize", &[]),
        flow("Onboarding"),
    ];

    assert_eq!(names(&mentions, "auth"), ["Authorize app", "Sign in"]);
    assert_eq!(names(&mentions, "post"), ["Sign in"]);
    assert_eq!(names(&mentions, "flow"), ["Onboarding"]);
    assert!(names(&mentions, "nothing").is_empty());
}

#[test]
fn an_empty_query_lists_everything_in_order() {
    let mentions = [request("Health", "GET", "/health", &[]), flow("Onboarding")];

    assert_eq!(names(&mentions, ""), ["Health", "Onboarding"]);
}

#[test]
fn the_menu_says_where_each_mention_is() {
    assert_eq!(
        request("Sign in", "POST", "/auth/login", &["Auth", "Legacy"]).location(),
        "Shop › Auth › Legacy"
    );
    assert_eq!(flow("Onboarding").location(), "Flow");
}
