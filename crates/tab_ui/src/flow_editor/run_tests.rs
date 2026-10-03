use std::sync::Arc;

use serde_json::json;

use super::run::{LogKind, describe_response, failures};

#[test]
fn counts_failed_blocks_and_requests() {
    assert_eq!(failures(0, 0), "");
    assert_eq!(failures(1, 0), ", 1 failed");
    assert_eq!(failures(0, 1), ", 1 request failed");
    assert_eq!(failures(2, 3), ", 2 failed, 3 requests failed");
}

#[test]
fn a_response_reads_as_its_status_and_body() {
    let response = |output: &str, status: u64| {
        vec![(
            output.to_owned(),
            Arc::new(json!({
                "body": {"error": "unauthorized"},
                "http": {"status": status, "headers": {"server": "test"}},
            })),
        )]
    };

    let (kind, text) = describe_response(&response("fail", 401));
    assert!(kind == LogKind::Failed);
    assert_eq!(text.as_ref(), r#"401 · fail: {"error":"unauthorized"}"#);

    let (kind, text) = describe_response(&response("success", 200));
    assert!(kind == LogKind::Ran);
    assert!(text.starts_with("200 · success: "));
    // Headers are left out of the line.
    assert!(!text.contains("server"));
}
