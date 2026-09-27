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
            .child(
                gpui_kit::div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(Editor::new(&self.editor).h_full().text_sm())
                    .child(super::cursor(&self.vim)),
            )
    }
}

#[gpui_kit::test]
fn vim_l_reaches_the_final_character_without_crossing_the_line(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abc\né猫🦅\n", true);
    cx.simulate_keystrokes("l l");
    assert_eq!(cursor(&view, cx), 2);
    cx.simulate_keystrokes("l");
    assert_eq!(cursor(&view, cx), 2);
    cx.simulate_keystrokes("x");
    assert_eq!(value(&view, cx), "ab\né猫🦅\n");
    cx.simulate_keystrokes("j 0 l l");
    assert_eq!(cursor(&view, cx), "ab\né猫".len());
    cx.simulate_keystrokes("l x");
    assert_eq!(value(&view, cx), "ab\né猫\n");
}

#[gpui_kit::test]
fn normal_cursor_paints_a_block_and_insert_focus_and_disabled_modes_remove_it(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::ActiveTheme as _;

    let (view, cx) = setup(cx, "abc", true);
    cx.simulate_keystrokes("l l");
    let (bounds, color) = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let block = super::cursor::layout(&view.read(cx).vim, window, cx).unwrap();
        let editor = view.read(cx).editor.read(cx);
        let (caret, height) = editor.cursor_layout().unwrap();
        assert_eq!(editor.cursor(), 2);
        assert_eq!(block.bounds.left(), caret.left());
        assert!(block.bounds.size.width > caret.size.width * 2.);
        assert_eq!(block.bounds.size.height, height);
        let bounds = gpui_kit::Bounds::from_corners(
            window.pixel_snap_point(block.bounds.origin),
            window.pixel_snap_point(block.bounds.bottom_right()),
        )
        .scale(window.scale_factor());
        let color = gpui_kit::Background::from(cx.theme().foreground);
        assert!(
            window
                .painted_quads()
                .iter()
                .any(|quad| quad.bounds == bounds && quad.background == color)
        );
        (bounds, color)
    });

    cx.simulate_keystrokes("i");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_none());
        assert!(
            !window
                .painted_quads()
                .iter()
                .any(|quad| quad.bounds == bounds && quad.background == color)
        );
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        let input = view.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_none());
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        preferences::update(cx, |p| p.vim_mode = false).unwrap();
    });
    cx.update(|window, cx| {
        assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_none());
    });
}

#[gpui_kit::test]
fn block_cursor_follows_wrapping_scrolling_and_interface_size(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};

    let text = format!("{}\n{}last", "wrapped ".repeat(100), "line\n".repeat(80));
    let (view, cx) = setup(cx, &text, true);
    cx.update(|_, cx| request_eagle_theme::init(cx));

    for theme in ["Default Light", "Default Dark"] {
        for font_size in [12., 16., 24.] {
            cx.update(|window, cx| {
                assert!(request_eagle_theme::apply(theme, cx));
                window.set_rem_size(px(font_size));
                window.refresh();
            });
            cx.simulate_resize(size(px(40. * font_size), px(20. * font_size)));

            for keys in ["g g 100 l", "G $", "g g 0"] {
                cx.simulate_keystrokes(keys);
                cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    let block = super::cursor::layout(&view.read(cx).vim, window, cx).unwrap();
                    let editor = view.read(cx).editor.read(cx);
                    let (caret, height) = editor.cursor_layout().unwrap();
                    assert!(
                        (block.bounds.center().y - caret.center().y - editor.scroll_offset().y)
                            .abs()
                            < px(0.01)
                    );
                    assert_eq!(block.bounds.size.height, height);
                    assert!(
                        block.clip.contains(&block.bounds.center()),
                        "{theme}, {font_size}, {keys}: {:?} outside {:?}",
                        block.bounds,
                        block.clip
                    );
                    let bounds = gpui_kit::Bounds::from_corners(
                        window.pixel_snap_point(block.bounds.origin),
                        window.pixel_snap_point(block.bounds.bottom_right()),
                    )
                    .scale(window.scale_factor());
                    assert!(
                        window
                            .painted_quads()
                            .iter()
                            .any(|quad| quad.bounds == bounds)
                    );
                });
            }
        }
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
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .soft_wrap(true)
                .line_number(true)
                .default_value(value.to_owned())
        });
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

#[gpui_kit::test]
fn linewise_paste_preserves_trailing_and_leading_blank_lines(cx: &mut TestAppContext) {
    for (source, keys, expected, expected_cursor) in [
        ("one\n", "y y G p", "one\n\none", 5),
        ("one\n", "y y G P", "one\none\n", 4),
        ("\none\ntwo", "y y j P", "\n\none\ntwo", 1),
        ("\none\ntwo", "y y j p", "\none\n\ntwo", 5),
        ("\none\ntwo", "2 y y G p", "\none\ntwo\n\none", 9),
    ] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "{keys}");
        assert_eq!(cursor(&view, cx), expected_cursor, "{keys}");
    }
}

#[gpui_kit::test]
fn opening_changing_and_pasting_lines_preserve_crlf(cx: &mut TestAppContext) {
    for (source, keys, expected) in [
        ("one\r\ntwo", "o x escape", "one\r\nx\r\ntwo"),
        ("one\r\ntwo", "O x escape", "x\r\none\r\ntwo"),
        ("one\r\ntwo", "G o x escape", "one\r\ntwo\r\nx"),
        ("one\r\ntwo", "c c x escape", "x\r\ntwo"),
        ("one\r\ntwo\r\nlast", "2 c c x escape", "x\r\nlast"),
        ("one\r\ntwo", "G y y p", "one\r\ntwo\r\ntwo"),
        ("one\r\n", "y y G p", "one\r\n\r\none"),
    ] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "{keys}");
    }
}

#[gpui_kit::test]
fn clicking_elsewhere_cancels_a_pending_operator_and_count(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one two three", true);
    // Locate a real caret on "two", then click it while an operator is pending.
    cx.simulate_keystrokes("w");
    let point = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        view.read(cx)
            .editor
            .read(cx)
            .cursor_layout()
            .unwrap()
            .0
            .center()
    });
    cx.simulate_keystrokes("0 2 d");
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    assert_eq!(cursor(&view, cx), 4);
    cx.simulate_keystrokes("w");
    assert_eq!(value(&view, cx), "one two three");
    assert_eq!(cursor(&view, cx), 8);
}

#[gpui_kit::test]
fn linewise_yanks_preserve_the_column_and_backward_motions(cx: &mut TestAppContext) {
    for (keys, expected_cursor) in [
        ("3 l y y", 3),
        ("3 l y j", 3),
        ("3 l Y", 3),
        ("j 3 l y k", 3),
    ] {
        let (view, cx) = setup(cx, "abcdefgh\nabcdefgh", true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), "abcdefgh\nabcdefgh");
        assert_eq!(cursor(&view, cx), expected_cursor, "{keys}");
    }
}

#[gpui_kit::test]
fn word_operators_preserve_line_endings_and_count_empty_lines(cx: &mut TestAppContext) {
    for (source, keys, expected) in [
        ("word\nnext", "d w", "\nnext"),
        ("word  \n  next", "d W", "\n  next"),
        ("word\r\nnext", "d w", "\r\nnext"),
        ("one two\nnext", "2 d w", "\nnext"),
        ("word\nnext\nlast", "2 d w", "last"),
        ("word\n\nnext", "2 d w", "next"),
        ("\nnext", "d w", "next"),
    ] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "{source:?}: {keys}");
    }
    let (view, cx) = setup(cx, "word\nnext", true);
    cx.simulate_keystrokes("y w");
    cx.read(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "word"));
    assert_eq!(value(&view, cx), "word\nnext");
}

#[gpui_kit::test]
fn vertical_motions_remember_the_column_and_end_of_line(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abcdef\nx\nabcdefghi", true);
    cx.simulate_keystrokes("4 l j j");
    assert_eq!(cursor(&view, cx), 13);
    cx.simulate_keystrokes("k k");
    assert_eq!(cursor(&view, cx), 4);
    cx.simulate_keystrokes("$ j j");
    assert_eq!(cursor(&view, cx), 17);
    cx.simulate_keystrokes("h k k");
    assert_eq!(cursor(&view, cx), 5);
    cx.simulate_keystrokes("0 j j");
    assert_eq!(cursor(&view, cx), 9);
    cx.simulate_keystrokes("k 0 k");
    assert_eq!(cursor(&view, cx), 0);
}

#[gpui_kit::test]
fn redo_honors_a_count(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abc", true);
    cx.simulate_keystrokes("x x");
    assert_eq!(value(&view, cx), "c");
    cx.simulate_keystrokes("2 u");
    assert_eq!(value(&view, cx), "abc");
    cx.simulate_keystrokes("2 ctrl-r");
    assert_eq!(value(&view, cx), "c");
}

#[gpui_kit::test]
fn visual_o_swaps_the_active_end_without_editing(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abcdef\nsecond\nthird", true);
    cx.simulate_keystrokes("l v 2 l o h d");
    assert_eq!(value(&view, cx), "ef\nsecond\nthird");
    cx.simulate_keystrokes("u g g V j o j d");
    assert_eq!(value(&view, cx), "abcdef\nthird");
}

#[gpui_kit::test]
fn capital_delete_and_change_honor_line_counts(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "first\nsecond\nthird", true);
    cx.simulate_keystrokes("l 2 D");
    assert_eq!(value(&view, cx), "f\nthird");
    cx.simulate_keystrokes("u 0 l 2 C");
    cx.simulate_input("new");
    cx.simulate_keystrokes("escape");
    assert_eq!(value(&view, cx), "fnew\nthird");
}

#[gpui_kit::test]
fn whitespace_only_first_nonblank_commands_match_vim(cx: &mut TestAppContext) {
    // Vim 9.1 with -Nu NONE places ^/gg/G on the last blank, and I after it.
    // Returning column zero here would change Vim's behavior.
    for keys in ["^", "g g", "G"] {
        let (view, cx) = setup(cx, " \t  ", true);
        cx.simulate_keystrokes(keys);
        assert_eq!(cursor(&view, cx), 3, "{keys}");
        cx.simulate_keystrokes("I x escape");
        assert_eq!(value(&view, cx), " \t  x");
    }
}
