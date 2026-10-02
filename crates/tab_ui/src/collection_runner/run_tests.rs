use request::{ExecutionError, NextRequest, ScriptLog, ScriptPhase, ScriptReport};

use super::run::{Cursor, Outcome, Position, RunRequest, RunResult, chosen_request};

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
    let skipped = RunResult::new(
        position,
        Err(ExecutionError::ScriptedRequest {
            source: Box::new(ExecutionError::Skipped {
                reason: "No token".into(),
                report: Box::new(report(None)),
            }),
            reports: vec![report(None), report(None)],
        }),
        true,
        false,
    );

    assert!(matches!(&skipped.outcome, Outcome::Skipped(reason) if reason == "No token"));
    assert_eq!(skipped.scripts.len(), 2);
    // Logs are left out while they are turned off.
    assert!(skipped.scripts.iter().all(|report| report.logs.is_empty()));
    assert!(!skipped.is_error());

    let failed = RunResult::new(
        position,
        Err(ExecutionError::Variables(
            "Unknown variable {{host}}".into(),
        )),
        true,
        true,
    );

    assert!(matches!(&failed.outcome, Outcome::Failed(message) if message.contains("{{host}}")));
    assert!(failed.scripts.is_empty());
    assert!(failed.is_error());
}
