use std::time::{Duration, Instant};

use gpui_kit::component::input::{EditorState, RopeExt};
use gpui_kit::{Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use lsp_types::{CompletionItem, CompletionTextEdit};
use request::ScriptPhase;
use ropey::Rope;

use super::{
    script_completions::completion_items,
    tests::{draft, element_bounds},
};

fn complete(source: &str, phase: ScriptPhase) -> Vec<CompletionItem> {
    let offset = source.find('|').unwrap();
    let text = Rope::from(source.replacen('|', "", 1));
    completion_items(&text, offset, phase)
}

fn labels(source: &str, phase: ScriptPhase) -> Vec<String> {
    complete(source, phase)
        .into_iter()
        .map(|item| item.label)
        .collect()
}

#[test]
fn suggests_supported_members_for_the_current_phase() {
    assert!(labels("pm.|", ScriptPhase::PostResponse).contains(&"response".into()));
    assert!(!labels("pm.|", ScriptPhase::PreRequest).contains(&"response".into()));
    assert!(labels("pm.response.|", ScriptPhase::PreRequest).is_empty());
    assert!(labels("pm.collectionVariables.|", ScriptPhase::PreRequest).is_empty());
    assert!(labels("pm.|", ScriptPhase::PreRequest).contains(&"execution".into()));
    assert!(!labels("pm.|", ScriptPhase::PostResponse).contains(&"execution".into()));
    assert!(labels("pm.execution.|", ScriptPhase::PostResponse).is_empty());
    assert_eq!(
        labels("pm.response.j|", ScriptPhase::PostResponse),
        ["json"]
    );
    assert_eq!(labels("pm.variables.s|", ScriptPhase::PreRequest), ["set"]);
    assert_eq!(
        labels("pm.request.headers.u|", ScriptPhase::PreRequest),
        ["upsert"]
    );
    assert_eq!(
        labels("pm.request.url.q|", ScriptPhase::PreRequest),
        ["query"]
    );
    assert_eq!(
        labels("pm.request.url.query.u|", ScriptPhase::PreRequest),
        ["upsert"]
    );
    assert_eq!(
        labels("pm.environment.s|", ScriptPhase::PreRequest),
        ["set"]
    );
    assert_eq!(
        labels("await pm.send|", ScriptPhase::PreRequest),
        ["sendRequest"]
    );
    assert_eq!(
        labels("pm.crypto.sha|", ScriptPhase::PreRequest),
        ["sha256"]
    );
    assert_eq!(
        labels("pm.encoding.base64UrlE|", ScriptPhase::PreRequest),
        ["base64UrlEncode"]
    );
    assert_eq!(
        labels("pm.schema.v|", ScriptPhase::PreRequest),
        ["validate"]
    );
    assert_eq!(
        labels("pm.execution.s|", ScriptPhase::PreRequest),
        ["skipRequest"]
    );
    assert_eq!(
        labels("pm.response.to.have.jsonS|", ScriptPhase::PostResponse),
        ["jsonSchema"]
    );
}

#[test]
fn completes_typed_object_arguments_and_inferred_values() {
    for (source, expected) in [
        ("pm.sendRequest({u|})", "url"),
        ("pm.sendRequest({url: 'https://example.com', b|})", "body"),
        ("pm.sendRequest({body: {r|}})", "raw"),
        ("pm.request.headers.upsert({k|})", "key"),
        ("pm.request.url.query.add({v|})", "value"),
        ("pm.schema.validate({}, {properties: {id: {ty|}}})", "type"),
        (
            "const response = await pm.sendRequest('https://example.com'); response.j|",
            "json",
        ),
        (
            "pm.sendRequest('https://example.com', (error, response) => { response.he| })",
            "headers",
        ),
        (
            "const token = {value: 'secret', expiresAt: 10}; token.ex|",
            "expiresAt",
        ),
        ("const send = pm.sendRequest; send({u|})", "url"),
    ] {
        assert!(
            labels(source, ScriptPhase::PreRequest).contains(&expected.into()),
            "{source}"
        );
    }
    let url = complete("pm.sendRequest({u|})", ScriptPhase::PreRequest).remove(0);
    assert_eq!(url.label, "url");
    assert!(url.detail.unwrap().contains("string"));
    let methods = labels("pm.sendRequest({method: \"P|\"})", ScriptPhase::PreRequest);
    assert!(methods.contains(&"POST".into()));
    assert!(methods.contains(&"PUT".into()));
    assert!(methods.contains(&"PATCH".into()));
}

#[test]
fn distinguishes_code_from_comments_strings_and_regex_literals() {
    for source in [
        "// pm.response.|",
        "/* pm.response.| */",
        "'pm.response.|'",
        "\"pm.response.|\"",
        "`pm.response.|`",
        "const pattern = /pm.response.|/;",
        "const p| = 1;",
        "function p|() {}",
        "function f(p|) {}",
        "p| => 42",
    ] {
        assert!(
            labels(source, ScriptPhase::PostResponse).is_empty(),
            "{source}"
        );
    }
    assert_eq!(
        labels("`value: ${pm.response.j|}`", ScriptPhase::PostResponse),
        ["json"]
    );
    assert!(labels("const result = p|", ScriptPhase::PostResponse).contains(&"pm".into()));
    assert_eq!(labels("console.l|", ScriptPhase::PreRequest), ["log"]);
    assert_eq!(labels("JSON.p|", ScriptPhase::PreRequest), ["parse"]);
}

#[test]
fn completes_assertions_with_nested_arguments_and_multiline_chains() {
    for source in [
        "pm.expect(pm.response.json()).to.be.b|",
        "pm.expect({value: call(1, ')')}).not.to.be.b|",
        "pm.expect([1]).to.have.property('length').and.b|",
        "pm.expect(true).to.be.true.and.b|",
        "pm.expect(true)\n    .to\n    .be\n    .b|",
        "pm.expect([1, 2]).to.have.lengthOf.b|",
        "pm.expect('hello').to.be.a('string').and.b|",
    ] {
        assert!(
            labels(source, ScriptPhase::PostResponse).contains(&"below".into()),
            "{source}"
        );
    }
    assert_eq!(
        labels("(pm.response).j|", ScriptPhase::PostResponse),
        ["json"]
    );
    assert_eq!(
        labels("pm.response?.j|", ScriptPhase::PostResponse),
        ["json"]
    );
    assert_eq!(
        labels("pm.expect(201).to.be.one|", ScriptPhase::PostResponse),
        ["oneOf"]
    );
    assert_eq!(
        labels("pm.expect({}).to.include.k|", ScriptPhase::PostResponse),
        ["keys"]
    );
}

#[test]
fn edits_replace_the_entire_member_and_preserve_unicode_and_surrounding_code() {
    for source in [
        "const emoji = '🦅';\nconsole.log('🦅', pm.response.j|son());",
        "const text = 'a\u{2028}🦅b'; pm.response.j|son();",
        "const text = 'a\u{2029}🦅b'; pm.response.j|son();",
        "const text = '🦅';\rpm.response.j|son();",
        "const text = 'a\u{2028}🦅b';\r\npm.response.j|son();",
    ] {
        let offset = source.find('|').unwrap();
        let text = Rope::from(source.replacen('|', "", 1));
        let items = completion_items(&text, offset, ScriptPhase::PostResponse);
        let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap() else {
            panic!()
        };
        let start = text.position_to_offset(&edit.range.start);
        let end = text.position_to_offset(&edit.range.end);
        let mut actual = text.to_string();
        actual.replace_range(start..end, &edit.new_text);
        assert_eq!(actual, source.replace('|', ""));
        assert_eq!(edit.new_text, "json");
        assert!(items[0].detail.as_deref().unwrap().contains("json()"));
    }
}

fn script_editor(
    cx: &mut TestAppContext,
    post: bool,
) -> (
    Entity<super::RequestDraft>,
    Entity<EditorState>,
    &mut VisualTestContext,
) {
    cx.executor().allow_parking();
    let (draft, cx) = draft(cx);
    let scripts = element_bounds(cx, "request-section-Scripts").unwrap();
    cx.simulate_click(scripts.center(), Modifiers::default());
    if post {
        let phase = element_bounds(cx, "script-phase-Post-response").unwrap();
        cx.simulate_click(phase.center(), Modifiers::default());
    }
    let editor = cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            let editor = draft.script_state(window, cx);
            window.focus(&editor.read(cx).focus_handle(cx), cx);
            editor
        })
    });
    (draft, editor, cx)
}

async fn wait_for(
    cx: &mut VisualTestContext,
    mut ready: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let started = Instant::now();
    loop {
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "editor assistance did not update"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
    }
}

#[gpui_kit::test]
async fn keyboard_completion_replaces_the_prefix_and_marks_the_script_dirty(
    cx: &mut TestAppContext,
) {
    let (draft, editor, cx) = script_editor(cx, true);
    cx.simulate_input("pm.response.j");
    wait_for(cx, |cx| {
        cx.read(|cx| {
            let menu = editor.read(cx).completion_menu_state();
            menu.open && menu.items.len() == 1 && menu.items[0].label == "json"
        })
    })
    .await;
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(editor.read(cx).value(), "pm.response.json");
        assert_eq!(
            draft.read(cx).request.scripts.post_response,
            "pm.response.json"
        );
        assert!(draft.read(cx).is_dirty());
        assert!(!editor.read(cx).completion_menu_state().open);
    });
}

#[gpui_kit::test]
async fn object_field_completion_replaces_the_prefix_inside_braces(cx: &mut TestAppContext) {
    let (draft, editor, cx) = script_editor(cx, false);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.replace_all("pm.sendRequest({})", window, cx);
            let offset = editor.value().find('}').unwrap();
            editor.set_selected_range(offset..offset, cx);
        })
    });
    cx.simulate_input("u");
    wait_for(cx, |cx| {
        cx.read(|cx| {
            let menu = editor.read(cx).completion_menu_state();
            menu.open && menu.items.len() == 1 && menu.items[0].label == "url"
        })
    })
    .await;
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(editor.read(cx).value(), "pm.sendRequest({url})");
        assert_eq!(
            draft.read(cx).request.scripts.pre_request,
            "pm.sendRequest({url})"
        );
    });
}

#[gpui_kit::test]
async fn completion_navigation_and_escape_use_the_native_menu(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, false);
    cx.simulate_input("pm.variables.");
    wait_for(cx, |cx| {
        cx.read(|cx| editor.read(cx).completion_menu_state().open)
    })
    .await;
    let expected = cx.read(|cx| {
        editor.read(cx).completion_menu_state().items[1]
            .label
            .clone()
    });
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(editor.read(cx).value(), format!("pm.variables.{expected}")));
    cx.simulate_input(";pm.");
    wait_for(cx, |cx| {
        cx.read(|cx| editor.read(cx).completion_menu_state().open)
    })
    .await;
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.read(|cx| assert!(!editor.read(cx).completion_menu_state().open));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            editor.read(cx).value(),
            format!("pm.variables.{expected};pm.\n")
        )
    });
}

#[gpui_kit::test]
async fn typing_punctuation_hides_stale_completions(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, true);
    cx.simulate_input("console.");
    wait_for(cx, |cx| {
        cx.read(|cx| editor.read(cx).completion_menu_state().open)
    })
    .await;
    cx.simulate_input("log('");
    wait_for(cx, |cx| {
        cx.read(|cx| !editor.read(cx).completion_menu_state().open)
    })
    .await;
}

#[gpui_kit::test]
async fn rapid_typing_cannot_accept_an_edit_for_older_text(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, true);
    for suffix in ["s", ";"] {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.replace_all("", window, cx));
        });
        cx.simulate_input("pm.response.j");
        wait_for(cx, |cx| {
            cx.read(|cx| editor.read(cx).completion_menu_state().open)
        })
        .await;
        cx.simulate_input(suffix);
        cx.read(|cx| assert!(!editor.read(cx).completion_menu_state().open));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(editor.read(cx).value(), format!("pm.response.j{suffix}\n"));
        });
    }
}

#[gpui_kit::test]
async fn function_parameter_help_tracks_the_caret_and_can_be_dismissed(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, false);
    cx.simulate_input("pm.crypto.hmacSha256(");
    wait_for(cx, |cx| {
        element_bounds(cx, "script-signature-help").is_some()
    })
    .await;
    cx.simulate_input("'secret', ");
    wait_for(cx, |cx| {
        element_bounds(cx, "script-signature-help").is_some()
    })
    .await;
    // Escape dismisses a completion list first, then the parameter popup.
    cx.simulate_keystrokes("escape escape");
    cx.run_until_parked();
    assert!(element_bounds(cx, "script-signature-help").is_none());
    cx.simulate_input("'body')");
    wait_for(cx, |cx| {
        element_bounds(cx, "script-signature-help").is_none()
    })
    .await;
    cx.read(|cx| {
        assert_eq!(
            editor.read(cx).value(),
            "pm.crypto.hmacSha256('secret', 'body')"
        )
    });
}
