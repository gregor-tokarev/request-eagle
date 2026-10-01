use std::collections::HashMap;

use crate::{HttpRequest, Method};

fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

#[test]
fn writes_requests_like_postman_snippets() {
    let request = HttpRequest {
        method: Method::Post,
        path: "{{base}}/pets".into(),
        headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
        body: Some(b"{\n  \"name\": \"Rex's\"\n}".to_vec()),
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(&values(&[("base", "https://pets.test"), ("token", "abc")])),
        "curl --location 'https://pets.test/pets' \\\n\
         --header 'Authorization: Bearer abc' \\\n\
         --header 'Content-Type: application/json' \\\n\
         --data '{\n  \"name\": \"Rex'\\''s\"\n}'"
    );
}

#[test]
fn names_the_method_only_when_curl_would_not_choose_it() {
    let command = |method, body: Option<&str>| {
        HttpRequest {
            method,
            path: "example.com".into(),
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: body.map(|body| body.as_bytes().to_vec()),
            ..HttpRequest::default()
        }
        .curl_command(&HashMap::new())
    };

    // The body of a GET request is not sent.
    assert_eq!(
        command(Method::Get, Some("x")),
        "curl --location 'https://example.com/' \\\n--header 'Content-Type: text/plain'"
    );
    assert!(command(Method::Post, Some("x")).starts_with("curl --location 'https://"));
    assert!(command(Method::Post, None).starts_with("curl --location --request POST 'https://"));
    assert!(command(Method::Put, Some("x")).starts_with("curl --location --request PUT 'https://"));
    assert!(command(Method::Delete, None).starts_with("curl --location --request DELETE '"));
    assert!(command(Method::Head, None).starts_with("curl --location --head 'https://"));
    assert!(command(Method::Patch, Some("@me")).ends_with("--data-raw '@me'"));
}

#[test]
fn adds_query_parameters_and_keeps_unknown_variables() {
    let request = HttpRequest {
        path: "http://example.com/search?lang=en#results".into(),
        query: vec![
            ("q".into(), "fish & chips".into()),
            ("page".into(), "{{page}}".into()),
            ("id".into(), "{{$guid}}".into()),
        ],
        headers: vec![
            ("X-Empty".into(), String::new()),
            ("X-Literal".into(), "{{!name}}".into()),
        ],
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(&values(&[("$guid", "fixed")])),
        "curl --location --globoff 'http://example.com/search?lang=en&q=fish+%26+chips&page={{page}}&id={{$guid}}' \\\n\
         --header 'X-Empty;' \\\n\
         --header 'X-Literal: {{name}}'"
    );

    let request = HttpRequest {
        path: "https://example.com".into(),
        query: vec![("a".into(), "1".into())],
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(&HashMap::new()),
        "curl --location 'https://example.com/?a=1'"
    );
}

#[test]
fn writes_urls_as_sending_does() {
    let command = |path: &str| {
        HttpRequest {
            path: path.into(),
            ..HttpRequest::default()
        }
        .curl_command(&values(&[("term", "hello world")]))
    };

    // Sending encodes the space, including one a variable fills in.
    assert_eq!(
        command("https://example.com/search?q={{term}}"),
        "curl --location 'https://example.com/search?q=hello%20world'"
    );
    // cURL would otherwise expand `[1-3]` into three requests.
    assert_eq!(
        command("https://example.com/?filter[name]=Rex"),
        "curl --location --globoff 'https://example.com/?filter[name]=Rex'"
    );
}
