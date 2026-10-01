use std::collections::HashMap;

use request::{HttpRequest, Method};

use crate::{CurlError, is_curl, parse_curl};

fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

#[test]
fn recognizes_curl_commands_but_not_urls() {
    assert!(is_curl("curl https://example.com"));
    assert!(is_curl("  curl\\\n  https://example.com"));
    assert!(!is_curl("curl"));
    assert!(!is_curl("curl.se/docs"));
    assert!(!is_curl("https://example.com/curl"));
    assert_eq!(parse_curl("https://example.com"), Err(CurlError::NotCurl));
}

#[test]
fn reads_a_command_written_over_several_lines() {
    let request = parse_curl(
        "curl --location --request PUT 'https://api.example.com/pets/1?verbose=true' \\\n\
         --header 'Content-Type: application/json' \\\n\
         --header \"Authorization: Bearer abc\" \\\n\
         --data '{\n    \"name\": \"Rex\"\n}'",
    )
    .unwrap();

    assert_eq!(
        request,
        HttpRequest {
            method: Method::Put,
            path: "https://api.example.com/pets/1?verbose=true".into(),
            headers: headers(&[
                ("Content-Type", "application/json"),
                ("Authorization", "Bearer abc"),
            ]),
            body: Some(b"{\n    \"name\": \"Rex\"\n}".to_vec()),
            ..HttpRequest::default()
        }
    );
}

#[test]
fn reads_browser_copies_with_ansi_c_quoting() {
    // Chrome writes bodies with control characters or quotes as $'…'.
    let request = parse_curl(
        "curl 'https://example.com/api' \\\r\n  -H 'accept: */*' \\\r\n  -H 'content-type: text/plain' \\\r\n  --data-raw $'it\\'s\\n\\u00e9 \\xc3\\xa9 \\101' \\\r\n  --compressed",
    )
    .unwrap();

    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.headers,
        headers(&[("accept", "*/*"), ("content-type", "text/plain")])
    );
    assert_eq!(request.body.as_deref(), Some("it's\né é A".as_bytes()));
}

#[test]
fn reads_combined_and_attached_short_options() {
    let request = parse_curl("curl -sSLXPATCH -HAccept:text/html -d x=1 example.com/a").unwrap();

    assert_eq!(request.method, Method::Patch);
    assert_eq!(request.path, "http://example.com/a");
    assert_eq!(
        request.headers,
        headers(&[
            ("Accept", "text/html"),
            ("Content-Type", "application/x-www-form-urlencoded"),
        ])
    );
    assert_eq!(request.body.as_deref(), Some(&b"x=1"[..]));
}

#[test]
fn skips_values_of_options_that_do_not_change_the_request() {
    let request = parse_curl(
        "curl -o out.json --max-time 10 -w '%{http_code}' --url https://example.com/x --retry 3",
    )
    .unwrap();

    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "https://example.com/x");
    assert!(request.headers.is_empty());
    assert!(request.body.is_none());
}

#[test]
fn ends_the_command_at_shell_operators() {
    let request =
        parse_curl("curl https://example.com/a?b=1&c=2 -H 'X: 1' | jq -r '.name' > out.json")
            .unwrap();

    assert_eq!(request.path, "https://example.com/a?b=1&c=2");
    assert_eq!(request.headers, headers(&[("X", "1")]));
}

#[test]
fn joins_data_and_sends_it_in_the_query_with_get() {
    let request = parse_curl(
        "curl -d a=1 --data-urlencode 'q=hello world&more' --data-urlencode =x/y https://example.com",
    )
    .unwrap();
    assert_eq!(
        request.body.as_deref(),
        Some(&b"a=1&q=hello%20world%26more&x%2Fy"[..])
    );

    let request = parse_curl("curl -G -d a=1 -d b=2 'https://example.com/s?x=0'").unwrap();
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "https://example.com/s?x=0&a=1&b=2");
    assert!(request.body.is_none());
    assert!(request.headers.is_empty());
}

#[test]
fn reads_json_forms_and_credentials() {
    let request = parse_curl(r#"curl --json '{"a":1}' https://example.com"#).unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.headers,
        headers(&[
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
        ])
    );

    let request = parse_curl(
        "curl -u user:secret -A agent -e https://ref.example -b 'a=1' --cookie b=2 \
         -F name=Rex -F 'note=a=b' https://example.com",
    )
    .unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.headers,
        headers(&[
            ("User-Agent", "agent"),
            ("Referer", "https://ref.example"),
            ("Authorization", "Basic dXNlcjpzZWNyZXQ="),
            ("Cookie", "a=1; b=2"),
            (
                "Content-Type",
                "multipart/form-data; boundary=RequestEagleFormBoundary"
            ),
        ])
    );
    let body = String::from_utf8(request.body.unwrap()).unwrap();
    assert!(body.contains("name=\"name\"\r\n\r\nRex\r\n"));
    assert!(body.contains("name=\"note\"\r\n\r\na=b\r\n"));
}

#[test]
fn reads_header_forms_and_head_requests() {
    let request =
        parse_curl("curl -I -H 'Empty;' -H 'Removed:' -H 'X-Pad:   v  ' localhost:3000").unwrap();

    assert_eq!(request.method, Method::Head);
    assert_eq!(request.path, "http://localhost:3000");
    assert_eq!(request.headers, headers(&[("Empty", ""), ("X-Pad", "v")]));
}

#[test]
fn explains_what_it_cannot_import() {
    assert_eq!(
        parse_curl("curl 'https://example.com"),
        Err(CurlError::UnclosedQuote)
    );
    assert_eq!(
        parse_curl("curl -X"),
        Err(CurlError::MissingValue("-X".into()))
    );
    assert_eq!(parse_curl("curl -s -L"), Err(CurlError::MissingUrl));
    assert_eq!(
        parse_curl("curl -X TRACE https://example.com"),
        Err(CurlError::UnsupportedMethod("TRACE".into()))
    );
    assert_eq!(
        parse_curl("curl -F file=@photo.jpg https://example.com"),
        Err(CurlError::File("--form file=@photo.jpg".into()))
    );
    assert_eq!(
        parse_curl("curl --data-binary @body.json https://example.com"),
        Err(CurlError::File("--data-binary @body.json".into()))
    );
    assert_eq!(
        parse_curl("curl -d a=1 -F b=2 https://example.com"),
        Err(CurlError::FormAndData)
    );
    // Request Eagle leaves out GET bodies, so importing would change the request.
    assert_eq!(
        parse_curl("curl -X GET -d '{\"query\":{}}' https://example.com/_search"),
        Err(CurlError::BodyWithoutMethod("GET"))
    );
}

#[test]
fn reads_values_attached_to_long_options() {
    let request = parse_curl("curl --request=PATCH --data-raw=x=1 https://example.com").unwrap();

    assert_eq!(request.method, Method::Patch);
    assert_eq!(request.body.as_deref(), Some(&b"x=1"[..]));
}

#[test]
fn joins_data_as_curl_does() {
    // `--json` continues the previous data, while `--data` adds a field.
    let request = parse_curl("curl --json '{\"a\":' --json '1}' https://example.com").unwrap();
    assert_eq!(request.body.as_deref(), Some(&b"{\"a\":1}"[..]));

    // `--get` adds the query before the fragment, which is not sent.
    let request = parse_curl("curl -G -d a=1 'https://example.com/path#top'").unwrap();
    assert_eq!(request.path, "https://example.com/path?a=1#top");
}

#[test]
fn reads_form_fields_and_their_content_type() {
    let request = parse_curl(
        "curl -H 'Content-Type: multipart/form-data' -F 'name=Rex;type=text/plain' \
         -F 'note=\"a;b \\\"c\\\"\"' --form-string 'raw=x;y' https://example.com",
    )
    .unwrap();

    assert_eq!(
        request.headers,
        headers(&[(
            "Content-Type",
            "multipart/form-data; boundary=RequestEagleFormBoundary"
        )])
    );
    let body = String::from_utf8(request.body.unwrap()).unwrap();
    assert!(body.contains("name=\"name\"\r\n\r\nRex\r\n"), "{body}");
    assert!(
        body.contains("name=\"note\"\r\n\r\na;b \"c\"\r\n"),
        "{body}"
    );
    assert!(body.contains("name=\"raw\"\r\n\r\nx;y\r\n"), "{body}");

    // Without its closing quote, cURL sends the value as it is written.
    let request = parse_curl("curl -F 'name=\"Rex' https://example.com").unwrap();
    let body = String::from_utf8(request.body.unwrap()).unwrap();
    assert!(body.contains("name=\"name\"\r\n\r\n\"Rex\r\n"), "{body}");
}

#[test]
fn leaves_out_headers_the_command_removes() {
    let request = parse_curl("curl --json '{}' -H 'Accept:' https://example.com").unwrap();

    assert_eq!(
        request.headers,
        headers(&[("Content-Type", "application/json")])
    );
}

#[test]
fn reads_the_commands_request_eagle_writes() {
    let requests = [
        HttpRequest {
            method: Method::Put,
            path: "https://example.com/pets?x=1".into(),
            headers: headers(&[("Content-Type", "application/json"), ("X-Empty", "")]),
            body: Some(br#"{"it's": "@home"}"#.to_vec()),
            ..HttpRequest::default()
        },
        HttpRequest {
            method: Method::Post,
            path: "https://example.com/form".into(),
            headers: headers(&[("Content-Type", "text/plain")]),
            body: Some(b"line one\nline two".to_vec()),
            ..HttpRequest::default()
        },
        HttpRequest {
            method: Method::Head,
            path: "http://localhost:3000/".into(),
            ..HttpRequest::default()
        },
    ];

    for request in requests {
        let command = request.curl_command(&HashMap::new());
        assert_eq!(parse_curl(&command).as_ref(), Ok(&request), "{command}");
    }
}
