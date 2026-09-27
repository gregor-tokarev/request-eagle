use gpui_kit::base::SelectableText;
use gpui_kit::component::{scroll::ScrollableElement as _, *};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::ResponseView;

impl ResponseView {
    pub(super) fn script_results(&self, console: bool, cx: &App) -> AnyElement {
        let tests = self
            .scripts
            .iter()
            .flat_map(|report| &report.tests)
            .collect::<Vec<_>>();
        let passed = tests.iter().filter(|test| test.error.is_none()).count();
        let errors = self
            .scripts
            .iter()
            .filter(|report| report.error.is_some())
            .count();
        let summary = if console {
            "Script console · latest run".to_owned()
        } else if tests.is_empty() && errors == 0 {
            "No tests yet. Add pm.test() in Scripts, then send the request.".to_owned()
        } else {
            format!(
                "{} passed · {} failed · {} script errors",
                passed,
                tests.len() - passed,
                errors
            )
        };
        let mut rows = Vec::new();

        for (phase_index, report) in self.scripts.iter().enumerate() {
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
                                .child(format!("{} script error", report.phase.label())),
                        )
                        .child(SelectableText::new(
                            ("script-error", phase_index),
                            error.clone(),
                        ))
                        .into_any_element(),
                );
            }

            if console {
                for (index, log) in report.logs.iter().enumerate() {
                    rows.push(
                        v_flex()
                            .gap_1()
                            .py_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} · {}", report.phase.label(), log.level)),
                            )
                            .child(SelectableText::new(
                                ("script-log", phase_index * 500 + index),
                                log.message.clone(),
                            ))
                            .into_any_element(),
                    );
                }
            } else {
                for (index, test) in report.tests.iter().enumerate() {
                    rows.push(
                        v_flex()
                            .gap_1()
                            .py_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(if test.error.is_some() {
                                                cx.theme().danger
                                            } else {
                                                cx.theme().success
                                            })
                                            .child(if test.error.is_some() {
                                                "FAIL"
                                            } else {
                                                "PASS"
                                            }),
                                    )
                                    .child(SelectableText::new(
                                        ("script-test", phase_index * 500 + index),
                                        test.name.clone(),
                                    ))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(report.phase.label()),
                                    ),
                            )
                            .when_some(test.error.clone(), |row, error| {
                                row.child(SelectableText::new(
                                    ("script-test-error", phase_index * 500 + index),
                                    error,
                                ))
                            })
                            .into_any_element(),
                    );
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
            .child(
                div()
                    .py_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
            .when(console && rows.is_empty(), |view| {
                view.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Use console.log() in your scripts to see output here."),
                )
            })
            .children(rows)
            .into_any_element()
    }
}
