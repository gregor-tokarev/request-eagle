use std::collections::HashMap;

use crate::{
    Body, CookieJar, Field, FormPart, HttpRequest, HttpSettings, HttpVersion, Method, RawLanguage,
    RequestPreferences,
};

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
        headers: vec![Field::new("Authorization", "Bearer {{token}}")],
        body: Some(Body::json("{\n  \"name\": \"Rex's\"\n}")),
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(
            &values(&[("base", "https://pets.test"), ("token", "abc")]),
            None,
            &RequestPreferences::default(),
        ),
        "curl --location 'https://pets.test/pets' \\\n\
         --header 'Authorization: Bearer abc' \\\n\
         --header 'Content-Type: application/json' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --data '{\n  \"name\": \"Rex'\\''s\"\n}'"
    );
}

#[test]
fn names_the_method_only_when_curl_would_not_choose_it() {
    let command = |method, body: Option<&str>| {
        HttpRequest {
            method,
            path: "example.com".into(),
            headers: vec![Field::new("Content-Type", "text/plain")],
            body: body.map(Body::json),
            ..HttpRequest::default()
        }
        .curl_command(&HashMap::new(), None, &RequestPreferences::default())
    };

    // The body of a GET request is not sent.
    assert_eq!(
        command(Method::Get, Some("x")),
        "curl --location 'https://example.com/' \\\n\
         --header 'Content-Type: text/plain' \\\n\
         --header 'User-Agent: RequestEagle'"
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
            Field::new("q", "fish & chips"),
            Field::new("page", "{{page}}"),
            Field::new("id", "{{$guid}}"),
        ],
        headers: vec![
            Field::new("X-Empty", String::new()),
            Field::new("X-Literal", "{{!name}}"),
        ],
        ..HttpRequest::default()
    };

    assert_eq!(
        request.curl_command(
            &values(&[("$guid", "fixed")]),
            None,
            &RequestPreferences::default()
        ),
        "curl --location --globoff 'http://example.com/search?lang=en&q=fish+%26+chips&page={{page}}&id={{$guid}}' \\\n\
         --header 'X-Empty;' \\\n\
         --header 'X-Literal: {{name}}' \\\n\
         --header 'User-Agent: RequestEagle'"
    );

    let request = HttpRequest {
        path: "https://example.com".into(),
        query: vec![Field::new("a", "1")],
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), None, &RequestPreferences::default()),
        "curl --location 'https://example.com/?a=1' \\\n--header 'User-Agent: RequestEagle'"
    );
}

#[test]
fn writes_urls_as_sending_does() {
    let command = |path: &str| {
        HttpRequest {
            path: path.into(),
            ..HttpRequest::default()
        }
        .curl_command(
            &values(&[("term", "hello world")]),
            None,
            &RequestPreferences::default(),
        )
    };

    // Sending encodes a variable's value within its query parameter.
    assert_eq!(
        command("https://example.com/search?q={{term}}"),
        "curl --location 'https://example.com/search?q=hello+world' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
    // A space written in the URL is encoded when the URL is parsed.
    assert_eq!(
        command("https://example.com/search?q=hello world"),
        "curl --location 'https://example.com/search?q=hello%20world' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
    // cURL would otherwise expand `[1-3]` into three requests.
    assert_eq!(
        command("https://example.com/?filter[name]=Rex"),
        "curl --location --globoff 'https://example.com/?filter[name]=Rex' \\\n\
         --header 'User-Agent: RequestEagle'"
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
        request.curl_command(
            &values(&[("term", "a&b c"), ("user", "42")]),
            None,
            &RequestPreferences::default()
        ),
        "curl --location --globoff 'https://example.com/users/42/posts?q=a%26b+c&page={{page}}' \\\n\
         --header 'User-Agent: RequestEagle'"
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
        request.curl_command(
            &values(&[("host", "https://example.com")]),
            None,
            &RequestPreferences::default()
        ),
        "curl --location 'https://example.com/users' \\\n\
         --header 'User-Agent: RequestEagle'"
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
        request.curl_command(&HashMap::new(), None, &RequestPreferences::default()),
        "curl --location --globoff 'https://example.com/users/{{user}}/posts/7' \\\n\
         --header 'User-Agent: RequestEagle'"
    );

    // The value is encoded as sending encodes it, around the reference.
    let request = HttpRequest {
        path: "https://example.com/files/:file/details".into(),
        path_variables: vec![("file".into(), "report#{{version}}".into())],
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), None, &RequestPreferences::default()),
        "curl --location --globoff 'https://example.com/files/report%23{{version}}/details' \\\n\
         --header 'User-Agent: RequestEagle'"
    );

    // A path variable that a URL variable brings in is filled in too.
    let request = HttpRequest {
        path: "{{base}}/:id".into(),
        path_variables: vec![("id".into(), "{{user}}".into())],
        ..HttpRequest::default()
    };
    assert_eq!(
        request.curl_command(
            &values(&[("base", "https://example.com/users/:id")]),
            None,
            &RequestPreferences::default()
        ),
        "curl --location --globoff 'https://example.com/users/{{user}}/{{user}}' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
}

#[test]
fn writes_the_request_settings_that_curl_has_options_for() {
    let command = |settings, preferences: &RequestPreferences| {
        HttpRequest {
            path: "https://pets.test".into(),
            settings,
            ..HttpRequest::default()
        }
        .curl_command(&HashMap::new(), None, preferences)
    };
    let strict = RequestPreferences {
        timeout_ms: 2000,
        follow_all_redirects: false,
        ssl_certificate_verification: true,
        http_version: HttpVersion::Http1_1,
        ..RequestPreferences::default()
    };
    let lenient = RequestPreferences {
        timeout_ms: 0,
        follow_all_redirects: true,
        ssl_certificate_verification: false,
        http_version: HttpVersion::Http2,
        ..RequestPreferences::default()
    };

    // A request's settings take precedence over the preferences.
    assert_eq!(
        command(
            HttpSettings {
                timeout_ms: Some(1500),
                follow_redirects: Some(false),
                verify_certificates: Some(false),
                ..HttpSettings::default()
            },
            &lenient
        ),
        "curl --insecure --max-time 1.5 --http2-prior-knowledge 'https://pets.test/' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
    assert_eq!(
        command(
            HttpSettings {
                timeout_ms: Some(0),
                follow_redirects: Some(true),
                verify_certificates: Some(true),
                ..HttpSettings::default()
            },
            &strict
        ),
        "curl --location --http1.1 'https://pets.test/' \\\n\
         --header 'User-Agent: RequestEagle'"
    );

    // Without settings, the preferences apply.
    assert_eq!(
        command(HttpSettings::default(), &strict),
        "curl --max-time 2 --http1.1 'https://pets.test/' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
    assert_eq!(
        command(HttpSettings::default(), &lenient),
        "curl --location --insecure --http2-prior-knowledge 'https://pets.test/' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
}

#[test]
fn sends_the_headers_sending_adds_that_curl_would_not() {
    let command = |method, headers: Vec<Field>| {
        HttpRequest {
            method,
            path: "https://pets.test".into(),
            headers,
            ..HttpRequest::default()
        }
        .curl_command(&HashMap::new(), None, &RequestPreferences::default())
    };

    // cURL would name itself, and would send no length without a body.
    assert_eq!(
        command(Method::Patch, Vec::new()),
        "curl --location --request PATCH 'https://pets.test/' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --header 'Content-Length: 0'"
    );
    // The request's own headers take precedence.
    assert_eq!(
        command(
            Method::Put,
            vec![
                Field::new("user-agent", "Mozilla/5.0"),
                Field::new("Transfer-Encoding", "chunked"),
            ]
        ),
        "curl --location --request PUT 'https://pets.test/' \\\n\
         --header 'user-agent: Mozilla/5.0' \\\n\
         --header 'Transfer-Encoding: chunked'"
    );
    assert!(!command(Method::Delete, Vec::new()).contains("Content-Length"));
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
        request.curl_command(&HashMap::new(), Some(&jar), &RequestPreferences::default()),
        "curl --location 'https://pets.test/pets' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --header 'Cookie: sid=abc; own=jar'"
    );

    // A cookie the request sets itself takes precedence.
    let request = HttpRequest {
        headers: vec![Field::new("Cookie", "own=typed")],
        ..request
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), Some(&jar), &RequestPreferences::default()),
        "curl --location 'https://pets.test/pets' \\\n\
         --header 'Cookie: own=typed; sid=abc' \\\n\
         --header 'User-Agent: RequestEagle'"
    );
    assert_eq!(
        request.curl_command(&HashMap::new(), None, &RequestPreferences::default()),
        "curl --location 'https://pets.test/pets' \\\n\
         --header 'Cookie: own=typed' \\\n\
         --header 'User-Agent: RequestEagle'"
    );

    // A request that leaves out the jar's cookies sends only its own.
    let request = HttpRequest {
        settings: HttpSettings {
            send_cookies: false,
            ..HttpSettings::default()
        },
        ..request
    };
    assert_eq!(
        request.curl_command(&HashMap::new(), Some(&jar), &RequestPreferences::default()),
        "curl --location 'https://pets.test/pets' \\\n\
         --header 'Cookie: own=typed' \\\n\
         --header 'User-Agent: RequestEagle'"
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
            .curl_command(&HashMap::new(), Some(&jar), &RequestPreferences::default())
            .ends_with("--header 'Cookie: sid=abc'")
    );
    // Without a known host, no cookies apply.
    assert!(
        !request("{{base}}/pets")
            .curl_command(&HashMap::new(), Some(&jar), &RequestPreferences::default())
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
        .curl_command(
            &values(&[("name", "Rex & co")]),
            None,
            &RequestPreferences::default(),
        )
    };

    assert_eq!(
        command(Body::Raw {
            language: RawLanguage::Xml,
            text: "<pet>{{name}}</pet>".into(),
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --header 'Content-Type: application/xml' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --data '<pet>Rex & co</pet>'"
    );
    // cURL encodes the values; names it sends as written. Without a name, it
    // would leave out the `=`.
    assert_eq!(
        command(Body::UrlEncoded {
            fields: vec![
                ("pet name".into(), "{{name}}".into()),
                ("token".into(), "{{token}}".into()),
                ("".into(), "{{name}}".into()),
            ],
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --data-urlencode 'pet+name=Rex & co' \\\n\
         --data-urlencode 'token={{token}}' \\\n\
         --data-raw '=Rex+%26+co'"
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
         --header 'User-Agent: RequestEagle' \\\n\
         --form-string 'title=@Rex & co;type=text' \\\n\
         --form 'avatar=@\"/tmp/my \\\"eagle\\\";1.png\";type=image/png'"
    );
    // cURL would call the file a URL-encoded form.
    assert_eq!(
        command(Body::Binary {
            file: "/tmp/eagle.png".into(),
        }),
        "curl --location 'https://example.com/upload' \\\n\
         --header 'Content-Type: image/png' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --data-binary '@/tmp/eagle.png'"
    );
    // A form without fields sends nothing, so the method is named, and its
    // type and length are sent as sending sends them.
    assert_eq!(
        command(Body::UrlEncoded { fields: Vec::new() }),
        "curl --location --request POST 'https://example.com/upload' \\\n\
         --header 'Content-Type: application/x-www-form-urlencoded' \\\n\
         --header 'User-Agent: RequestEagle' \\\n\
         --header 'Content-Length: 0'"
    );
}

#[test]
fn signed_bodies_are_the_bytes_curl_sends() {
    let form = |fields: &[(&str, &str)]| {
        let body = Body::UrlEncoded {
            fields: fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        };
        String::from_utf8(crate::curl::sent_body(Some(&body)).unwrap()).unwrap()
    };

    // What cURL 8 sent for each `--data-urlencode` of these fields, and for
    // the `--data-raw` of a field without a name.
    assert_eq!(form(&[("a b", "c d")]), "a+b=c+d");
    assert_eq!(form(&[("", "c d")]), "=c+d");
    assert_eq!(
        form(&[("x", "~!*()'@#$&+/")]),
        "x=~%21%2A%28%29%27%40%23%24%26%2B%2F"
    );
    assert_eq!(form(&[("a", "1"), ("b", "2 3")]), "a=1&b=2+3");

    assert_eq!(crate::curl::sent_body(None), Some(Vec::new()));
    assert_eq!(
        crate::curl::sent_body(Some(&Body::Binary {
            file: "data.bin".into()
        })),
        None
    );
}
