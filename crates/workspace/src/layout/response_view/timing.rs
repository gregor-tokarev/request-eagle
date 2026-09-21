use std::time::Duration;

use gpui_kit::{base::SelectableText, component::*, prelude::FluentBuilder as _, *};
use request::HttpMetrics;

const LABEL_WIDTH: Pixels = px(96.);
const VALUE_WIDTH: Pixels = px(72.);
const COLUMN_GAP: Pixels = px(8.);
const ROW_HEIGHT: Pixels = px(25.);

pub(super) fn timing_details(
    metrics: HttpMetrics,
    processing: Duration,
    elapsed: Duration,
    cx: &App,
) -> Div {
    // (Label, start time, duration, color), all relative to request dispatch.
    let phases = [
        (
            "Prepare",
            Duration::ZERO,
            metrics.prepare,
            cx.theme().muted_foreground,
        ),
        (
            "Wait for headers",
            metrics.prepare,
            metrics.waiting,
            cx.theme().danger,
        ),
        (
            "Download",
            metrics.prepare + metrics.waiting,
            metrics.download,
            cx.theme().success,
        ),
        // Formatting starts after execution completes, including its small
        // uninstrumented overhead, rather than directly after the last phase.
        (
            "Format body",
            elapsed.saturating_sub(processing),
            processing,
            cx.theme().muted_foreground,
        ),
    ];
    let total = elapsed.as_secs_f64().max(f64::EPSILON);

    v_flex()
        .gap(px(8.))
        .child(
            h_flex()
                .debug_selector(|| "detail-time-title".into())
                .justify_between()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SelectableText::new("time-title", "Response time"))
                .child(SelectableText::new("time-total", duration_label(elapsed)).document_order(1)),
        )
        .child(
            v_flex()
                .relative()
                // The shared grid keeps horizontal position tied to elapsed time.
                .child(
                    div()
                        .debug_selector(|| "timing-plot".into())
                        .absolute()
                        .left(LABEL_WIDTH + COLUMN_GAP)
                        .right(VALUE_WIDTH + COLUMN_GAP)
                        .h(ROW_HEIGHT * phases.len())
                        .border_x_1()
                        .border_color(cx.theme().border)
                        .children([0.25, 0.5, 0.75].map(|fraction| {
                            div()
                                .absolute()
                                .left(relative(fraction))
                                .h_full()
                                .w(px(1.))
                                .bg(cx.theme().border.opacity(0.45))
                        })),
                )
                .children(phases.into_iter().enumerate().map(|(index, (label, start, duration, color))| {
                    let start = (start.as_secs_f64() / total).clamp(0., 1.) as f32;
                    let width = (duration.as_secs_f64() / total).clamp(0., (1. - start) as f64) as f32;
                    let opacity = match index {
                        1 => 0.18,
                        2 => 0.85,
                        _ => 0.3,
                    };

                    h_flex()
                        .debug_selector(move || format!("timing-phase-{index}"))
                        .h(ROW_HEIGHT)
                        .gap(COLUMN_GAP)
                        .child(div()
                            .w(LABEL_WIDTH)
                            .flex_none()
                            .cursor_text()
                            .text_color(cx.theme().muted_foreground)
                            .child(SelectableText::new(("phase-label", index), label).document_order((index * 2 + 2) as u64)))
                        .child(div()
                            .relative()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .when(!duration.is_zero(), |plot| plot.child(
                                div()
                                    .debug_selector(move || format!("timing-bar-{index}"))
                                    .absolute()
                                    .left(relative(start))
                                    .w(relative(width))
                                    .min_w(px(1.))
                                    .h_full()
                                    .bg(color.opacity(opacity))
                                    .when(index == 1, |bar| bar.border_1().border_dashed().border_color(color)),
                            )))
                        .child(div()
                            .w(VALUE_WIDTH)
                            .flex_none()
                            .text_align(TextAlign::Right)
                            .cursor_text()
                            .child(SelectableText::new(("phase-duration", index), duration_label(duration)).document_order((index * 2 + 3) as u64)))
                }))
                .child(
                    h_flex()
                        .ml(LABEL_WIDTH + COLUMN_GAP)
                        .mr(VALUE_WIDTH + COLUMN_GAP)
                        .pt(px(6.))
                        .justify_between()
                        .text_size(px(10.))
                        .text_color(cx.theme().muted_foreground)
                        .children([Duration::ZERO, elapsed / 2, elapsed].into_iter().enumerate().map(|(index, time)| {
                            SelectableText::new(("timing-axis", index), duration_label(time)).document_order(10 + index as u64)
                        })),
                ),
        )
        .child(div().text_size(px(10.)).text_color(cx.theme().muted_foreground)
            .child(SelectableText::new("timing-note", "Waiting includes connection setup, upload, and the server response. Separate DNS, TCP, TLS and first-byte timings are not available.").document_order(13)))
}

pub(super) fn duration_label(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.)
}
