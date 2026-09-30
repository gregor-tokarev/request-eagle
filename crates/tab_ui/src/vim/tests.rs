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
        let mouse_vim = self.vim.clone();

        v_flex()
            .size_full()
            .child(self.vim.clone())
            .child(Input::new(&self.input))
            .child(
                gpui_kit::div()
                    .track_focus(&self.vim.focus_handle(cx))
                    .capture_any_mouse_down(move |_, _, cx| {
                        mouse_vim.update(cx, |vim, _| vim.mouse_down());
                    })
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

#[gpui_kit::test]
fn visual_block_cursor_follows_the_active_end_and_replaces_the_caret(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abcdef\nsecond", true);
    // The editor only paints its caret in an active window.
    cx.update(|window, _| window.activate_window());
    let paint = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let block = super::cursor::layout(&view.read(cx).vim, window, cx);
            let (caret, _) = view.read(cx).editor.read(cx).cursor_layout().unwrap();
            let caret = gpui_kit::Bounds::from_corners(
                window.pixel_snap_point(caret.origin),
                window.pixel_snap_point(caret.bottom_right()),
            )
            .scale(window.scale_factor());
            let caret_painted = window
                .painted_quads()
                .iter()
                .any(|quad| quad.bounds == caret);

            (block.map(|block| block.bounds), caret_painted)
        })
    };

    // Normal-mode blocks mark where the cursor belongs on each character.
    cx.simulate_keystrokes("l");
    let (first, caret_painted) = paint(cx);
    assert!(first.is_some() && !caret_painted);
    cx.simulate_keystrokes("2 l");
    let (third, _) = paint(cx);

    for (keys, block) in [("2 h v 2 l", third), ("o", first), ("escape j V k", first)] {
        cx.simulate_keystrokes(keys);
        assert_eq!(paint(cx), (block, false), "{keys}");
    }

    cx.simulate_keystrokes("escape i");
    assert_eq!(paint(cx), (None, true));
}

#[gpui_kit::test]
fn visual_selections_scroll_with_the_vim_cursor(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};

    let lines = "line\n".repeat(200);
    // A wrapped line taller than the editor: Visual Line mode must reveal its
    // first character, not the start of the next line.
    let tall_line = format!("short\n{}\nend", "wrapped ".repeat(2000));

    for (text, keys) in [
        (&lines, "V 1 0 0 j"),
        (&lines, "G V 1 0 0 k"),
        (&lines, "v 1 5 0 j"),
        (&lines, "G v 1 5 0 k"),
        (&tall_line, "j V"),
    ] {
        let (view, cx) = setup(cx, text, true);
        cx.simulate_resize(size(px(400.), px(200.)));
        cx.simulate_keystrokes(keys);
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let block = super::cursor::layout(&view.read(cx).vim, window, cx).unwrap();
            assert!(
                block.clip.contains(&block.bounds.center()),
                "{keys}: {:?} outside {:?}",
                block.bounds,
                block.clip
            );
        });
    }
}

#[gpui_kit::test]
fn block_cursor_hides_when_its_line_scrolls_out_of_view(cx: &mut TestAppContext) {
    use gpui_kit::{point, px, size};

    let (view, cx) = setup(cx, &"line\n".repeat(200), true);
    cx.simulate_resize(size(px(400.), px(200.)));
    cx.simulate_keystrokes("l l");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_some());

        let editor = view.read(cx).editor.clone();
        let line_height = editor.read(cx).line_height().unwrap();
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(0.), -line_height * 50.), cx)
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_none());
    });
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
fn mouse_drag_selections_enter_visual_mode_and_keep_the_active_end(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};

    for (reverse, keys, expected) in [
        (false, "x", "abghi"),
        (true, "d", "abghi"),
        (false, "h x", "abfghi"),
        (true, "l x", "abcghi"),
    ] {
        let (view, cx) = setup(cx, "abcdefghi", true);
        if reverse {
            cx.simulate_keystrokes("v l");
        }
        let (start, end) = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let editor = view.read(cx).editor.read(cx);
            let start = editor.range_to_bounds(&(2..2)).unwrap().center();
            let end = editor.range_to_bounds(&(6..6)).unwrap().center();
            if reverse { (end, start) } else { (start, end) }
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.read(|cx| assert_eq!(view.read(cx).editor.read(cx).selected_range(), 2..6));
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "reverse={reverse}, {keys}");
    }
}

#[gpui_kit::test]
fn native_visual_selections_expand_to_graphemes_and_preserve_crlf(cx: &mut TestAppContext) {
    let text = "Ae\u{301}👩\u{200d}🚀\r\nB";
    let end = text.find('B').unwrap();
    for reverse in [false, true] {
        let (view, cx) = setup(cx, text, true);
        cx.update(|_, cx| {
            view.read(cx).editor.clone().update(cx, |editor, cx| {
                // Begin inside the combining cluster; the visual range must
                // expand to the whole grapheme and retain the complete CRLF.
                let (start, end) = if reverse { (end, 2) } else { (2, end) };
                editor.set_selected_range(start..end, cx);
            });
        });
        cx.simulate_keystrokes("y");
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(&text[1..end])
        );
        assert_eq!(value(&view, cx), text);
    }
}

#[gpui_kit::test]
fn insert_mode_and_disabled_vim_keep_native_selection_editing(cx: &mut TestAppContext) {
    for enabled in [false, true] {
        let (view, cx) = setup(cx, "abcdefghi", enabled);
        if enabled {
            cx.simulate_keystrokes("i");
        }
        cx.update(|_, cx| {
            view.read(cx).editor.clone().update(cx, |editor, cx| {
                editor.set_selected_range(2..6, cx);
            });
        });
        cx.simulate_keystrokes("x");
        assert_eq!(value(&view, cx), "abxghi");
    }
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
fn same_position_click_cancels_partial_vim_commands(cx: &mut TestAppContext) {
    for pending in ["d", "2", "2 d", "g", "d g"] {
        let (view, cx) = setup(cx, "one two three", true);
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
        cx.simulate_keystrokes(pending);
        cx.simulate_click(point, gpui_kit::Modifiers::default());
        assert_eq!(cursor(&view, cx), 0);
        cx.simulate_keystrokes("w");
        assert_eq!(value(&view, cx), "one two three", "{pending}");
        assert_eq!(cursor(&view, cx), 4, "{pending}");
    }
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
        cx.read(|cx| assert!(view.read(cx).vim.read(cx).is_normal()));
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
            let mut column = super::column::Column::at(&text, 10);
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
fn application_shortcuts_can_override_escape_in_every_vim_mode(cx: &mut TestAppContext) {
    use std::{cell::Cell, rc::Rc};

    for key in ["escape", "ctrl-["] {
        for mode in ["l", "v", "V", "i"] {
            let (view, cx) = setup(cx, "text", true);
            let sent = Rc::new(Cell::new(0));
            cx.update(|_, cx| {
                cx.bind_keys([gpui_kit::KeyBinding::new(key, crate::SendRequest, None)]);
                let sent = sent.clone();
                cx.on_action(move |_: &crate::SendRequest, _| sent.set(sent.get() + 1));
            });
            cx.simulate_keystrokes(&format!("{mode} {key}"));
            assert_eq!(sent.get(), 1, "{mode} {key}");
            assert_eq!(value(&view, cx), "text");
        }
    }
}

#[gpui_kit::test]
fn digits_after_g_cancel_the_prefix_and_operator(cx: &mut TestAppContext) {
    let source = "first\nsecond\nthird\nfourth\n";
    for invalid in ["g 2", "d g 2", "c g 2", "y g 2", "2 d g 2"] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(&format!("3 G {invalid} g g"));
        assert_eq!(value(&view, cx), source, "{invalid}");
        assert_eq!(cursor(&view, cx), 0, "{invalid}");
    }
    for (keys, expected, offset) in [
        ("3 G 2 g g", source, 6),
        ("3 G d 2 g g", "first\nfourth\n", 6),
        ("3 G 2 d g g", "first\nfourth\n", 6),
    ] {
        let (view, cx) = setup(cx, source, true);
        cx.simulate_keystrokes(keys);
        assert_eq!(value(&view, cx), expected, "{keys}");
        assert_eq!(cursor(&view, cx), offset, "{keys}");
    }
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
        cx.read(|cx| assert!(view.read(cx).vim.read(cx).is_normal()));
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
    cx.read(|cx| assert!(view.read(cx).vim.read(cx).is_normal()));
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

#[gpui_kit::test]
fn cursor_motions_do_not_redraw_the_unchanged_mode_indicator(cx: &mut TestAppContext) {
    use std::{cell::Cell, rc::Rc};

    let (view, cx) = setup(cx, "abcdef\nABCDEF\nlast", true);
    let updates = Rc::new(Cell::new(0));
    let _subscription = cx.update(|_, cx| {
        let updates = updates.clone();
        let vim = view.read(cx).vim.clone();
        cx.observe(&vim, move |_, _| {
            updates.set(updates.get() + 1);
        })
    });

    cx.simulate_keystrokes("j l h k 2 g escape z");
    assert_eq!(cursor(&view, cx), 0);
    assert_eq!(updates.get(), 0);

    for (keys, expected) in [
        ("v", 1),
        ("l j o", 1),
        ("V", 2),
        ("v", 3),
        ("escape", 4),
        ("i", 5),
        ("x", 5),
        ("escape", 6),
    ] {
        cx.simulate_keystrokes(keys);
        assert_eq!(updates.get(), expected, "{keys}");
    }

    cx.update(|_, cx| preferences::update(cx, |p| p.vim_mode = false).unwrap());
    assert_eq!(updates.get(), 7);
}

#[gpui_kit::test]
fn disabled_application_chords_leave_their_prefixes_to_vim(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "first\nsecond", true);
    cx.update(|_, cx| {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("d x", crate::SendRequest, None),
            gpui_kit::KeyBinding::new("d x", gpui_kit::NoAction, None),
        ]);
    });
    cx.simulate_keystrokes("d d");
    assert_eq!(value(&view, cx), "second");
}

#[gpui_kit::test]
fn enabling_vim_collapses_the_native_selection_and_normalizes_the_caret(cx: &mut TestAppContext) {
    for (text, selection, expected) in [
        ("abc", 3..3, 2),
        ("abc", 0..3, 2),
        ("abc\ndef", 0..2, 2),
        ("", 0..0, 0),
        ("abé", 4..4, 2),
    ] {
        let (view, cx) = setup(cx, text, false);
        cx.update(|_, cx| {
            view.read(cx)
                .editor
                .clone()
                .update(cx, |editor, cx| editor.set_selected_range(selection, cx));
            preferences::update(cx, |p| p.vim_mode = true).unwrap();
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert_eq!(
                view.read(cx).editor.read(cx).selected_range(),
                expected..expected
            );
            assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_some());
        });
        assert_eq!(value(&view, cx), text);
    }
}

#[gpui_kit::test]
fn vertical_motions_from_long_prefixes_only_measure_what_the_destination_needs(
    cx: &mut TestAppContext,
) {
    let (_, cx) = setup(cx, "small editor", true);
    let text =
        gpui_kit::component::input::Rope::from(format!("{}\nshort\n", "a".repeat(4 * 1024 * 1024)));
    cx.update(|window, cx| {
        let allocated = crate::test_allocator::allocated_by(|| {
            let mut column = super::column::Column::at(&text, 4 * 1024 * 1024 - 10);
            assert_eq!(
                column.offset(&text, 4 * 1024 * 1024 + 1, window, cx),
                text.len() - 2
            );
            assert_eq!(column.offset(&text, text.len(), window, cx), text.len());
            assert_eq!(column.offset(&text, 0, window, cx), 4 * 1024 * 1024 - 10);
        });
        assert!(
            allocated < 128 * 1024,
            "allocated {allocated} bytes for a long source prefix"
        );
    });
}

#[gpui_kit::test]
fn lazy_display_columns_survive_short_lines_and_match_the_full_rendered_prefix(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{
        TextRun,
        component::{ActiveTheme as _, input::Rope},
        font, rems,
    };

    let (_, cx) = setup(cx, "small editor", true);
    for prefix in [
        "ab\t".repeat(90),
        "é猫e\u{301}👩\u{200d}🚀".repeat(30),
        "Wi".repeat(100),
    ] {
        let target = "0123456789".repeat(200);
        let source = format!("{prefix}end\nx\n{target}");
        let text = Rope::from(source);
        cx.update(|window, cx| {
            let font = font(cx.theme().mono_font_family.clone());
            let size = rems(0.875).to_pixels(window.rem_size());
            let x = window
                .text_system()
                .shape_line(
                    prefix.clone().into(),
                    size,
                    &[TextRun {
                        len: prefix.len(),
                        font: font.clone(),
                        ..Default::default()
                    }],
                    None,
                )
                .width;
            let shaped = window.text_system().shape_line(
                target.clone().into(),
                size,
                &[TextRun {
                    len: target.len(),
                    font,
                    ..Default::default()
                }],
                None,
            );
            let mut column = super::column::Column::at(&text, prefix.len());
            let short = prefix.len() + 4;
            assert_eq!(column.offset(&text, short, window, cx), short);
            assert_eq!(
                column.offset(&text, short + 2, window, cx),
                short + 2 + shaped.closest_index_for_x(x)
            );
            assert_eq!(column.offset(&text, 0, window, cx), prefix.len());
        });
    }
}

#[gpui_kit::test]
fn shared_dispatch_keeps_search_and_unrelated_input_scoped_after_other_editors_close(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(cx, "one two one", true);
    let retained = cx.update(|window, cx| {
        (0..100)
            .map(|_| {
                let editor = cx.new(|cx| EditorState::new(window, cx));
                cx.new(|cx| Vim::new(editor, cx))
            })
            .collect::<Vec<_>>()
    });
    cx.simulate_keystrokes("l");
    assert_eq!(cursor(&view, cx), 1);
    drop(retained);
    cx.run_until_parked();
    cx.simulate_keystrokes("/");
    cx.simulate_input("one");
    cx.simulate_keystrokes("enter");
    assert_eq!(cursor(&view, cx), 8);
    cx.update(|window, cx| {
        view.read(cx)
            .input
            .clone()
            .update(cx, |input, cx| input.focus(window, cx));
    });
    cx.simulate_keystrokes("h j k l");
    cx.read(|cx| assert_eq!(view.read(cx).input.read(cx).value().as_str(), "hjkl"));
    assert_eq!(value(&view, cx), "one two one");
}

#[gpui_kit::test]
fn search_cancellation_routes_to_its_owner_after_another_editor_receives_keys(
    cx: &mut TestAppContext,
) {
    struct Pair(Entity<Harness>, Entity<Harness>);

    impl Render for Pair {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .child(self.0.clone())
                .child(self.1.clone())
        }
    }

    let (first, cx) = setup(cx, "one two one", true);
    let second = cx.update(|window, cx| {
        let second = cx.new(|cx| {
            let editor = cx.new(|cx| EditorState::new(window, cx).default_value("abcd"));
            let input = cx.new(|cx| InputState::new(window, cx));
            let vim = cx.new(|cx| Vim::new(editor.clone(), cx));
            Harness { editor, input, vim }
        });
        window.replace_root(cx, |_, _| Pair(first.clone(), second.clone()));
        second
    });
    cx.simulate_keystrokes("/");
    cx.simulate_input("one");
    let search_focus = cx.update(|window, cx| {
        let search = window.focused(cx).unwrap();
        second
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx));
        search
    });
    cx.simulate_keystrokes("l");
    assert_eq!(cursor(&second, cx), 1);
    cx.update(|window, cx| window.focus(&search_focus, cx));
    cx.simulate_keystrokes("escape");
    assert_eq!(cursor(&first, cx), 0);
    assert_eq!(value(&first, cx), "one two one");
}

#[gpui_kit::test]
fn external_line_copies_use_linewise_paste_without_changing_characterwise_registers(
    cx: &mut TestAppContext,
) {
    for ending in ["\n", "\r\n"] {
        for paste in ["p", "P"] {
            for (metadata, trailing, expected) in [
                (None, 1, "one\nx\nthree"),
                (None, 2, "one\nx\n\nthree"),
                (Some(false), 1, "one\nx\n\nthree"),
                (Some(true), 1, "one\nx\nthree"),
            ] {
                let (view, cx) = setup(cx, &"one\ntwo\nthree".replace('\n', ending), true);
                cx.update(|_, cx| {
                    let text = format!("x{}", ending.repeat(trailing));
                    let item = if let Some(linewise) = metadata {
                        gpui_kit::ClipboardItem::new_string_with_json_metadata(
                            text,
                            serde_json::json!({ "request_eagle_vim_linewise": linewise }),
                        )
                    } else {
                        gpui_kit::ClipboardItem::new_string(text)
                    };
                    cx.write_to_clipboard(item);
                });
                cx.simulate_keystrokes(&format!("j V {paste}"));
                assert_eq!(value(&view, cx), expected.replace('\n', ending));
                assert_eq!(cursor(&view, cx), 3 + ending.len());
            }
            let (view, cx) = setup(cx, "", true);
            cx.update(|_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(format!("x{ending}")))
            });
            cx.simulate_keystrokes(paste);
            assert_eq!(value(&view, cx), "x");
        }
    }
}

#[gpui_kit::test]
fn native_caret_changes_stay_on_graphemes_in_normal_mode(cx: &mut TestAppContext) {
    for (text, end, expected) in [
        ("abc", 3, 2),
        ("abé", 4, 2),
        ("ae\u{301}", 4, 1),
        ("abc\n", 4, 4),
    ] {
        let (view, cx) = setup(cx, text, true);
        cx.update(|_, cx| {
            view.read(cx)
                .editor
                .clone()
                .update(cx, |editor, cx| editor.set_selected_range(end..end, cx))
        });
        assert_eq!(cursor(&view, cx), expected);
        cx.simulate_keystrokes("i");
        cx.update(|_, cx| {
            view.read(cx)
                .editor
                .clone()
                .update(cx, |editor, cx| editor.set_selected_range(end..end, cx))
        });
        assert_eq!(cursor(&view, cx), end);
        assert_eq!(value(&view, cx), text);
    }
}

#[gpui_kit::test]
fn native_copy_and_search_chords_keep_their_prefixes(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, "abcd", true);
    cx.update(|_, cx| {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("g c", gpui_kit::component::input::Copy, Some("Input")),
            gpui_kit::KeyBinding::new("g s", gpui_kit::component::input::Search, Some("Input")),
        ]);
        view.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.set_selected_range(0..2, cx));
    });
    cx.simulate_keystrokes("g c");
    cx.read(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "ab"));
    cx.simulate_keystrokes("g s");
    cx.simulate_input("cd");
    cx.read(|cx| {
        let search = view.read(cx).editor.read(cx).search_session();
        assert!(search.open);
        assert_eq!(search.query, "cd");
    });
    assert_eq!(value(&view, cx), "abcd");
}

#[gpui_kit::test]
fn application_search_highlights_preserve_vim_selection(cx: &mut TestAppContext) {
    for binding in ["secondary-f", "z", "g s"] {
        for (before, selection, expected) in [
            ("l", 1..1, "oe two one\nlast one"),
            ("l v l", 1..3, "o two one\nlast one"),
            ("V", 0..12, "last one"),
        ] {
            let (view, cx) = setup(cx, "one two one\nlast one", true);
            cx.update(|_, cx| {
                cx.bind_keys([
                    gpui_kit::KeyBinding::new(
                        "z",
                        gpui_kit::component::input::Search,
                        Some("Input"),
                    ),
                    gpui_kit::KeyBinding::new(
                        "g s",
                        gpui_kit::component::input::Search,
                        Some("Input"),
                    ),
                ]);
            });
            cx.simulate_keystrokes(&format!("{before} {binding}"));
            cx.simulate_input("one");
            // Native search navigates painted highlights, not the editor's
            // selection. Enter keeps the panel open; Escape returns to editing.
            for navigation in ["enter", "enter", "shift-enter"] {
                cx.simulate_keystrokes(navigation);
                cx.read(|cx| {
                    let editor = view.read(cx).editor.read(cx);
                    assert!(editor.search_session().open);
                    assert_eq!(editor.search_session().query, "one");
                    assert_eq!(editor.search_session().matcher.matched_ranges().len(), 3);
                    assert_eq!(
                        editor.selected_range(),
                        selection,
                        "{before} {binding} {navigation}"
                    );
                    assert_eq!(view.read(cx).vim.read(cx).is_normal(), before == "l");
                });
            }
            cx.simulate_keystrokes("escape");
            cx.read(|cx| {
                assert_eq!(view.read(cx).editor.read(cx).selected_range(), selection);
                assert!(!view.read(cx).editor.read(cx).search_session().open);
            });
            cx.simulate_keystrokes("x");
            assert_eq!(value(&view, cx), expected, "{before} {binding}");
        }
    }
}

#[gpui_kit::test]
fn operators_apply_prompted_and_repeated_search_motions(cx: &mut TestAppContext) {
    for (keys, expected, copied, inserting) in [
        ("d", "two one two end", "one ", false),
        ("2 d", "two end", "one two one ", false),
        ("d 2", "two end", "one two one ", false),
        ("y", "one two one two end", "one ", false),
        ("c", "two one two end", "one ", true),
    ] {
        for prompted in [false, true] {
            let (view, cx) = setup(cx, "one two one two end", true);
            if prompted {
                cx.simulate_keystrokes(&format!("{keys} /"));
                cx.simulate_input("two");
                cx.simulate_keystrokes("enter");
            } else {
                cx.simulate_keystrokes("/");
                cx.simulate_input("two");
                cx.simulate_keystrokes(&format!("escape {keys} n"));
            }
            assert_eq!(value(&view, cx), expected, "{keys}, prompted={prompted}");
            cx.read(|cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), copied);
                assert_eq!(!view.read(cx).vim.read(cx).is_normal(), inserting);
            });
            if inserting {
                cx.simulate_input("new ");
                assert_eq!(value(&view, cx), format!("new {expected}"));
            }
        }
    }
}

#[gpui_kit::test]
fn cancelled_or_unmatched_search_operators_leave_text_and_register_unchanged(
    cx: &mut TestAppContext,
) {
    for (query, completion) in [
        ("two", "escape"),
        ("two", "ctrl-["),
        ("absent", "enter"),
        ("two", "close"),
    ] {
        let (view, cx) = setup(cx, "one two one", true);
        cx.update(|_, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("kept".into()))
        });
        cx.simulate_keystrokes("l d /");
        cx.simulate_input(query);
        if completion == "close" {
            cx.update(|_, cx| {
                view.read(cx)
                    .editor
                    .clone()
                    .update(cx, |editor, cx| editor.close_search(cx))
            });
        } else {
            cx.simulate_keystrokes(completion);
        }
        assert_eq!(value(&view, cx), "one two one");
        assert_eq!(cursor(&view, cx), 1);
        cx.read(|cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "kept"));
    }
}

#[gpui_kit::test]
fn search_operators_follow_vim_exclusive_line_boundaries_in_both_directions(
    cx: &mut TestAppContext,
) {
    // Compared with Vim 9.1 using an unconfigured instance and the same n/N motions.
    for ending in ["\n", "\r\n"] {
        for (source, position, query, keys, expected, copied, linewise) in [
            (
                "  one\ntwo\nthree",
                "2 l",
                "two",
                "d n",
                "two\nthree",
                "  one\n",
                true,
            ),
            (
                "  one\ntwo\nthree",
                "3 l",
                "two",
                "d n",
                "  o\ntwo\nthree",
                "ne",
                false,
            ),
            (
                "one\ntwo\nthree",
                "j 0",
                "one",
                "d N",
                "two\nthree",
                "one\n",
                true,
            ),
            (
                "one\ntwo\nthree",
                "j 0",
                "ne",
                "d N",
                "o\ntwo\nthree",
                "ne",
                false,
            ),
            (
                "  one\ntwo\nthree",
                "j 0",
                "one",
                "d N",
                "two\nthree",
                "  one\n",
                true,
            ),
            (
                "one\ntwo\nthree",
                "j l",
                "one",
                "d N",
                "wo\nthree",
                "one\nt",
                false,
            ),
        ] {
            let (view, cx) = setup(cx, &source.replace('\n', ending), true);
            cx.simulate_keystrokes("/");
            cx.simulate_input(query);
            cx.simulate_keystrokes(&format!("escape {position} {keys}"));
            assert_eq!(
                value(&view, cx),
                expected.replace('\n', ending),
                "{position} {keys}"
            );
            cx.read(|cx| {
                let clipboard = cx.read_from_clipboard().unwrap();
                assert_eq!(clipboard.text().unwrap(), copied.replace('\n', ending));
                let gpui_kit::ClipboardEntry::String(entry) = &clipboard.entries()[0] else { panic!("expected text") };
                assert_eq!(entry.metadata_json::<serde_json::Value>().unwrap()["request_eagle_vim_linewise"], linewise);
            });
        }
    }
}

#[gpui_kit::test]
fn silent_document_replacement_invalidates_vim_positions_and_pending_commands(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(cx, "\nabcdefghijk\nabcdef", true);
    cx.simulate_keystrokes("2 j 4 l 2 k");
    assert_eq!(cursor(&view, cx), 0);
    cx.update(|window, cx| {
        view.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.set_value("ab\ncd", window, cx))
    });
    cx.simulate_keystrokes("j");
    assert_eq!(cursor(&view, cx), 3);
    cx.simulate_keystrokes("g g d");
    cx.update(|window, cx| {
        view.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.set_value("xy\nzw", window, cx))
    });
    cx.simulate_keystrokes("l");
    assert_eq!(value(&view, cx), "xy\nzw");
    assert_eq!(cursor(&view, cx), 1);
}

#[gpui_kit::test]
fn replacements_reconcile_unchanged_normal_and_visual_selections(cx: &mut TestAppContext) {
    for silent in [false, true] {
        for visual in [false, true] {
            let (view, cx) = setup(cx, "abcd", true);
            cx.simulate_keystrokes(if visual { "v l" } else { "2 l" });
            cx.update(|window, cx| {
                view.read(cx).editor.clone().update(cx, |editor, cx| {
                    if silent {
                        editor.set_value("xy", window, cx);
                    } else {
                        editor.set_selected_range(0..4, cx);
                        editor.replace("xy".to_owned(), window, cx);
                    }
                    editor.set_selected_range(if visual { 0..2 } else { 2..2 }, cx);
                });
            });
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert_eq!(view.read(cx).editor.read(cx).selected_range(), 1..1);
                assert!(super::cursor::layout(&view.read(cx).vim, window, cx).is_some());
            });
            assert_eq!(value(&view, cx), "xy");
        }
    }
}

#[gpui_kit::test]
fn linewise_delete_change_and_forward_yank_do_not_shape_long_columns(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx, "small visible editor", true);
    let length = 2 * 1024 * 1024;
    let source = format!("{}\n{}\nlast", "a".repeat(length), "b".repeat(length));
    for operator in ["d", "c", "y"] {
        cx.update(|window, cx| {
            // Exercise the real command without laying out a multi-megabyte
            // native editor. Clipboard/edit allocations are included; measuring
            // a display column would add hundreds of megabytes of glyph data.
            let editor = cx.new(|cx| EditorState::new(window, cx).default_value(source.clone()));
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(length - 2..length - 2, cx);
                editor.focus(window, cx);
            });
            let vim = cx.new(|cx| Vim::new(editor.clone(), cx));
            let allocated = crate::test_allocator::allocated_by(|| {
                for key in [operator, "j"] {
                    let event = gpui_kit::KeystrokeEvent {
                        keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
                        action: None,
                        context_stack: Vec::new(),
                    };
                    vim.update(cx, |vim, cx| vim.dispatch(&event, window, cx));
                }
            });
            eprintln!("{operator}j allocated {allocated} bytes");
            assert!(
                allocated < 64 * 1024 * 1024,
                "{operator}j allocated {allocated} bytes"
            );
            let expected_len = match operator {
                "d" => 4,
                "c" => 5,
                _ => source.len(),
            };
            assert_eq!(editor.read(cx).text().len(), expected_len);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap().len(),
                2 * (length + 1)
            );
        });
    }
}
