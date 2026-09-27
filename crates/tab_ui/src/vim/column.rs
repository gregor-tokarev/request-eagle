use gpui_kit::{
    component::{ActiveTheme as _, input::Rope},
    *,
};

use super::motions::{line, normal_cursor};

#[derive(Clone, Copy)]
pub(super) enum Column {
    Display(Pixels),
    End,
}

impl Column {
    pub(super) fn at(text: &Rope, cursor: usize, window: &Window, cx: &App) -> Self {
        let range = line(text, cursor);
        let shaped = shape(text.slice(range.clone()).to_string(), window, cx);
        Self::Display(shaped.x_for_index(cursor - range.start))
    }

    pub(super) fn offset(self, text: &Rope, cursor: usize, window: &Window, cx: &App) -> usize {
        let range = line(text, cursor);
        let offset = match self {
            Self::End => range.end,
            Self::Display(x) => {
                range.start
                    + shape(text.slice(range.clone()).to_string(), window, cx)
                        .closest_index_for_x(x)
            }
        };
        normal_cursor(text, offset)
    }
}

fn shape(text: String, window: &Window, cx: &App) -> ShapedLine {
    // Match the body/script editors' font and text_sm size. Measuring the actual
    // layout also respects platform tab expansion and wide/combined glyphs.
    let run = TextRun {
        len: text.len(),
        font: font(cx.theme().mono_font_family.clone()),
        ..Default::default()
    };
    window.text_system().shape_line(
        text.into(),
        rems(0.875).to_pixels(window.rem_size()),
        &[run],
        None,
    )
}
