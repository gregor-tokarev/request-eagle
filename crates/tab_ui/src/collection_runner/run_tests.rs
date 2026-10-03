use request::{
    ExecutionError, ExecutionFailure, NextRequest, ScriptLog, ScriptPhase, ScriptReport,
};

use super::run::{
    Cursor, Kept, Outcome, Position, RunRequest, RunResult, Totals, chosen_request, count_label,
    duration_label, failure_summary,
};

fn requests(names: &[&str]) -> Vec<RunRequest> {
    names
        .iter()
        .map(|name| RunRequest {
            path: format!("{name}.toml").into(),
            id: format!("{name}-id").into(),
            name: (*name).into(),
            folders: Vec::new(),
            request: Default::default(),
        })
        .collect()
}

fn at(iteration: usize, index: usize) -> Option<Position> {
    Some(Position { iteration, index })
}

fn report(next_request: Option<NextRequest>) -> ScriptReport {
    ScriptReport {
        phase: ScriptPhase::PreRequest,
        collection: false,
        message: None,
        tests: Vec::new(),
        logs: vec![ScriptLog {
            level: "log".into(),
            message: "hello".into(),
        }],
        error: None,
        next_request,
    }
}

#[test]
fn requests_run_in_order_for_each_iteration() {
    let requests = requests(&["Login", "List"]);
    let mut cursor = Cursor::new(requests.len(), 2);
    let mut visited = Vec::new();

    while let Some(position) = cursor.next() {
        visited.push(position);
        assert_eq!(cursor.advance(None, &requests), None);
    }

    assert_eq!(
        visited.into_iter().map(Some).collect::<Vec<_>>(),
        [at(0, 0), at(0, 1), at(1, 0), at(1, 1)]
    );
    assert_eq!(Cursor::new(0, 3).next(), None);
    assert_eq!(Cursor::new(2, 0).next(), None);
}

#[test]
fn scripts_choose_the_next_request_by_id_or_name() {
    let requests = requests(&["Login", "List", "Logout", "List"]);
    let mut cursor = Cursor::new(requests.len(), 2);

    cursor.advance(Some(&NextRequest::Request("Logout-id".into())), &requests);
    assert_eq!(cursor.next(), at(0, 2));

    // A name finds the first request with it, which can loop back.
    cursor.advance(Some(&NextRequest::Request("List".into())), &requests);
    assert_eq!(cursor.next(), at(0, 1));

    // Null ends the iteration, and the next one starts from the top.
    cursor.advance(Some(&NextRequest::Stop), &requests);
    assert_eq!(cursor.next(), at(1, 0));

    let warning = cursor.advance(Some(&NextRequest::Request("Missing".into())), &requests);
    assert!(warning.unwrap().contains("iteration 2 ended"));
    assert_eq!(cursor.next(), None);
}

#[test]
fn finishing_ends_the_run() {
    let requests = requests(&["Login"]);
    let mut cursor = Cursor::new(requests.len(), 5);

    cursor.finish();

    assert_eq!(cursor.next(), None);
    assert_eq!(cursor.advance(None, &requests), None);
}

#[test]
fn the_last_script_choice_wins() {
    let scripts = [
        report(Some(NextRequest::Request("A".into()))),
        report(None),
        report(Some(NextRequest::Stop)),
        report(None),
    ];

    assert_eq!(chosen_request(&scripts), Some(&NextRequest::Stop));
    assert_eq!(chosen_request(&[report(None)]), None);
}

#[test]
fn skipped_and_failed_requests_keep_their_scripts() {
    let position = Position {
        iteration: 0,
        index: 0,
    };
    let mut kept = usize::MAX;
    let skipped = RunResult::new(
        position,
        request::Method::Get,
        Err(ExecutionFailure {
            error: ExecutionError::Skipped {
                reason: "No token".into(),
            },
            scripts: vec![report(None), report(None)],
        }),
        Some(&mut kept),
        false,
    );

    assert!(matches!(&skipped.outcome, Outcome::Skipped(reason) if reason == "No token"));
    assert_eq!(skipped.scripts.len(), 2);
    // Logs are left out while they are turned off.
    assert!(skipped.scripts.iter().all(|report| report.logs.is_empty()));
    assert!(!skipped.is_error());

    let failed = RunResult::new(
        position,
        request::Method::Get,
        Err(ExecutionError::Variables("Unknown variable {{host}}".into()).into()),
        Some(&mut kept),
        true,
    );

    assert!(matches!(&failed.outcome, Outcome::Failed(message) if message.contains("{{host}}")));
    assert!(failed.scripts.is_empty());
    assert!(failed.is_error());
}

fn response(body: &[u8], method: request::Method) -> request::Execution {
    request::Execution {
        response: request::Response::Http(request::HttpResponse {
            status: request::StatusCode::OK,
            version: request::Version::HTTP_11,
            headers: Default::default(),
            body: body.to_vec(),
            metrics: Default::default(),
        }),
        elapsed: std::time::Duration::from_millis(10),
        scripts: vec![report(None)],
        sent: Some(request::HttpRequest {
            method,
            path: "https://example.com/sent".into(),
            ..Default::default()
        }),
    }
}

#[test]
fn responses_are_kept_within_the_run_limit_with_the_method_sent() {
    let position = Position {
        iteration: 0,
        index: 0,
    };
    let mut remaining = 5;

    let kept = RunResult::new(
        position,
        request::Method::Get,
        Ok(response(b"1234", request::Method::Post)),
        Some(&mut remaining),
        true,
    );
    let over = RunResult::new(
        position,
        request::Method::Get,
        Ok(response(b"56", request::Method::Get)),
        Some(&mut remaining),
        true,
    );
    let off = RunResult::new(
        position,
        request::Method::Get,
        Ok(response(b"7", request::Method::Get)),
        None,
        true,
    );

    // The pre-request script's method, not the saved one.
    assert_eq!(kept.method, request::Method::Post);
    assert_eq!(kept.url.as_deref(), Some("https://example.com/sent"));
    assert_eq!(kept.scripts.len(), 1);
    assert!(matches!(
        &kept.outcome,
        Outcome::Response { response: Kept::Response(execution), .. } if execution.scripts.is_empty()
    ));
    assert_eq!(remaining, 1);
    assert!(matches!(
        over.outcome,
        Outcome::Response {
            response: Kept::OverLimit,
            ..
        }
    ));
    assert!(matches!(
        off.outcome,
        Outcome::Response {
            response: Kept::Off,
            ..
        }
    ));
}

#[test]
fn totals_count_tests_skips_errors_and_response_times() {
    let position = Position {
        iteration: 0,
        index: 0,
    };
    let mut passing = report(None);
    passing.tests = vec![
        request::ScriptTest {
            name: "ok".into(),
            error: None,
        },
        request::ScriptTest {
            name: "bad".into(),
            error: Some("expected".into()),
        },
    ];
    let mut execution = response(b"", request::Method::Get);
    execution.scripts = vec![passing];
    let mut totals = Totals::default();

    totals.add(&RunResult::new(
        position,
        request::Method::Get,
        Ok(execution),
        None,
        true,
    ));
    totals.add(&RunResult::new(
        position,
        request::Method::Get,
        Err(ExecutionFailure {
            error: ExecutionError::Skipped {
                reason: "No token".into(),
            },
            scripts: vec![report(None)],
        }),
        None,
        true,
    ));
    totals.add(&RunResult::new(
        position,
        request::Method::Get,
        Err(ExecutionError::Variables("Unknown variable".into()).into()),
        None,
        true,
    ));

    assert_eq!((totals.passed, totals.failed, totals.tests()), (1, 1, 2));
    assert_eq!((totals.passing_requests, totals.failing_requests), (1, 1));
    assert_eq!((totals.skipped, totals.errors), (1, 1));
    assert_eq!(totals.average(), Some(std::time::Duration::from_millis(10)));
    assert_eq!(Totals::default().average(), None);
}

#[test]
fn durations_read_the_same_everywhere() {
    use std::time::Duration;

    assert_eq!(duration_label(Duration::from_millis(28)), "28 ms");
    assert_eq!(duration_label(Duration::from_millis(1073)), "1.07 s");
    assert_eq!(duration_label(Duration::from_millis(8851)), "8.85 s");
    assert_eq!(duration_label(Duration::from_secs(125)), "2 min 05 s");
}

#[test]
fn common_transport_failures_are_summarized() {
    assert_eq!(
        failure_summary(
            "HTTP transport failed: error sending request: client error (Connect): tcp connect error: Connection refused (os error 111)"
        ),
        "Connection refused"
    );
    assert_eq!(
        failure_summary(
            "HTTP transport failed: error sending request: client error (Connect): dns error: failed to lookup address information: Name or service not known"
        ),
        "Host not found"
    );
    assert_eq!(
        failure_summary("Unknown variable {{baseUrl}}."),
        "Unknown variable {{baseUrl}}."
    );
    assert_eq!(
        failure_summary("request timed out after 30s"),
        "request timed out after 30s"
    );
}

#[test]
fn large_counts_keep_their_thousands_apart() {
    assert_eq!(count_label(7), "7");
    assert_eq!(count_label(1000), "1,000");
    assert_eq!(count_label(1_000_000), "1,000,000");
}
