use std::path::PathBuf;

use gpui_kit::{
    Entity, InputEvent as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};
use request::{GrpcDefinition, GrpcRequest, MethodKind};

use super::GrpcDraft;
use super::definition::DefinitionState;
use super::draft::GrpcSection;
use crate::request_draft::tests::element_bounds;

const ECHO_PROTO: &str = r#"
syntax = "proto3";
package echo.v1;
import "common/types.proto";

service EchoService {
  rpc Say(EchoRequest) returns (EchoReply);
  rpc Chat(stream EchoRequest) returns (stream EchoReply);
}

message EchoRequest {
  string text = 1;
  common.Tag tag = 2;
}

message EchoReply {
  string text = 1;
}
"#;

const TYPES_PROTO: &str = r#"
syntax = "proto3";
package common;
message Tag { string name = 1; }
"#;

/// `protos/echo.proto`, whose import resolves from `shared`.
fn protos() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let echo = directory.path().join("protos/echo.proto");
    let shared = directory.path().join("shared");

    std::fs::create_dir_all(echo.parent().unwrap()).unwrap();
    std::fs::create_dir_all(shared.join("common")).unwrap();
    std::fs::write(&echo, ECHO_PROTO).unwrap();
    std::fs::write(shared.join("common/types.proto"), TYPES_PROTO).unwrap();

    (directory, echo, shared)
}

fn draft(
    request: GrpcRequest,
    cx: &mut TestAppContext,
) -> (Entity<GrpcDraft>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });

    let (draft, cx) = cx.add_window_view(|window, cx| {
        let mut draft = GrpcDraft::new(request, None, Default::default(), None, cx);
        draft.prepare(window, cx);
        draft
    });
    cx.run_until_parked();

    (draft, cx)
}

/// Scroll the request section to its end, where lower controls are visible.
fn scroll_section_down(cx: &mut VisualTestContext) {
    let content = element_bounds(cx, "grpc-section-content").unwrap();

    cx.update(|window, cx| {
        window.dispatch_event(
            ScrollWheelEvent {
                position: content.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        )
    });
    cx.run_until_parked();
}

fn proto_request(echo: PathBuf, shared: PathBuf, method: &str) -> GrpcRequest {
    GrpcRequest {
        url: "127.0.0.1:1".into(),
        method: format!("echo.v1.EchoService/{method}"),
        definition: GrpcDefinition::ProtoFile {
            path: echo,
            import_paths: vec![shared],
        },
        ..GrpcRequest::default()
    }
}

#[gpui_kit::test]
fn a_new_request_shows_the_address_bar_and_an_empty_response(cx: &mut TestAppContext) {
    let (draft, cx) = draft(GrpcRequest::default(), cx);

    for selector in [
        "grpc-tls",
        "grpc-url-bar",
        "grpc-method",
        "grpc-invoke",
        "grpc-message",
        "grpc-response-empty",
    ] {
        assert!(element_bounds(cx, selector).is_some(), "{selector}");
    }

    // Without a URL, reflection has nothing to load.
    draft.read_with(cx, |draft, _| {
        assert!(matches!(draft.definition, DefinitionState::Idle));
        assert!(!draft.is_dirty());
    });
}

#[gpui_kit::test]
fn a_proto_file_lists_methods_and_generates_an_example(cx: &mut TestAppContext) {
    let (_directory, echo, shared) = protos();
    let (draft, cx) = draft(proto_request(echo, shared, "Say"), cx);

    draft.read_with(cx, |draft, _| {
        let DefinitionState::Loaded(definition) = &draft.definition else {
            panic!("the .proto file did not load");
        };
        assert_eq!(definition.services()[0].methods.len(), 2);
        assert_eq!(draft.method_kind(), Some(MethodKind::Unary));
    });

    let button = element_bounds(cx, "grpc-example-message").unwrap();
    cx.simulate_click(button.center(), Modifiers::default());

    draft.read_with(cx, |draft, _| {
        let message: serde_json::Value = serde_json::from_str(&draft.request.message).unwrap();
        assert_eq!(message["text"], "text");
        assert_eq!(message["tag"]["name"], "name");
        assert!(draft.is_dirty());
    });
}

#[gpui_kit::test]
fn a_missing_import_path_is_reported_in_service_definition(cx: &mut TestAppContext) {
    let (_directory, echo, _) = protos();
    let request = GrpcRequest {
        definition: GrpcDefinition::ProtoFile {
            path: echo,
            import_paths: Vec::new(),
        },
        ..GrpcRequest::default()
    };
    let (draft, cx) = draft(request, cx);

    draft.read_with(cx, |draft, _| {
        let DefinitionState::Failed(error) = &draft.definition else {
            panic!("the import should not resolve");
        };
        assert!(error.contains("common/types.proto"), "{error}");
    });
    assert!(element_bounds(cx, "grpc-definition-error-dot").is_some());

    let tab = element_bounds(cx, "grpc-section-Service definition").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    assert!(element_bounds(cx, "grpc-definition-status").is_some());
    assert!(element_bounds(cx, "grpc-proto-path").is_some());
}

#[gpui_kit::test]
fn streaming_requests_offer_send_and_end_streaming(cx: &mut TestAppContext) {
    let (_directory, echo, shared) = protos();
    let (draft, cx) = draft(proto_request(echo, shared, "Chat"), cx);

    draft.read_with(cx, |draft, _| {
        assert_eq!(draft.method_kind(), Some(MethodKind::BidiStreaming));
    });
    assert!(element_bounds(cx, "grpc-send-message").is_some());
    assert!(element_bounds(cx, "grpc-end-streaming").is_some());
}

#[gpui_kit::test]
fn unary_requests_send_their_message_when_invoked(cx: &mut TestAppContext) {
    let (_directory, echo, shared) = protos();
    let (draft, cx) = draft(proto_request(echo, shared, "Say"), cx);

    draft.read_with(cx, |draft, _| {
        assert_eq!(draft.method_kind(), Some(MethodKind::Unary));
    });
    assert!(element_bounds(cx, "grpc-send-message").is_none());
}

#[gpui_kit::test]
fn the_lock_switches_tls_and_the_url_scheme(cx: &mut TestAppContext) {
    // A local definition keeps the test off the network.
    let (_directory, echo, shared) = protos();
    let mut request = proto_request(echo, shared, "Say");
    request.url = "grpc://localhost:50051".into();
    let (draft, cx) = draft(request, cx);

    let lock = element_bounds(cx, "grpc-tls").unwrap();
    cx.simulate_click(lock.center(), Modifiers::default());

    draft.read_with(cx, |draft, cx| {
        assert_eq!(draft.request.url, "grpcs://localhost:50051");
        assert!(draft.request.uses_tls());
        assert_eq!(
            draft.url.as_ref().unwrap().read(cx).value(),
            "grpcs://localhost:50051"
        );
    });
}

#[gpui_kit::test]
fn invoking_without_a_method_explains_what_is_missing(cx: &mut TestAppContext) {
    // A local definition keeps the test off the network.
    let (_directory, echo, shared) = protos();
    let mut request = proto_request(echo, shared, "Say");
    request.method.clear();
    let (draft, cx) = draft(request, cx);

    let invoke = element_bounds(cx, "grpc-invoke").unwrap();
    cx.simulate_click(invoke.center(), Modifiers::default());

    assert!(element_bounds(cx, "grpc-error").is_some());
    draft.read_with(cx, |draft, _| assert!(draft.call.is_none()));
}

#[gpui_kit::test]
fn import_paths_are_edited_with_the_request(cx: &mut TestAppContext) {
    let (draft, cx) = draft(GrpcRequest::default(), cx);

    let tab = element_bounds(cx, "grpc-section-Service definition").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    let import = element_bounds(cx, "grpc-import-proto").unwrap();
    cx.simulate_click(import.center(), Modifiers::default());

    draft.read_with(cx, |draft, _| {
        assert_eq!(draft.section, GrpcSection::Definition);
        assert!(!draft.request.definition.is_reflection());
    });

    scroll_section_down(cx);
    let add = element_bounds(cx, "grpc-add-import-path").unwrap();
    cx.simulate_click(add.center(), Modifiers::default());
    cx.simulate_input("shared");

    draft.read_with(cx, |draft, _| {
        let GrpcDefinition::ProtoFile { import_paths, .. } = &draft.request.definition else {
            panic!("expected a .proto definition");
        };
        assert_eq!(import_paths, &[PathBuf::from("shared")]);
    });

    scroll_section_down(cx);
    let remove = element_bounds(cx, "grpc-remove-import-path-0").unwrap();
    cx.simulate_click(remove.center(), Modifiers::default());

    draft.read_with(cx, |draft, _| {
        let GrpcDefinition::ProtoFile { import_paths, .. } = &draft.request.definition else {
            panic!("expected a .proto definition");
        };
        assert!(import_paths.is_empty());
    });
}

#[gpui_kit::test]
fn settings_are_saved_with_the_request(cx: &mut TestAppContext) {
    let (draft, cx) = draft(GrpcRequest::default(), cx);

    let tab = element_bounds(cx, "grpc-section-Settings").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());

    let defaults = element_bounds(cx, "grpc-include-default-fields").unwrap();
    cx.simulate_click(defaults.center(), Modifiers::default());
    let verify = element_bounds(cx, "grpc-verify-certificates").unwrap();
    cx.simulate_click(verify.center(), Modifiers::default());
    scroll_section_down(cx);
    let limit = element_bounds(cx, "grpc-max-message").unwrap();
    cx.simulate_click(limit.center(), Modifiers::default());
    cx.simulate_input("8");

    draft.read_with(cx, |draft, _| {
        let settings = &draft.request.settings;
        assert!(!settings.include_default_fields);
        // Verification follows the preference, on by default, until changed.
        assert_eq!(settings.verify_certificates, Some(false));
        assert_eq!(settings.max_response_message_mb, Some(8));
        assert!(draft.is_dirty());
    });
}

#[gpui_kit::test]
fn clearing_the_source_drops_a_waiting_invoke(cx: &mut TestAppContext) {
    let (draft, cx) = draft(GrpcRequest::default(), cx);

    // An Invoke waiting for reflection, whose URL is then cleared.
    draft.update_in(cx, |draft, window, cx| {
        draft.invoke_when_loaded = true;
        draft
            .response
            .update(cx, |response, cx| response.wait("Loading".into(), cx));
        draft.load_definition(false, window, cx);
    });

    draft.read_with(cx, |draft, _| assert!(!draft.invoke_when_loaded));
    let invoke = element_bounds(cx, "grpc-invoke").unwrap();
    cx.simulate_click(invoke.center(), Modifiers::default());
    // Invoke runs again and explains what is missing.
    assert!(element_bounds(cx, "grpc-error").is_some());
}

#[gpui_kit::test]
fn scripts_are_edited_for_each_hook(cx: &mut TestAppContext) {
    // Script assistance uses the shared TypeScript worker outside GPUI's test executor.
    cx.executor().allow_parking();
    let (draft, cx) = draft(GrpcRequest::default(), cx);

    let tab = element_bounds(cx, "grpc-section-Scripts").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    assert!(element_bounds(cx, "request-scripts").is_some());

    for (phase, selector) in [
        ("Before invoke", "script-phase-Before invoke"),
        ("On message", "script-phase-On message"),
        ("After response", "script-phase-After response"),
    ] {
        let phase_tab = element_bounds(cx, selector).unwrap();
        cx.simulate_click(phase_tab.center(), Modifiers::default());
        let editor = element_bounds(cx, "script-editor").unwrap();
        cx.simulate_click(editor.center(), Modifiers::default());
        cx.simulate_input(&format!("// {phase}"));
    }

    draft.read_with(cx, |draft, _| {
        let scripts = &draft.request.scripts;
        assert_eq!(scripts.before_invoke, "// Before invoke");
        assert_eq!(scripts.on_message, "// On message");
        assert_eq!(scripts.after_response, "// After response");
        assert_eq!(draft.script_count(), 3);
        assert!(draft.is_dirty());
    });
}

#[gpui_kit::test]
fn a_before_invoke_script_can_skip_the_call(cx: &mut TestAppContext) {
    // The script runs on a thread outside GPUI's test executor.
    cx.executor().allow_parking();
    let (_directory, echo, shared) = protos();
    let mut request = proto_request(echo, shared, "Say");
    request.scripts.before_invoke = "pm.execution.skipRequest('No token yet');".into();
    let (draft, cx) = draft(request, cx);

    let invoke = element_bounds(cx, "grpc-invoke").unwrap();
    cx.simulate_click(invoke.center(), Modifiers::default());

    for _ in 0..500 {
        cx.run_until_parked();
        if draft.read_with(cx, |draft, _| draft.call_task.is_none()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    draft.read_with(cx, |draft, _| {
        assert!(draft.call.is_none());
        assert!(draft.call_task.is_none());
    });
    assert!(element_bounds(cx, "grpc-skipped").is_some());
}

#[gpui_kit::test]
fn before_invoke_results_stay_when_the_server_is_unreachable(cx: &mut TestAppContext) {
    // The script and the connection run outside GPUI's test executor.
    cx.executor().allow_parking();
    let (_directory, echo, shared) = protos();
    let mut request = proto_request(echo, shared, "Say");
    request.message = "{}".into();
    request.scripts.before_invoke = "console.log('prepared');".into();
    let (draft, cx) = draft(request, cx);

    let invoke = element_bounds(cx, "grpc-invoke").unwrap();
    cx.simulate_click(invoke.center(), Modifiers::default());

    for _ in 0..500 {
        cx.run_until_parked();
        if draft.read_with(cx, |draft, _| draft.call_task.is_none()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    draft.read_with(cx, |draft, cx| {
        assert!(draft.call.is_none());
        let scripts = &draft.response.read(cx).scripts;
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].logs[0].message, "prepared");
    });
    assert!(element_bounds(cx, "grpc-response-section-Console").is_some());
}
