use std::collections::HashMap;

use crate::request_url::{
    append_encoded_query, fill_path_variables, path_variables, query_params, with_query_params,
};
use crate::{HttpRequest, RequestVariables};

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn query_params_are_read_as_written() {
    assert_eq!(
        query_params("https://example.com/search?q=owner%20name&flag&&page={{page}}#top"),
        pairs(&[("q", "owner%20name"), ("flag", ""), ("page", "{{page}}")])
    );
    assert_eq!(query_params("https://example.com/a=b"), []);
    assert_eq!(query_params("https://example.com/#?q=1"), []);
    assert_eq!(query_params("{{base}}?q=a=b"), pairs(&[("q", "a=b")]));
}

#[test]
fn query_params_replace_the_url_query_and_keep_its_fragment() {
    let url = "https://example.com/search?old=1#top";

    assert_eq!(
        with_query_params(url, &pairs(&[("q", "a b"), ("flag", ""), ("x", "{{x}}")])),
        "https://example.com/search?q=a b&flag&x={{x}}#top"
    );
    assert_eq!(
        with_query_params(url, &[]),
        "https://example.com/search#top"
    );
    assert_eq!(
        with_query_params("example.com/?", &pairs(&[("page", "2")])),
        "example.com/?page=2"
    );
    assert_eq!(
        with_query_params("example.com", &pairs(&[("page", "2")])),
        "example.com?page=2"
    );
}

#[test]
fn query_params_escape_only_what_would_end_them() {
    let params = pairs(&[("a=b&c#d", "e=f&g#h%20")]);
    let url = with_query_params("https://example.com/", &params);

    assert_eq!(url, "https://example.com/?a%3Db%26c%23d=e=f%26g%23h%20");
    assert_eq!(
        query_params(&url),
        pairs(&[("a%3Db%26c%23d", "e=f%26g%23h%20")])
    );
}

#[test]
fn path_variables_are_whole_path_segments_after_the_host() {
    let names = |url| {
        path_variables(url)
            .map(|(range, name)| (&url[range], name))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        names("https://user:secret@example.com:8443/users/:id/posts/:post_id?sort=:asc#:top"),
        [(":id", "id"), (":post_id", "post_id")]
    );
    assert_eq!(names("{{base_url}}/pets/:id"), [(":id", "id")]);
    assert_eq!(
        names("localhost:3000/:id.json/a:b/:/"),
        [(":id.json", "id.json")]
    );
    assert_eq!(
        names("/:first/:first"),
        [(":first", "first"), (":first", "first")]
    );
    assert_eq!(names("example.com:8080"), []);
}

#[test]
fn path_variables_are_filled_when_they_have_a_value() {
    let values = pairs(&[("id", "a/b?c#d"), ("empty", ""), ("unused", "x")]);

    assert_eq!(
        fill_path_variables("https://example.com:8080/:id/:empty/:id?id=:id", &values),
        "https://example.com:8080/a/b%3Fc%23d/:empty/a/b%3Fc%23d?id=:id"
    );
}

#[test]
fn sending_resolves_variables_in_path_variable_values() {
    let request = HttpRequest {
        path: "{{base}}/users/:id/:missing".into(),
        path_variables: pairs(&[("id", "{{user}}")]),
        ..Default::default()
    };
    let values = HashMap::from([
        ("base".to_owned(), "https://example.com".to_owned()),
        ("user".to_owned(), "42".to_owned()),
    ]);

    assert_eq!(
        RequestVariables::new(values, None)
            .resolve(&request)
            .unwrap()
            .path,
        "https://example.com/users/42/:missing"
    );
}

#[test]
fn appended_query_params_are_encoded_around_variables() {
    let params = pairs(&[("q", "a & b+{{term}}"), ("{{key}}", "")]);

    assert_eq!(
        append_encoded_query("https://example.com/?page=1#top", &params),
        "https://example.com/?page=1&q=a+%26+b%2B{{term}}&{{key}}=#top"
    );
    assert_eq!(
        append_encoded_query("https://example.com/?", &params[..1]),
        "https://example.com/?q=a+%26+b%2B{{term}}"
    );
    assert_eq!(
        append_encoded_query("https://example.com", &params[..1]),
        "https://example.com?q=a+%26+b%2B{{term}}"
    );
}
