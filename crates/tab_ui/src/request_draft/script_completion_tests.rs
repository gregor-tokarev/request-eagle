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
fn suggests_only_supported_members_for_the_current_phase() {
    assert!(labels("pm.|", ScriptPhase::PostResponse).contains(&"response".into()));
    assert!(!labels("pm.|", ScriptPhase::PreRequest).contains(&"response".into()));
    assert!(labels("pm.response.|", ScriptPhase::PreRequest).is_empty());
    assert!(labels("pm.response.headers.|", ScriptPhase::PreRequest).is_empty());
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
        labels("pm.request.url.toS|", ScriptPhase::PostResponse),
        ["toString"]
    );
    assert_eq!(
        labels("pm.response.to.have.|", ScriptPhase::PostResponse),
        ["status", "header", "body", "jsonBody"]
    );
    assert!(labels("pm.environment.|", ScriptPhase::PreRequest).is_empty());
    assert!(labels("other.pm.response.|", ScriptPhase::PostResponse).is_empty());
    assert!(labels("pm.response.json().|", ScriptPhase::PostResponse).is_empty());
    assert!(labels("assertion.|", ScriptPhase::PostResponse).is_empty());
    assert_eq!(
        labels("pm.response.to.be.s|", ScriptPhase::PostResponse),
        ["success", "serverError"]
    );
    assert!(labels("pm.response.to.be.|", ScriptPhase::PreRequest).is_empty());
}

#[test]
fn distinguishes_code_from_comments_strings_and_regular_expressions() {
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
        "({p|: true})",
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
    assert_eq!(
        labels("const result = p|", ScriptPhase::PostResponse),
        ["pm"]
    );
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
    assert_eq!(
        labels(
            "pm.expect([]).to.deep.include.m|",
            ScriptPhase::PostResponse
        ),
        ["members", "most", "match"]
    );
    assert_eq!(
        labels("pm.expect({}).to.have.nested.p|", ScriptPhase::PostResponse),
        ["property"]
    );
}

#[test]
fn edits_replace_the_entire_member_and_preserve_unicode_and_surrounding_code() {
    let source = "const emoji = '🦅';\nconsole.log('🦅', pm.response.j|son());";
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
    assert_eq!(items[0].detail.as_deref(), Some("()"));
}

fn script_editor(
    cx: &mut TestAppContext,
    post: bool,
) -> (
    Entity<super::RequestDraft>,
    Entity<EditorState>,
    &mut VisualTestContext,
) {
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

#[gpui_kit::test]
fn vim_insert_mode_keeps_completions_and_escape_returns_to_normal(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, true);
    cx.update(|_, cx| preferences::update(cx, |p| p.vim_mode = true).unwrap());
    cx.simulate_keystrokes("i");
    cx.simulate_input("pm.response.");
    cx.run_until_parked();
    cx.read(|cx| assert!(editor.read(cx).completion_menu_state().open));
    cx.simulate_input("j");
    cx.simulate_keystrokes("enter");
    cx.read(|cx| assert_eq!(editor.read(cx).value(), "pm.response.json"));
    cx.simulate_input(";pm.");
    cx.run_until_parked();
    cx.read(|cx| assert!(editor.read(cx).completion_menu_state().open));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.read(|cx| assert!(!editor.read(cx).completion_menu_state().open));
    cx.simulate_keystrokes("h");
    cx.read(|cx| assert_eq!(editor.read(cx).value(), "pm.response.json;pm."));
}

#[gpui_kit::test]
fn keyboard_completion_replaces_the_prefix_and_marks_the_script_dirty(cx: &mut TestAppContext) {
    let (draft, editor, cx) = script_editor(cx, true);
    cx.simulate_input("pm.response.");
    cx.run_until_parked();
    cx.read(|cx| assert!(editor.read(cx).completion_menu_state().open));
    cx.simulate_input("j");
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            editor.read(cx).completion_menu_state().items[0].label,
            "json"
        )
    });
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
fn completion_navigation_and_escape_use_the_native_menu(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, false);
    cx.simulate_input("pm.variables.");
    cx.run_until_parked();
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(editor.read(cx).value(), "pm.variables.set"));
    cx.simulate_input(";");
    cx.simulate_input("pm.");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(editor.read(cx).value(), "pm.variables.set;pm.");
        assert!(!editor.read(cx).completion_menu_state().open);
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(editor.read(cx).value(), "pm.variables.set;pm.\n"));
}

#[gpui_kit::test]
fn typing_punctuation_hides_stale_completions(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, true);
    cx.simulate_input("console.");
    cx.run_until_parked();
    cx.read(|cx| assert!(editor.read(cx).completion_menu_state().open));
    cx.simulate_input("log('");
    cx.run_until_parked();
    cx.read(|cx| assert!(!editor.read(cx).completion_menu_state().open));
}

#[gpui_kit::test]
fn script_completion_follows_the_caret_on_the_first_frame(cx: &mut TestAppContext) {
    use gpui_kit::{
        Background, EntityInputHandler as _,
        component::{ActiveTheme as _, Theme},
        point, px, size,
    };

    let (_, editor, cx) = script_editor(cx, true);
    cx.simulate_resize(size(px(1440.), px(900.)));
    for theme in ["Default Light", "Default Dark"] {
        for font_size in [12., 16., 24.] {
            cx.update(|_, cx| {
                assert!(request_eagle_theme::apply(theme, cx));
                Theme::global_mut(cx).font_size = px(font_size);
                Theme::sync_base(cx);
            });
            for (source, keys) in [
                ("pm.variables.", ["t", "o", "O", "b"]),
                ("pm.request", [".", "h", "e", "a"]),
            ] {
                cx.simulate_keystrokes("secondary-a");
                cx.simulate_input(source);
                cx.run_until_parked();
                cx.read(|cx| assert!(editor.read(cx).completion_menu_state().open));

                for typed in keys {
                    cx.update(|window, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.replace_text_in_range(None, typed, window, cx);
                        });
                        window.refresh();
                        // Inspect this draw before notifications can cause a catch-up frame.
                        window.draw(cx).clear(cx);

                        let editor = editor.read(cx);
                        let (caret, height) = editor.cursor_layout().unwrap();
                        let offset = point(-px(4.), editor.scroll_offset().y + height + px(4.));
                        let expected = window
                            .pixel_snap_point(caret.origin + offset)
                            .scale(window.scale_factor());
                        let background = Background::from(cx.theme().popover);
                        let popovers = window
                            .painted_quads()
                            .iter()
                            .filter(|quad| quad.background == background)
                            .map(|quad| quad.bounds.origin)
                            .collect::<Vec<_>>();
                        let at_caret = popovers.iter().any(|origin| {
                            (origin.x - expected.x).0.abs() <= 1.
                                && (origin.y - expected.y).0.abs() <= 1.
                        });
                        assert!(
                            at_caret,
                            "{theme}, {font_size}px, typed {typed}: expected {expected:?}, got {popovers:?}"
                        );
                    });
                }
            }
        }
    }
}

#[gpui_kit::test]
fn script_completion_click_uses_the_moved_popover_bounds(cx: &mut TestAppContext) {
    let (_, editor, cx) = script_editor(cx, true);
    cx.simulate_input("pm.variables.");
    cx.simulate_input("g");
    let bounds = cx.debug_bounds("completion-menu").unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(editor.read(cx).value(), "pm.variables.get");
        assert!(!editor.read(cx).completion_menu_state().open);
    });
}
