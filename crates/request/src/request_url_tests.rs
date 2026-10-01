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
    assert_eq!(names("https://example.com/users/:{{name}}"), []);
}

#[test]
fn path_variables_are_filled_when_they_have_a_value() {
    let values = pairs(&[("id", "a/b?c#d{e}"), ("empty", ""), ("unused", "x")]);
    let filled = fill_path_variables(
        "https://example.com:8080/:id/:empty/:id?id=:id",
        &values,
        |value| Ok::<_, ()>(value.to_owned()),
    );

    assert_eq!(
        filled.unwrap(),
        "https://example.com:8080/a/b%3Fc%23d%7Be%7D/:empty/a/b%3Fc%23d%7Be%7D?id=:id"
    );
}

fn resolve(request: HttpRequest, values: &[(&str, &str)]) -> String {
    let values = values
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>();

    RequestVariables::new(values, None)
        .resolve(&request)
        .unwrap()
        .path
}

#[test]
fn sending_resolves_variables_in_path_variable_values() {
    let request = HttpRequest {
        path: "{{base}}/users/:id/:missing".into(),
        path_variables: pairs(&[("id", "{{user}}")]),
        ..Default::default()
    };
    let values = [("base", "https://example.com"), ("user", "42")];
    assert_eq!(
        resolve(request.clone(), &values),
        "https://example.com/users/42/:missing"
    );

    // A resolved value stays in its path segment.
    let request = HttpRequest {
        path: "https://example.com/files/:id/end?x=1".into(),
        path_variables: pairs(&[("id", "{{file}}")]),
        ..Default::default()
    };
    assert_eq!(
        resolve(request, &[("file", "a#b?c{{d}}")]),
        "https://example.com/files/a%23b%3Fc%7B%7Bd%7D%7D/end?x=1"
    );

    // A value after a fragment is not sent, so it is not resolved.
    let request = HttpRequest {
        path: "{{endpoint}}/files/:id?q=1".into(),
        path_variables: pairs(&[("id", "{{missing}}")]),
        ..Default::default()
    };
    assert_eq!(
        resolve(request, &[("endpoint", "https://example.com/path#top")]),
        "https://example.com/path?q=1"
    );
}

#[test]
fn variables_in_the_query_stay_within_their_key_or_value() {
    let request = HttpRequest {
        path: "https://example.com/{{segment}}?q={{term}}&{{key}}=a b+{{!literal}}#{{ignored}}"
            .into(),
        ..Default::default()
    };

    assert_eq!(
        resolve(
            request,
            &[("segment", "a/b"), ("term", "x&y=z #1+2%"), ("key", "k=v")]
        ),
        "https://example.com/a/b?q=x%26y%3Dz+%231%2B2%25&k%3Dv=a b+{{literal}}"
    );
}

#[test]
fn a_whole_url_variable_keeps_the_query_written_after_it() {
    let request = HttpRequest {
        path: "{{endpoint}}/{{missing}}?q=2".into(),
        ..Default::default()
    };

    assert_eq!(
        resolve(
            request.clone(),
            &[("endpoint", "https://example.com/path?old=1#top")]
        ),
        "https://example.com/path?old=1&q=2"
    );

    let request = HttpRequest {
        path: "{{endpoint}}?q=2".into(),
        ..request
    };
    assert_eq!(
        resolve(request, &[("endpoint", "https://example.com/path?old=1")]),
        "https://example.com/path?old=1&q=2"
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
        append_encoded_query(" https://example.com/pets ", &params[..1]),
        "https://example.com/pets?q=a+%26+b%2B{{term}}"
    );
}

#[test]
fn query_params_moved_into_the_url_resolve_as_they_were_sent() {
    let mut request = HttpRequest {
        path: "https://example.com/pets ".into(),
        query: pairs(&[("q", "{{term}} &"), ("{{key}}", "1")]),
        ..Default::default()
    };
    let values = [("term", "a&b+c d%"), ("key", "k=v")];
    let mut sent = request.clone().prepare_for_send();
    sent.path = resolve(sent.clone(), &values);
    let mut separate = url::Url::parse(&sent.path).unwrap();
    let pairs: Vec<_> = sent
        .query
        .iter()
        .map(|(key, value)| (resolve_text(key, &values), resolve_text(value, &values)))
        .collect();
    separate.query_pairs_mut().extend_pairs(&pairs);

    request.inline_query();
    let inline = resolve(request.prepare_for_send(), &values);

    assert_eq!(inline, separate.as_str());
    assert_eq!(
        inline,
        "https://example.com/pets?q=a%26b%2Bc+d%25+%26&k%3Dv=1"
    );
}

fn resolve_text(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(&format!("{{{{{name}}}}}"), value)
    })
}
