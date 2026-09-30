use gpui_kit::component::{ActiveTheme as _, input::RopeExt as _};
use gpui_kit::*;

use super::{Vim, motions::next};

/// Paint after the editor so the block uses this frame's text layout,
/// including wrapping, the line-number gutter, and scrolling.
pub(crate) fn cursor(vim: &Entity<Vim>) -> impl IntoElement {
    let vim = vim.clone();

    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let Some(cursor) = layout(&vim, window, cx) else {
                return;
            };

            window.with_content_mask(
                Some(ContentMask {
                    bounds: cursor.clip,
                }),
                |window| {
                    window.paint_quad(fill(cursor.bounds, cx.theme().foreground));
                    let _ = cursor.glyph.paint(
                        cursor.bounds.origin,
                        cursor.bounds.size.height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                },
            );
        },
    )
    .absolute()
    .size_full()
}

pub(super) struct BlockCursor {
    pub(super) bounds: Bounds<Pixels>,
    pub(super) clip: Bounds<Pixels>,
    glyph: ShapedLine,
}

pub(super) fn layout(vim: &Entity<Vim>, window: &Window, cx: &App) -> Option<BlockCursor> {
    let (editor, offset) = vim.read(cx).block_cursor(cx)?;
    let editor = editor.read(cx);

    if !editor.focus_handle(cx).is_focused(window) {
        return None;
    }

    // A Visual selection keeps the editor's caret at one of its edges, so the
    // block is placed on the Vim cursor's character rather than on the caret.
    let character_bounds = editor.range_to_bounds(&(offset..offset))?;
    let text = editor.text();
    let character = match text.char_at(offset) {
        None | Some('\n' | '\r' | '\t') => " ".to_owned(),
        _ => text.slice(offset..next(text, offset)).to_string(),
    };
    let background = cx
        .theme()
        .highlight_theme
        .style
        .editor_background
        .unwrap_or_else(|| cx.theme().input_background());
    let run = TextRun {
        len: character.len(),
        font: font(cx.theme().mono_font_family.clone()),
        color: background,
        ..Default::default()
    };
    // Request body and script editors both use text_sm.
    let glyph = window.text_system().shape_line(
        character.into(),
        rems(0.875).to_pixels(window.rem_size()),
        &[run],
        None,
    );
    let bounds = Bounds::new(
        character_bounds.origin,
        size(glyph.width, character_bounds.size.height),
    );

    Some(BlockCursor {
        bounds,
        clip: editor.input_bounds(),
        glyph,
    })
}
