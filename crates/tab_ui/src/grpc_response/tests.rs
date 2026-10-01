use std::time::{Duration, SystemTime};

use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
use request::{
    GrpcError, GrpcEvent, GrpcMessage, GrpcStatus, MethodKind, ScriptLog, ScriptPhase,
    ScriptReport, ScriptTest,
};

use super::GrpcResponse;
use super::view::{CallState, Filter};
use crate::request_draft::tests::element_bounds;

fn response(cx: &mut TestAppContext) -> (Entity<GrpcResponse>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });

    cx.add_window_view(|_, cx| GrpcResponse::new(cx))
}

fn message(json: &str) -> GrpcMessage {
    GrpcMessage {
        json: json.to_owned(),
        at: SystemTime::now(),
    }
}

fn stream_events() -> Vec<GrpcEvent> {
    vec![
        GrpcEvent::Metadata(vec![("content-type".into(), "application/grpc".into())]),
        GrpcEvent::Sent(message("{\n  \"text\": \"one\"\n}")),
        GrpcEvent::Received(message("{\n  \"text\": \"echo one\"\n}")),
        GrpcEvent::Finished {
            status: GrpcStatus::ok(),
            trailers: vec![("x-count".into(), "1".into())],
            elapsed: Duration::from_millis(12),
        },
    ]
}

#[gpui_kit::test]
fn streams_list_messages_newest_first_with_filters(cx: &mut TestAppContext) {
    let (response, cx) = response(cx);

    response.update_in(cx, |response, window, cx| {
        response.start(
            MethodKind::BidiStreaming,
            "localhost:50051".into(),
            window,
            cx,
        );
        response.receive(stream_events(), window, cx);
    });

    response.read_with(cx, |response, _| {
        // Sent request, received response, sent, received and completed.
        let summaries = response
            .visible()
            .into_iter()
            .map(|index| response.entries[index].summary.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            summaries,
            [
                "Call completed",
                r#"{"text":"echo one"}"#,
                r#"{"text":"one"}"#,
                "Received response from localhost:50051",
                "Sent request to localhost:50051",
            ]
        );
        assert!(matches!(response.state, CallState::Finished { .. }));
        assert_eq!(response.metadata.len(), 1);
        assert_eq!(response.trailers.len(), 1);
    });

    assert!(element_bounds(cx, "grpc-stream-row-4").is_some());
    assert!(element_bounds(cx, "grpc-status-code").is_some());

    response.update(cx, |response, cx| {
        response.filter = Filter::Received;
        response.refresh_list();
        cx.notify();
    });
    response.read_with(cx, |response, _| assert_eq!(response.visible().len(), 1));

    // Expanding a message shows its indented JSON.
    response.update(cx, |response, cx| {
        response.filter = Filter::All;
        response.refresh_list();
        cx.notify();
    });
    let row = element_bounds(cx, "grpc-stream-summary-1").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    assert!(element_bounds(cx, "grpc-stream-detail-1").is_some());

    // Clearing hides the stream until it is restored.
    let clear = element_bounds(cx, "grpc-clear-messages").unwrap();
    cx.simulate_click(clear.center(), Modifiers::default());
    response.read_with(cx, |response, _| assert!(response.visible().is_empty()));
    let restore = element_bounds(cx, "grpc-restore-messages").unwrap();
    cx.simulate_click(restore.center(), Modifiers::default());
    response.read_with(cx, |response, _| assert_eq!(response.visible().len(), 5));
}

#[gpui_kit::test]
fn unary_calls_show_the_response_message_and_status(cx: &mut TestAppContext) {
    let (response, cx) = response(cx);

    response.update_in(cx, |response, window, cx| {
        response.start(MethodKind::Unary, "localhost:50051".into(), window, cx);
        response.receive(stream_events(), window, cx);
    });

    assert!(element_bounds(cx, "grpc-response-body").is_some());
    assert!(element_bounds(cx, "grpc-stream").is_none());
    assert!(element_bounds(cx, "grpc-elapsed").is_some());
}

#[gpui_kit::test]
fn error_statuses_explain_the_failure(cx: &mut TestAppContext) {
    let (response, cx) = response(cx);

    response.update_in(cx, |response, window, cx| {
        response.start(MethodKind::Unary, "localhost:50051".into(), window, cx);
        response.receive(
            vec![GrpcEvent::Finished {
                status: GrpcStatus {
                    code: 14,
                    message: String::new(),
                },
                trailers: Vec::new(),
                elapsed: Duration::from_millis(3),
            }],
            window,
            cx,
        );
    });

    assert!(element_bounds(cx, "grpc-error").is_some());
    response.read_with(cx, |response, _| {
        let CallState::Finished { status, .. } = &response.state else {
            panic!("expected a status");
        };
        assert_eq!(status.name(), "UNAVAILABLE");
    });
}

fn report(phase: ScriptPhase, message: Option<usize>, failure: Option<&str>) -> ScriptReport {
    ScriptReport {
        phase,
        collection: false,
        message,
        tests: vec![ScriptTest {
            name: "has text".into(),
            error: failure.map(str::to_owned),
        }],
        logs: vec![ScriptLog {
            level: "log".into(),
            message: "checked".into(),
        }],
        error: None,
    }
}

#[gpui_kit::test]
fn failed_script_tests_show_once_the_call_ends(cx: &mut TestAppContext) {
    let (response, cx) = response(cx);

    response.update_in(cx, |response, window, cx| {
        response.start(MethodKind::Unary, "localhost:50051".into(), window, cx);
        let mut events = stream_events();
        events.insert(
            3,
            GrpcEvent::Script(report(ScriptPhase::OnMessage, Some(1), None)),
        );
        events.insert(
            4,
            GrpcEvent::Script(report(
                ScriptPhase::AfterResponse,
                None,
                Some("expected 1 to equal 2"),
            )),
        );
        response.receive(events, window, cx);
    });

    assert!(element_bounds(cx, "script-test-results").is_some());
    response.read_with(cx, |response, _| {
        assert_eq!(response.scripts.len(), 2);
        assert_eq!(response.scripts[0].label(), "On message 1");
    });

    let console = element_bounds(cx, "grpc-response-section-Console").unwrap();
    cx.simulate_click(console.center(), Modifiers::default());
    assert!(element_bounds(cx, "script-console").is_some());
}

#[gpui_kit::test]
fn calls_stopped_by_before_invoke_show_why(cx: &mut TestAppContext) {
    let (response, cx) = response(cx);

    response.update_in(cx, |response, _, cx| {
        response.fail_invoke(
            GrpcError::Skipped {
                reason: "No token yet".into(),
                report: Box::new(report(ScriptPhase::BeforeInvoke, None, None)),
            },
            cx,
        );
    });
    assert!(element_bounds(cx, "grpc-skipped").is_some());
    assert!(element_bounds(cx, "grpc-response-section-Tests").is_some());

    response.update_in(cx, |response, _, cx| {
        let mut failed = report(ScriptPhase::BeforeInvoke, None, None);
        failed.error = Some("Error: no token".into());
        response.fail_invoke(
            GrpcError::Script {
                message: "Error: no token".into(),
                report: Box::new(failed),
            },
            cx,
        );
    });
    assert!(element_bounds(cx, "script-test-results").is_some());
    response.read_with(cx, |response, _| {
        assert!(matches!(response.state, CallState::Failed(_)));
    });
}
