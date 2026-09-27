use gpui_kit::component::{
    input::{Editor, EditorState, Input, InputState},
    v_flex,
};
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, TestAppContext,
    VisualTestContext, Window,
};

use super::Vim;

struct Harness {
    editor: Entity<EditorState>,
    input: Entity<InputState>,
    vim: Entity<Vim>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.vim.clone())
            .child(Input::new(&self.input))
            .child(Editor::new(&self.editor).h_full())
    }
}

fn setup<'a>(
    cx: &'a mut TestAppContext,
    value: &str,
    enabled: bool,
) -> (Entity<Harness>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::update(cx, |p| p.vim_mode = enabled).unwrap();
    });
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = cx.new(|cx| EditorState::new(window, cx).default_value(value.to_owned()));
        let input = cx.new(|cx| InputState::new(window, cx));
        let vim = cx.new(|cx| Vim::new(editor.clone(), cx));
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        Harness { editor, input, vim }
    });

    (view, cx)
}

fn value(view: &Entity<Harness>, cx: &VisualTestContext) -> String {
    cx.read(|cx| view.read(cx).editor.read(cx).value().to_string())
}

fn cursor(view: &Entity<Harness>, cx: &VisualTestContext) -> usize {
    cx.read(|cx| view.read(cx).editor.read(cx).cursor())
}

#[gpui_kit::test]
fn vim_can_be_toggled_in_an_open_editor_without_changing_its_text(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "", false);
    cx.simulate_keystrokes("h j k l");
    assert_eq!(value(&view, cx), "hjkl");

    cx.update(|_, cx| preferences::update(cx, |p| p.vim_mode = true).unwrap());
    cx.simulate_keystrokes("0 l");
    assert_eq!(value(&view, cx), "hjkl");
    assert_eq!(cursor(&view, cx), 1);
    cx.simulate_keystrokes("i a escape");
    assert_eq!(value(&view, cx), "hajkl");
    cx.simulate_keystrokes("l x");
    assert_eq!(value(&view, cx), "hakl");

    cx.update(|_, cx| preferences::update(cx, |p| p.vim_mode = false).unwrap());
    cx.simulate_keystrokes("i");
    assert_eq!(value(&view, cx), "haikl");
}

#[gpui_kit::test]
fn vim_operators_counts_clipboard_and_undo_use_the_editor_history(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one two three\nfour\nfive", true);
    cx.simulate_keystrokes("2 d w");
    assert_eq!(value(&view, cx), "three\nfour\nfive");
    cx.simulate_keystrokes("u");
    assert_eq!(value(&view, cx), "one two three\nfour\nfive");
    cx.simulate_keystrokes("ctrl-r");
    assert_eq!(value(&view, cx), "three\nfour\nfive");
    cx.simulate_keystrokes("y y p");
    assert_eq!(value(&view, cx), "three\nthree\nfour\nfive");
    cx.simulate_keystrokes("G d d");
    assert_eq!(value(&view, cx), "three\nthree\nfour");
    cx.simulate_keystrokes("p");
    assert_eq!(value(&view, cx), "three\nthree\nfour\nfive");
}

#[gpui_kit::test]
fn vim_visual_selection_and_changes_are_unicode_safe(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "é猫🦅 rest\nsecond", true);
    cx.simulate_keystrokes("v 2 l d");
    assert_eq!(value(&view, cx), " rest\nsecond");
    cx.simulate_keystrokes("w c w");
    cx.simulate_input("new");
    cx.simulate_keystrokes("escape");
    assert_eq!(value(&view, cx), " new\nsecond");
    cx.simulate_keystrokes("g g V j y G p");
    assert_eq!(value(&view, cx), " new\nsecond\n new\nsecond");
}

#[gpui_kit::test]
fn vim_keeps_single_line_inputs_and_insert_mode_shortcuts_working(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "first\nlast", true);
    cx.simulate_keystrokes("o");
    cx.simulate_input("middle");
    cx.simulate_keystrokes("ctrl-[");
    assert_eq!(value(&view, cx), "first\nmiddle\nlast");

    cx.update(|window, cx| {
        view.read(cx)
            .input
            .clone()
            .update(cx, |input, cx| input.focus(window, cx));
    });
    cx.simulate_keystrokes("h j k l");
    cx.read(|cx| assert_eq!(view.read(cx).input.read(cx).value(), "hjkl"));
    assert_eq!(value(&view, cx), "first\nmiddle\nlast");
}

#[gpui_kit::test]
fn vim_line_operations_preserve_trailing_newlines_and_empty_lines(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "first\nlast\n", true);
    cx.simulate_keystrokes("j d d");
    assert_eq!(value(&view, cx), "first\n");
    cx.simulate_keystrokes("u c c");
    cx.simulate_input("new");
    cx.simulate_keystrokes("escape");
    assert_eq!(value(&view, cx), "first\nnew\n");
    cx.simulate_keystrokes("g g d G");
    assert_eq!(value(&view, cx), "");
    cx.simulate_keystrokes("u");
    assert_eq!(value(&view, cx), "first\nnew\n");
}

#[gpui_kit::test]
fn vim_paste_does_not_auto_close_brackets_or_drop_blank_lines(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "word", true);
    cx.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("{".into())));
    cx.simulate_keystrokes("P");
    assert_eq!(value(&view, cx), "{word");

    cx.update(|_, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string_with_json_metadata(
            "line\n\n".into(),
            serde_json::json!({ "request_eagle_vim_linewise": true }),
        ))
    });
    cx.simulate_keystrokes("p");
    assert_eq!(value(&view, cx), "{word\nline\n");
}

#[gpui_kit::test]
fn vim_change_word_at_word_end_does_not_consume_the_next_word(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "a word\n\nlast", true);
    cx.simulate_keystrokes("c w");
    cx.simulate_input("new");
    cx.simulate_keystrokes("escape j d $");
    assert_eq!(value(&view, cx), "new word\n\nlast");
}

#[gpui_kit::test]
fn vim_search_uses_the_native_search_field(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one two one", true);
    cx.simulate_keystrokes("/");
    cx.simulate_input("one");
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("n");
    cx.read(|cx| {
        let editor = view.read(cx).editor.read(cx);
        assert_eq!(editor.search_session().query, "one");
        assert!(editor.value()[editor.cursor()..].starts_with("one"));
        assert_eq!(editor.value(), "one two one");
    });
    let first = cursor(&view, cx);
    cx.simulate_keystrokes("n");
    assert_ne!(cursor(&view, cx), first);
    cx.simulate_keystrokes("N");
    assert_eq!(cursor(&view, cx), first);
}

#[gpui_kit::test]
fn vim_does_not_intercept_the_send_shortcut(cx: &mut TestAppContext) {
    use std::{cell::Cell, rc::Rc};

    let (view, cx) = setup(cx, "text", true);
    let sent = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "ctrl-enter",
            crate::SendRequest,
            None,
        )]);
        let sent = sent.clone();
        cx.on_action(move |_: &crate::SendRequest, _| sent.set(sent.get() + 1));
    });
    cx.simulate_keystrokes("ctrl-enter i ctrl-enter");
    assert_eq!(sent.get(), 2);
    assert_eq!(value(&view, cx), "text");
}
