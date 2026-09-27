use gpui_kit::component::{
    input::{Editor, EditorState, Input, InputState},
    v_flex,
};
use gpui_kit::{
    AppContext, Context, Entity, Focusable, InteractiveElement, IntoElement, ParentElement, Render,
    Styled, TestAppContext, VisualTestContext, Window,
};

use super::Vim;

struct Harness {
    editor: Entity<EditorState>,
    input: Entity<InputState>,
    vim: Entity<Vim>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.vim.clone())
            .child(Input::new(&self.input))
            .child(
                gpui_kit::div()
                    .track_focus(&self.vim.focus_handle(cx))
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
        cx.bind_keys([
            gpui_kit::KeyBinding::new("ctrl-enter", crate::SendRequest, None),
            gpui_kit::KeyBinding::new("ctrl-g s", crate::SendRequest, None),
        ]);
        let sent = sent.clone();
        cx.on_action(move |_: &crate::SendRequest, _| sent.set(sent.get() + 1));
    });
    cx.simulate_keystrokes("ctrl-enter ctrl-g s i ctrl-enter");
    assert_eq!(sent.get(), 3);
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
        ("\r\nnext", "d w", "next"),
    ] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "{source:?}: {keys}");
    }
    let (view, cx) = setup(cx, "word\nnext", true);
    cx.simulate_keystrokes("y w");
    cx.read(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "word"));
    assert_eq!(value(&view, cx), "word\nnext");

    let (view, cx) = setup(cx, "one\r\n\r\nnext", true);
    cx.simulate_keystrokes("w");
    assert_eq!(cursor(&view, cx), 5);
    cx.simulate_keystrokes("w");
    assert_eq!(cursor(&view, cx), 7);
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

#[gpui_kit::test]
fn character_commands_keep_combining_sequences_and_joined_emoji_whole(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "e\u{301}👩\u{200d}🚀🇺🇸z", true);
    cx.simulate_keystrokes("l");
    assert_eq!(cursor(&view, cx), "e\u{301}".len());
    cx.simulate_keystrokes("l");
    assert_eq!(cursor(&view, cx), "e\u{301}👩\u{200d}🚀".len());
    cx.simulate_keystrokes("h x");
    assert_eq!(value(&view, cx), "e\u{301}🇺🇸z");
    cx.simulate_keystrokes("0 x");
    assert_eq!(value(&view, cx), "🇺🇸z");
    cx.simulate_keystrokes("v d");
    assert_eq!(value(&view, cx), "z");
}

#[test]
fn grapheme_navigation_crosses_rope_chunks_in_both_directions() {
    use unicode_segmentation::UnicodeSegmentation as _;
    let source = "e\u{301}👩\u{200d}🚀🇺🇸\r\n".repeat(500);
    let rope = gpui_kit::component::input::Rope::from(source.clone());
    assert!(rope.chunks().count() > 1);
    let mut expected: Vec<_> = source
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect();
    expected.push(source.len());

    for pair in expected.windows(2) {
        assert_eq!(super::grapheme::next(&rope, pair[0]), pair[1]);
        assert_eq!(super::grapheme::previous(&rope, pair[1]), pair[0]);
    }
}

#[gpui_kit::test]
fn unsupported_g_sequences_never_execute_their_suffix(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abc", true);
    cx.simulate_keystrokes("x");
    for keys in ["g x", "g i", "g u", "g o", "g p", "g d", "2 g x"] {
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), "bc", "{keys}");
        cx.read(|cx| assert!(view.read(cx).vim.read(cx).normal_editor().is_some()));
    }
    cx.simulate_keystrokes("l g g");
    assert_eq!(cursor(&view, cx), 0);
    cx.simulate_keystrokes("u g ctrl-r");
    assert_eq!(value(&view, cx), "abc");
}

#[gpui_kit::test]
fn vertical_motions_keep_the_rendered_column_after_tabs(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "\tabc\n0123456789\n\tabc", true);
    cx.simulate_keystrokes("l");
    let left = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        view.read(cx)
            .editor
            .read(cx)
            .cursor_layout()
            .unwrap()
            .0
            .left()
    });
    cx.simulate_keystrokes("j");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let caret = view.read(cx).editor.read(cx).cursor_layout().unwrap().0;
        assert!((caret.left() - left).abs() < gpui_kit::px(0.01));
    });
    cx.simulate_keystrokes("j");
    assert_eq!(cursor(&view, cx), "\tabc\n0123456789\n\t".len());
}

#[gpui_kit::test]
fn counted_insert_and_open_commands_repeat_the_completed_edit(cx: &mut TestAppContext) {
    for (keys, expected) in [
        ("3 i a b c escape", "abcabcabcold"),
        ("3 a a b c escape", "oabcabcabcld"),
        ("3 I a b c escape", "abcabcabcold"),
        ("3 A a b c escape", "oldabcabcabc"),
        ("3 o a b c escape", "old\nabc\nabc\nabc"),
        ("3 O a b c escape", "abc\nabc\nabc\nold"),
        ("3 i a b backspace escape", "aaaold"),
        ("3 o escape", "old\n\n\n"),
        ("3 i a b left escape", "abold"),
    ] {
        let (view, cx) = setup(cx, "old", true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "{keys}");
    }
    let (view, cx) = setup(cx, "old\r\nlast", true);
    cx.simulate_keystrokes("3 o a escape");
    assert_eq!(value(&view, cx), "old\r\na\r\na\r\na\r\nlast");
}

#[gpui_kit::test]
fn backward_words_stop_at_each_empty_line(cx: &mut TestAppContext) {
    for ending in ["\n", "\r\n"] {
        let source = format!("one{ending}{ending}next");
        let (view, cx) = setup(cx, &source, true);
        cx.simulate_keystrokes("G b");
        assert_eq!(cursor(&view, cx), 3 + ending.len());
        cx.simulate_keystrokes("b");
        assert_eq!(cursor(&view, cx), 0);
        cx.simulate_keystrokes("G y B");
        assert_eq!(cursor(&view, cx), 3 + ending.len());
        cx.simulate_keystrokes("G d B");
        assert_eq!(value(&view, cx), format!("one{ending}next"));
        cx.read(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), ending));
    }
}

#[gpui_kit::test]
fn visual_paste_swaps_the_register_but_capital_p_preserves_it(cx: &mut TestAppContext) {
    for (paste, expected_register) in [("p", "bar"), ("P", "foo")] {
        let (view, cx) = setup(cx, "foo bar", true);
        cx.simulate_keystrokes(&format!("y e w v e {paste}"));
        assert_eq!(value(&view, cx), "foo foo");
        cx.read(|cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                expected_register
            )
        });
        cx.simulate_keystrokes("p");
        assert_eq!(value(&view, cx), format!("foo foo{expected_register}"));
    }
}

#[gpui_kit::test]
fn linewise_delete_lands_on_the_first_nonblank_of_the_survivor(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one\n  two\n\tlast", true);
    cx.simulate_keystrokes("d d");
    assert_eq!(cursor(&view, cx), 2);
    cx.simulate_keystrokes("x");
    assert_eq!(value(&view, cx), "  wo\n\tlast");
    cx.simulate_keystrokes("G d d");
    assert_eq!(cursor(&view, cx), 2);
}

#[gpui_kit::test]
fn search_repeat_extends_visual_selections(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one two one\nlast one", true);
    cx.simulate_keystrokes("/");
    cx.simulate_input("one");
    cx.simulate_keystrokes("escape g g 0 v n d");
    assert_eq!(value(&view, cx), "ne\nlast one");
    cx.simulate_keystrokes("u G V N d");
    assert_eq!(value(&view, cx), "");
}

#[gpui_kit::test]
fn linewise_registers_use_the_destination_editors_line_endings(cx: &mut TestAppContext) {
    for (source_ending, destination_ending) in [("\r\n", "\n"), ("\n", "\r\n")] {
        let source = format!("  one{source_ending}{source_ending}last");
        let (_, source_cx) = setup(cx, &source, true);
        source_cx.simulate_keystrokes("3 y y");
        let destination = format!("top{destination_ending}bottom");
        let (view, destination_cx) = setup(cx, &destination, true);
        destination_cx.simulate_keystrokes("p");
        assert_eq!(
            value(&view, destination_cx),
            format!(
                "top{destination_ending}  one{destination_ending}{destination_ending}last{destination_ending}bottom"
            )
        );
        assert_eq!(cursor(&view, destination_cx), 5 + destination_ending.len());
    }
}

#[gpui_kit::test]
fn linewise_paste_targets_the_first_nonblank_character(cx: &mut TestAppContext) {
    for (paste, expected_cursor) in [("p", 8), ("P", 2)] {
        let (view, cx) = setup(cx, "  one\nlast", true);
        cx.simulate_keystrokes(&format!("y y {paste}"));
        assert_eq!(cursor(&view, cx), expected_cursor);
        cx.simulate_keystrokes("x");
        let expected = if paste == "p" {
            "  one\n  ne\nlast"
        } else {
            "  ne\n  one\nlast"
        };
        assert_eq!(value(&view, cx), expected);
    }
}

#[gpui_kit::test]
fn display_column_measurement_does_not_copy_the_unused_line_tail(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx, "small editor", true);
    let text = gpui_kit::component::input::Rope::from("a".repeat(4 * 1024 * 1024));
    cx.update(|window, cx| {
        let allocated = crate::test_allocator::allocated_by(|| {
            let column = super::column::Column::at(&text, 10, window, cx);
            assert_eq!(column.offset(&text, 0, window, cx), 10);
        });
        assert!(
            allocated < 128 * 1024,
            "allocated {allocated} bytes for column 10"
        );
    });
}

#[gpui_kit::test]
fn clamped_vertical_operators_do_not_delete_the_current_line(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one line", true);
    cx.simulate_keystrokes("j k d j d k");
    assert_eq!(cursor(&view, cx), 0);
    assert_eq!(value(&view, cx), "one line");
}

#[gpui_kit::test]
fn linewise_paste_replaces_an_empty_buffer_without_losing_blank_lines(cx: &mut TestAppContext) {
    for paste in ["p", "P"] {
        for (register, count, expected) in [
            ("one\n", "", "one"),
            ("one\n", "2 ", "one\none"),
            ("one\n\n", "", "one\n"),
            ("\n", "", ""),
            ("\n\n", "", "\n"),
            ("one\r\n\r\n", "", "one\n"),
        ] {
            let (view, cx) = setup(cx, "", true);
            cx.update(|_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string_with_json_metadata(
                    register.into(),
                    serde_json::json!({ "request_eagle_vim_linewise": true }),
                ));
            });
            cx.simulate_keystrokes(&format!("{count}{paste}"));
            assert_eq!(value(&view, cx), expected, "{register:?}: {count}{paste}");
        }
    }
}

#[gpui_kit::test]
fn yanks_reset_the_desired_column_to_the_resulting_cursor(cx: &mut TestAppContext) {
    for (yank, column) in [("y b", 0), ("y y", 1), ("y w", 1), ("Y", 1)] {
        let (view, cx) = setup(cx, "0123456789\nab\n0123456789", true);
        cx.simulate_keystrokes(&format!("8 l j {yank} j"));
        assert_eq!(cursor(&view, cx), 14 + column, "{yank}");
    }
}

#[gpui_kit::test]
fn visual_line_paste_preserves_boundaries_and_places_the_cursor_at_the_first_token(
    cx: &mut TestAppContext,
) {
    for ending in ["\n", "\r\n"] {
        for paste in ["p", "P"] {
            for (source, keys, register, linewise, expected) in [
                ("one\ntwo\nthree", "j V", "  xx", false, "one\n  xx\nthree"),
                ("one\ntwo", "G V", "  xx", false, "one\n  xx"),
                ("one\ntwo", "G V", "  xx\n", true, "one\n  xx"),
                ("one\n", "g g V", "  xx", false, "  xx\n"),
                ("one\n", "G V", "  xx", false, "one\n  xx"),
                ("one", "V", "  xx\n", false, "  xx\n"),
                ("one\ntwo", "G V", "\n", true, "one\n"),
            ] {
                let source = source.replace('\n', ending);
                let (view, cx) = setup(cx, &source, true);
                cx.update(|_, cx| {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string_with_json_metadata(
                        register.replace('\n', ending),
                        serde_json::json!({ "request_eagle_vim_linewise": linewise }),
                    ));
                });
                cx.simulate_keystrokes(&format!("{keys} {paste}"));
                let destination_ending = if source.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let expected = expected.replace('\n', destination_ending);
                assert_eq!(value(&view, cx), expected, "{source:?}: {keys} {paste}");
                if let Some(column) = expected.find("xx") {
                    assert_eq!(cursor(&view, cx), column);
                }
            }
        }
    }
}

#[gpui_kit::test]
fn native_editing_shortcuts_cannot_bypass_normal_or_visual_mode(cx: &mut TestAppContext) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    for mode in ["", "v l"] {
        let (view, cx) = setup(cx, "one two", true);
        cx.update(|_, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("paste".into()));
            cx.bind_keys([gpui_kit::KeyBinding::new(
                "alt-p",
                gpui_kit::component::input::Paste,
                Some("Input"),
            )]);
        });
        cx.simulate_keystrokes(&format!("4 l {mode}"));
        let selected = cx.read(|cx| view.read(cx).editor.read(cx).selected_range());
        for shortcut in [
            format!("{modifier}-v"),
            format!("{modifier}-x"),
            format!("{modifier}-]"),
            format!("{modifier}-z"),
            "ctrl-backspace".into(),
            "alt-backspace".into(),
            "ctrl-delete".into(),
            "alt-delete".into(),
            "alt-p".into(),
        ] {
            cx.simulate_keystrokes(&shortcut);
            assert_eq!(value(&view, cx), "one two", "{mode}: {shortcut}");
            cx.read(|cx| assert_eq!(view.read(cx).editor.read(cx).selected_range(), selected));
        }
        cx.simulate_keystrokes(&format!("escape i {modifier}-v"));
        assert!(value(&view, cx).contains("paste"));
    }
}

#[gpui_kit::test]
fn linewise_registers_pasted_over_character_selections_split_the_surrounding_line(
    cx: &mut TestAppContext,
) {
    for ending in ["\n", "\r\n"] {
        for (keys, expected) in [
            ("3 l v 2 l p", "abc\n  one\nghi\nlast"),
            ("v 2 l P", "\n  one\nDEFghi\nlast"),
            ("6 l v 2 l p", "abcDEF\n  one\n\nlast"),
            ("v j p", "\n  one\nast"),
        ] {
            let (view, cx) = setup(cx, &"abcDEFghi\nlast".replace('\n', ending), true);
            cx.update(|_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string_with_json_metadata(
                    format!("  one{ending}"),
                    serde_json::json!({ "request_eagle_vim_linewise": true }),
                ));
            });
            cx.simulate_keystrokes(keys);
            let expected = expected.replace('\n', ending);
            assert_eq!(value(&view, cx), expected, "{keys}");
            assert_eq!(cursor(&view, cx), expected.find("one").unwrap());
        }
    }
}

#[gpui_kit::test]
fn application_shortcuts_can_override_plain_vim_keys_and_chords(cx: &mut TestAppContext) {
    use std::{cell::Cell, rc::Rc};

    let (view, cx) = setup(cx, "text", true);
    let sent = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("x", crate::SendRequest, None),
            gpui_kit::KeyBinding::new("g s", crate::SendRequest, None),
        ]);
        let sent = sent.clone();
        cx.on_action(move |_: &crate::SendRequest, _| sent.set(sent.get() + 1));
    });
    cx.simulate_keystrokes("x g s 2 d x l");
    assert_eq!(sent.get(), 3);
    assert_eq!(value(&view, cx), "text");
    assert_eq!(cursor(&view, cx), 1);
}

#[gpui_kit::test]
fn redo_cancels_pending_operators_without_changing_history(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abc", true);
    cx.simulate_keystrokes("x u");
    for operator in ["d", "c", "y", "2 d"] {
        cx.simulate_keystrokes(&format!("{operator} ctrl-r"));
        assert_eq!(value(&view, cx), "abc", "{operator}");
    }
    cx.simulate_keystrokes("ctrl-r");
    assert_eq!(value(&view, cx), "bc");
}

#[gpui_kit::test]
fn visual_uppercase_operators_apply_to_all_selected_lines(cx: &mut TestAppContext) {
    for ending in ["\n", "\r\n"] {
        for selection in ["2 l v j", "j 2 l v k", "V j"] {
            for (operator, expected) in [
                ("D", "last"),
                ("X", "last"),
                ("C", "\nlast"),
                ("S", "\nlast"),
                ("Y", "AbcdEF\n  ghIjK\nlast"),
            ] {
                let (view, cx) = setup(cx, &"AbcdEF\n  ghIjK\nlast".replace('\n', ending), true);
                cx.simulate_keystrokes(&format!("{selection} {operator}"));
                assert_eq!(
                    value(&view, cx),
                    expected.replace('\n', ending),
                    "{selection} {operator}"
                );
                cx.read(|cx| {
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        format!("AbcdEF{ending}  ghIjK{ending}")
                    )
                });
            }
        }
    }
}

#[gpui_kit::test]
fn visual_case_changes_do_not_undo_or_overwrite_the_register(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "ABCDé\nNext", true);
    cx.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("kept".into())));
    cx.simulate_keystrokes("A x escape 0 l v 2 l u");
    assert_eq!(value(&view, cx), "Abcdéx\nNext");
    cx.simulate_keystrokes("V U");
    assert_eq!(value(&view, cx), "ABCDÉX\nNext");
    cx.read(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "kept"));
    cx.simulate_keystrokes("u");
    assert_eq!(value(&view, cx), "Abcdéx\nNext");
}

#[gpui_kit::test]
fn visual_unsupported_insert_commands_do_not_enter_insert_mode(cx: &mut TestAppContext) {
    for key in ["i", "a"] {
        let (view, cx) = setup(cx, "abcd", true);
        cx.simulate_keystrokes(&format!("v l {key} w escape"));
        assert_eq!(value(&view, cx), "abcd");
        cx.read(|cx| assert!(view.read(cx).vim.read(cx).normal_editor().is_some()));
    }
}

#[gpui_kit::test]
fn forward_search_commits_the_counted_match_and_wraps(cx: &mut TestAppContext) {
    for (keys, query, expected_cursor) in [
        ("3 /", "one", 14),
        ("G $ 2 /", "one", 10),
        ("6 l /", "one", 10),
        ("2 l 3 /", "absent", 2),
    ] {
        let (view, cx) = setup(cx, "start one one one tail", true);
        cx.simulate_keystrokes(keys);
        cx.simulate_input(query);
        cx.simulate_keystrokes("enter");
        assert_eq!(cursor(&view, cx), expected_cursor, "{keys}");
        cx.read(|cx| assert!(!view.read(cx).editor.read(cx).search_session().open));
        assert_eq!(value(&view, cx), "start one one one tail");
    }
}

#[gpui_kit::test]
fn native_search_extends_visual_selections_and_escape_restores_them(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one two one\nlast one", true);
    cx.simulate_keystrokes("l v 2 /");
    cx.simulate_input("one");
    cx.simulate_keystrokes("enter d");
    assert_eq!(value(&view, cx), "one");

    let (view, cx) = setup(cx, "one\nmiddle\nlast one\nend", true);
    cx.simulate_keystrokes("V /");
    cx.simulate_input("last");
    cx.simulate_keystrokes("enter d");
    assert_eq!(value(&view, cx), "end");

    let (view, cx) = setup(cx, "one two one", true);
    cx.simulate_keystrokes("l v l /");
    cx.simulate_input("two");
    cx.simulate_keystrokes("escape d");
    assert_eq!(value(&view, cx), "o two one");
}

#[gpui_kit::test]
fn refocusing_the_editor_abandons_a_pending_search_motion(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one two one", true);
    cx.simulate_keystrokes("v /");
    cx.simulate_input("one");
    cx.update(|window, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(4..4, cx);
            editor.focus(window, cx);
        });
    });
    cx.simulate_keystrokes("l");
    assert_eq!(cursor(&view, cx), 5);
    cx.read(|cx| assert!(view.read(cx).vim.read(cx).normal_editor().is_some()));
}

#[gpui_kit::test]
fn characterwise_registers_use_the_destination_line_endings(cx: &mut TestAppContext) {
    for (source_ending, destination_ending) in [("\r\n", "\n"), ("\n", "\r\n")] {
        let source = format!("one{source_ending}second{source_ending}last");
        let (_, source_cx) = setup(cx, &source, true);
        source_cx.simulate_keystrokes("l v j y");
        let destination = format!("top{destination_ending}bottom");
        let (view, destination_cx) = setup(cx, &destination, true);
        destination_cx.simulate_keystrokes("P");
        assert_eq!(
            value(&view, destination_cx),
            format!("ne{destination_ending}setop{destination_ending}bottom")
        );
    }
}

#[gpui_kit::test]
fn external_clipboard_text_is_pasted_literally(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "one\ntwo", true);
    cx.update(|_, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("raw\r\ntext".into()))
    });
    cx.simulate_keystrokes("P");
    assert_eq!(value(&view, cx), "raw\r\ntextone\ntwo");
}

#[gpui_kit::test]
fn end_word_motions_skip_empty_lines_like_vim(cx: &mut TestAppContext) {
    // :help e and :help E explicitly say they do not stop in an empty line.
    for ending in ["\n", "\r\n"] {
        for keys in ["$ e", "$ E", "$ 2 e", "$ 2 E"] {
            let source = format!("one{ending}{ending}next");
            let (view, cx) = setup(cx, &source, true);
            cx.simulate_keystrokes(keys);
            assert_eq!(cursor(&view, cx), source.len() - 1, "{keys}");
        }
        let (view, cx) = setup(cx, &format!("one{ending}{ending}next"), true);
        cx.simulate_keystrokes("$ d e");
        assert_eq!(value(&view, cx), "on");
    }
}

#[gpui_kit::test]
fn escaping_an_empty_insertion_moves_left_like_vim(cx: &mut TestAppContext) {
    // A clean Vim 9.1 run of lli<Esc> on abc ends on b; I<Esc> on
    // "  abc" ends on the second blank. An unchanged insertion still moves left.
    for (source, keys) in [
        ("abc", "l l i escape"),
        ("abc", "l l 3 i escape"),
        ("  abc", "I escape"),
        ("  abc", "3 I escape"),
    ] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(keys);
        assert_eq!(cursor(&view, cx), 1, "{keys}");
        assert_eq!(value(&view, cx), source);
    }
}
