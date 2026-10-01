use std::time::Duration;

use request::{Execution, HeaderMap, HttpResponse, Response, StatusCode, Version};

use super::ResponseContent;

fn response(body: &[u8], content_type: &str) -> ResponseContent {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", content_type.parse().unwrap());

    ResponseContent::new(Execution {
        scripts: Vec::new(),
        elapsed: Duration::from_millis(239),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers,
            body: body.to_vec(),
            metrics: request::HttpMetrics::default(),
        }),
    })
}

#[test]
fn response_formatting_preserves_the_entire_body() {
    let json = response(b"{\"a\":1}", "application/json");
    assert_eq!(json.raw, "{\"a\":1}");
    assert_eq!(json.pretty.as_deref(), Some("{\n  \"a\": 1\n}"));
    assert_eq!(json.language, "json");
    assert_eq!(json.http().body, b"{\"a\":1}");
    assert_eq!(response(b"<html></html>", "text/html").language, "html");
    assert_eq!(response(b"1.2.3.4", "text/plain").language, "text");
    assert!(response(b"{broken", "application/json").pretty.is_none());

    let oversized = "a".repeat(1_048_575) + "é";
    let large = response(oversized.as_bytes(), "text/plain");
    assert_eq!(large.raw.as_ref(), oversized);
    assert_eq!(large.raw.len(), 1_048_577);
    assert_eq!(large.http().body.len(), 1_048_577);
}

#[test]
fn response_size_limits_lock_raw_for_long_lines_and_pretty_expansion() {
    let cases = [
        format!("\"{}\"", "a".repeat(32 * 1024)),
        format!("[\n{}0]", "0,\n".repeat(70_000)),
        format!("[\n{}0]", "0,\n".repeat(90_000)),
    ];
    for raw in cases {
        let content = response(raw.as_bytes(), "application/json");
        assert!(content.raw_only);
        assert!(content.pretty.is_none());
        assert_eq!(content.raw.as_ref(), raw);
    }
    assert!(!response(&vec![b'a'; 32 * 1024], "text/plain").raw_only);
    let at_limit = "1234567\n".repeat(32 * 1024);
    assert!(!response(at_limit.as_bytes(), "text/plain").raw_only);
}
