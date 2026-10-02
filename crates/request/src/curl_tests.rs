use std::collections::HashMap;

use crate::{Body, CookieJar, FormPart, HttpRequest, HttpSettings, Method, RawLanguage};

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
        body: Some(Body::json("{\n  \"name\": \"Rex's\"\n}")),
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(
            &values(&[("base", "https://pets.test"), ("token", "abc")]),
            None
        ),
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
            body: body.map(Body::json),
            ..HttpRequest::default()
        }
        .curl_command(&HashMap::new(), None)
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
        request.curl_command(&values(&[("$guid", "fixed")]), None),
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
        request.curl_command(&HashMap::new(), None),
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
        .curl_command(&values(&[("term", "hello world")]), None)
    };

    // Sending encodes a variable's value within its query parameter.
    assert_eq!(
        command("https://example.com/search?q={{term}}"),
        "curl --location 'https://example.com/search?q=hello+world'"
    );
    // A space written in the URL is encoded when the URL is parsed.
    assert_eq!(
        command("https://example.com/search?q=hello world"),
        "curl --location 'https://example.com/search?q=hello%20world'"
    );
    // cURL would otherwise expand `[1-3]` into three requests.
    assert_eq!(
        command("https://example.com/?filter[name]=Rex"),
        "curl --location --globoff 'https://example.com/?filter[name]=Rex'"
    );
}

#[test]
fn fills_variables_as_sending_does() {
    let request = HttpRequest {
        method: Method::Get,
        path: "https://example.com/users/:id/posts?q={{term}}&page={{page}}".into(),
        path_variables: vec![("id".into(), "{{user}}".into())],
        ..HttpRequest::default()
    };

    // A value in the query is encoded within its parameter, a path variable
    // is filled in, and an unknown variable stays as written.
    assert_eq!(
        request.curl_command(&values(&[("term", "a&b c"), ("user", "42")]), None),
        "curl --location --globoff 'https://example.com/users/42/posts?q=a%26b+c&page={{page}}'"
    );
}

#[test]
fn leaves_out_what_sending_leaves_out_before_filling_variables() {
    // A GET body is not sent, so an unfinished reference in it does not
    // keep the URL from being filled in.
    let request = HttpRequest {
        method: Method::Get,
        path: "{{host}}/users".into(),
        body: Some(Body::json("{{unfinished")),
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(&values(&[("host", "https://example.com")]), None),
        "curl --location 'https://example.com/users'"
    );
}

#[test]
fn keeps_unknown_references_of_path_variables_readable() {
    let request = HttpRequest {
        path: "https://example.com/users/:id/posts/:post".into(),
        path_variables: vec![
            ("id".into(), "{{user}}".into()),
            ("post".into(), "7".into()),
        ],
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(&HashMap::new(), None),
        "curl --location --globoff 'https://example.com/users/{{user}}/posts/7'"
    );

    // The value is encoded as sending encodes it, around the reference.
    let request = HttpRequest {
        path: "https://example.com/files/:file/details".into(),
        path_variables: vec![("file".into(), "report#{{version}}".into())],
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), None),
        "curl --location --globoff 'https://example.com/files/report%23{{version}}/details'"
    );

    // A path variable that a URL variable brings in is filled in too.
    let request = HttpRequest {
        path: "{{base}}/:id".into(),
        path_variables: vec![("id".into(), "{{user}}".into())],
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(&values(&[("base", "https://example.com/users/:id")]), None),
        "curl --location --globoff 'https://example.com/users/{{user}}/{{user}}'"
    );
}

#[test]
fn writes_the_request_settings_that_curl_has_options_for() {
    let command = |settings| {
        HttpRequest {
            path: "https://pets.test".into(),
            settings,
            ..HttpRequest::default()
        }
        .curl_command(&HashMap::new(), None)
    };

    assert_eq!(
        command(HttpSettings {
            timeout_ms: Some(1500),
            follow_redirects: Some(false),
            verify_certificates: Some(false),
        }),
        "curl --insecure --max-time 1.5 'https://pets.test/'"
    );
    assert_eq!(
        command(HttpSettings {
            timeout_ms: Some(0),
            follow_redirects: Some(true),
            verify_certificates: Some(true),
        }),
        "curl --location 'https://pets.test/'"
    );
}

#[test]
fn includes_the_jar_cookies_for_the_url() {
    let jar = CookieJar::new();
    let url = url::Url::parse("https://pets.test/").unwrap();
    jar.set(&url, "sid=abc; Path=/").unwrap();
    jar.set(&url, "own=jar; Path=/").unwrap();

    let request = HttpRequest {
        path: "https://pets.test/pets".into(),
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), Some(&jar)),
        "curl --location 'https://pets.test/pets' \\\n--header 'Cookie: sid=abc; own=jar'"
    );

    // A cookie the request sets itself takes precedence.
    let request = HttpRequest {
        headers: vec![("Cookie".into(), "own=typed".into())],
        ..request
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), Some(&jar)),
        "curl --location 'https://pets.test/pets' \\\n--header 'Cookie: own=typed; sid=abc'"
    );
    assert_eq!(
        request.curl_command(&HashMap::new(), None),
        "curl --location 'https://pets.test/pets' \\\n--header 'Cookie: own=typed'"
    );
}

#[test]
fn includes_the_jar_cookies_beside_literal_braces() {
    let jar = CookieJar::new();
    jar.set(
        &url::Url::parse("https://pets.test/").unwrap(),
        "sid=abc; Path=/",
    )
    .unwrap();
    let request = |path: &str| HttpRequest {
        path: path.into(),
        ..HttpRequest::default()
    };

    assert!(
        request("https://pets.test/?q={{!literal}}")
            .curl_command(&HashMap::new(), Some(&jar))
            .ends_with("--header 'Cookie: sid=abc'")
    );
    // Without a known host, no cookies apply.
    assert!(
        !request("{{base}}/pets")
            .curl_command(&HashMap::new(), Some(&jar))
            .contains("Cookie")
    );
}

#[test]
fn writes_each_body_type_with_the_option_that_sends_it() {
    let command = |body| {
        HttpRequest {
            method: Method::Post,
            path: "https://example.com/upload".into(),
            body: Some(body),
            ..HttpRequest::default()
        }
        .curl_command(&values(&[("name", "Rex & co")]), None)
    };

    assert_eq!(
        command(Body::Raw {
            language: RawLanguage::Xml,
            text: "<pet>{{name}}</pet>".into(),
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --header 'Content-Type: application/xml' \\\n\
         --data '<pet>Rex & co</pet>'"
    );
    // cURL encodes the values; names it sends as written.
    assert_eq!(
        command(Body::UrlEncoded {
            fields: vec![
                ("pet name".into(), "{{name}}".into()),
                ("token".into(), "{{token}}".into()),
            ],
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --data-urlencode 'pet+name=Rex & co' \\\n\
         --data-urlencode 'token={{token}}'"
    );
    assert_eq!(
        command(Body::Multipart {
            parts: vec![
                FormPart {
                    name: "title".into(),
                    value: "@{{name}};type=text".into(),
                    file: false,
                },
                FormPart {
                    name: "avatar".into(),
                    value: "/tmp/my \"eagle\";1.png".into(),
                    file: true,
                },
            ],
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --form-string 'title=@Rex & co;type=text' \\\n\
         --form 'avatar=@\"/tmp/my \\\"eagle\\\";1.png\"'"
    );
    // cURL would call the file a URL-encoded form.
    assert_eq!(
        command(Body::Binary {
            file: "/tmp/eagle.png".into(),
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --header 'Content-Type: image/png' \\\n\
         --data-binary '@/tmp/eagle.png'"
    );
    // A form without fields sends nothing, so the method is named.
    assert_eq!(
        command(Body::UrlEncoded { fields: Vec::new() }),
        "curl --location --request POST 'https://example.com/upload'"
    );
}
