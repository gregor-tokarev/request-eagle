use gpui_kit::base::SelectableText;
use gpui_kit::component::{scroll::ScrollableElement as _, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ScriptReport;

/// A short label in the color of what it reports, such as PASS or FAIL.
pub(crate) fn badge(text: impl Into<SharedString>, color: Hsla, cx: &App) -> Div {
    div()
        .flex_none()
        .px_1()
        .rounded(cx.theme().radius_tokens().sm)
        .bg(color.opacity(0.15))
        .text_color(color)
        .text_xs()
        .font_family(cx.theme().mono_font_family.clone())
        .child(text.into())
}

/// A console line: its level, then the message as the script logged it.
/// Levels take the same width, so messages start at the same place.
pub(crate) fn log_line(
    id: impl Into<ElementId>,
    level: &str,
    message: impl Into<SharedString>,
    cx: &App,
) -> Div {
    let color = match level {
        "error" => cx.theme().danger,
        "warn" => cx.theme().warning,
        _ => cx.theme().muted_foreground,
    };

    h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .child(
            badge(level.to_uppercase(), color, cx)
                .w(rems(3.))
                .flex()
                .justify_center(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(cx.theme().mono_font_family.clone())
                .child(SelectableText::new(id, message.into())),
        )
}

/// A test and why it failed, under the label of the script that ran it.
fn test_row(id: usize, name: &str, error: Option<&str>, phase: String, cx: &App) -> AnyElement {
    let (label, color) = match error {
        None => ("PASS", cx.theme().success),
        Some(_) => ("FAIL", cx.theme().danger),
    };

    h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(badge(label, color, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(SelectableText::new(("script-test", id), name.to_owned()))
                .when_some(error, |test, error| {
                    test.child(div().text_color(cx.theme().muted_foreground).child(
                        SelectableText::new(("script-test-error", id), error.to_owned()),
                    ))
                }),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(phase),
        )
        .into_any_element()
}

/// The tests, or with `console` the logs, of a run's scripts. `action` is
/// what runs the request again, such as "send the request".
pub(crate) fn script_results(
    scripts: &[ScriptReport],
    console: bool,
    action: &str,
    cx: &App,
) -> AnyElement {
    let tests = scripts
        .iter()
        .flat_map(|report| &report.tests)
        .collect::<Vec<_>>();
    let passed = tests.iter().filter(|test| test.error.is_none()).count();
    let errors = scripts
        .iter()
        .filter(|report| report.error.is_some())
        .count();
    // The Console tab counts its lines, so only the tests need a summary.
    let summary = if console {
        None
    } else if tests.is_empty() && errors == 0 {
        Some(format!(
            "No tests yet. Add pm.test() in Scripts, then {action}."
        ))
    } else {
        Some(format!(
            "{} passed · {} failed · {} script {}",
            passed,
            tests.len() - passed,
            errors,
            if errors == 1 { "error" } else { "errors" }
        ))
    };
    let mut rows = Vec::new();

    for (phase_index, report) in scripts.iter().enumerate() {
        if let Some(error) = &report.error {
            rows.push(
                v_flex()
                    .gap_1()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_color(cx.theme().danger)
                            .child(format!("{} script error", report.label())),
                    )
                    .child(SelectableText::new(
                        ("script-error", phase_index),
                        error.clone(),
                    ))
                    .into_any_element(),
            );
        }

        if console {
            if !report.logs.is_empty() {
                rows.push(
                    div()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(report.label())
                        .into_any_element(),
                );
            }
            for (index, log) in report.logs.iter().enumerate() {
                rows.push(
                    log_line(
                        ("script-log", phase_index * 500 + index),
                        &log.level,
                        log.message.clone(),
                        cx,
                    )
                    .py_1()
                    .into_any_element(),
                );
            }
        } else {
            for (index, test) in report.tests.iter().enumerate() {
                rows.push(test_row(
                    phase_index * 500 + index,
                    &test.name,
                    test.error.as_deref(),
                    report.label(),
                    cx,
                ));
            }
        }
    }

    v_flex()
        .id("script-results-scroll")
        .debug_selector(move || {
            if console {
                "script-console".into()
            } else {
                "script-test-results".into()
            }
        })
        .flex_1()
        .min_h_0()
        .min_w_0()
        .overflow_y_scrollbar()
        .px_2()
        .pb_3()
        .when_some(summary, |view, summary| {
            view.child(
                div()
                    .py_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
        })
        .when(console && rows.is_empty(), |view| {
            view.child(
                div()
                    .py_2()
                    .text_color(cx.theme().muted_foreground)
                    .child("Use console.log() in your scripts to see output here."),
            )
        })
        .children(rows)
        .into_any_element()
}
