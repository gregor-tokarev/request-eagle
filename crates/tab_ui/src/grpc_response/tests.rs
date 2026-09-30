use std::time::{Duration, SystemTime};

use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
use request::{GrpcEvent, GrpcMessage, GrpcStatus, MethodKind};

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
