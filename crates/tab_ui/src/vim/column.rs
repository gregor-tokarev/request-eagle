use gpui_kit::{
    component::{ActiveTheme as _, input::Rope},
    *,
};

use super::motions::{line, next, normal_cursor};

#[derive(Clone, Copy)]
pub(super) enum Column {
    Display(Pixels),
    End,
}

impl Column {
    pub(super) fn at(text: &Rope, cursor: usize, window: &Window, cx: &App) -> Self {
        let range = line(text, cursor);

        if cursor == range.start {
            return Self::Display(px(0.));
        }

        Self::Display(shape(text.slice(range.start..cursor).to_string(), window, cx).width)
    }

    pub(super) fn offset(self, text: &Rope, cursor: usize, window: &Window, cx: &App) -> usize {
        let range = line(text, cursor);
        let offset = match self {
            Self::End => range.end,
            Self::Display(x) => {
                if x <= px(0.) {
                    return range.start;
                }

                // Grow only until the requested display position is covered.
                // A minified target line can be megabytes longer than this prefix.
                let mut length = 64;

                loop {
                    let mut end = range.start.saturating_add(length).min(range.end);

                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }

                    let end = next(text, end).min(range.end);
                    let shaped = shape(text.slice(range.start..end).to_string(), window, cx);

                    if shaped.width >= x || end == range.end {
                        break range.start + shaped.closest_index_for_x(x);
                    }

                    length = length.saturating_mul(2);
                }
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
