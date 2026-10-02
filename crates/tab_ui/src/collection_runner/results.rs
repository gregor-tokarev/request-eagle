use std::time::Duration;

use gpui_kit::base::{SelectableText, Tab, Tabs};
use gpui_kit::component::{button::*, scroll::Scrollbar, tag::Tag, tooltip::Tooltip, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;

use super::run::{Outcome, RunRequest, RunResult, Totals};
use super::runner::{CollectionRunner, Run, RunStatus};
use crate::response_view::status_color;

/// Which results the list shows, as Postman's result tabs choose them.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum ResultFilter {
    #[default]
    All,
    Passed,
    Failed,
    Skipped,
    Errors,
    Console,
}

impl ResultFilter {
    fn shows(self, result: &RunResult) -> bool {
        match self {
            Self::All => true,
            Self::Passed => result.tests().any(|test| test.error.is_none()),
            Self::Failed => result.tests().any(|test| test.error.is_some()),
            Self::Skipped => matches!(result.outcome, Outcome::Skipped(_)),
            Self::Errors => result.is_error(),
            Self::Console => false,
        }
    }
}

#[derive(Clone, Copy)]
enum ResultRow {
    Iteration(usize),
    Result(usize),
    Log {
        result: usize,
        report: usize,
        log: usize,
    },
    Note(usize),
}

/// The rows of the results list for the chosen filter.
pub(super) struct ResultsState {
    pub filter: ResultFilter,
    rows: Vec<ResultRow>,
    /// The iteration of the last result row, which starts a heading when it
    /// changes.
    iteration: Option<usize>,
    list: ListState,
    pub selected: Option<usize>,
}

impl Default for ResultsState {
    fn default() -> Self {
        Self {
            filter: ResultFilter::All,
            rows: Vec::new(),
            iteration: None,
            list: ListState::new(0, ListAlignment::Top, px(0.)),
            selected: None,
        }
    }
}

impl ResultsState {
    /// Add the rows of the run's result at `index`.
    fn add(&mut self, run: &Run, index: usize) {
        let result = &run.results[index];

        if self.filter == ResultFilter::Console {
            for (report_index, report) in result.scripts.iter().enumerate() {
                self.rows
                    .extend((0..report.logs.len()).map(|log| ResultRow::Log {
                        result: index,
                        report: report_index,
                        log,
                    }));
            }
            self.rows.extend(
                run.notes
                    .iter()
                    .enumerate()
                    .filter(|(_, (after, _))| *after == index)
                    .map(|(note, _)| ResultRow::Note(note)),
            );
            return;
        }

        if !self.filter.shows(result) {
            return;
        }

        if self.iteration != Some(result.position.iteration) {
            self.iteration = Some(result.position.iteration);
            self.rows
                .push(ResultRow::Iteration(result.position.iteration));
        }
        self.rows.push(ResultRow::Result(index));
    }
}

/// A run's duration as Postman writes it, such as `1s 458ms`.
fn duration_label(duration: Duration) -> String {
    let milliseconds = duration.as_millis();

    if milliseconds < 1000 {
        format!("{milliseconds}ms")
    } else {
        format!("{}s {}ms", milliseconds / 1000, milliseconds % 1000)
    }
}

/// A short label in the color of what it reports, such as PASS or FAIL.
fn badge(text: impl Into<SharedString>, color: Hsla, cx: &App) -> Div {
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

/// The request's folders, each followed by a separator, then its name.
pub(super) fn request_title(request: &RunRequest, cx: &App) -> Div {
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .overflow_hidden()
        .children(request.folders.iter().map(|folder| {
            h_flex()
                .flex_none()
                .gap_1()
                .text_color(cx.theme().muted_foreground)
                .child(folder.clone())
                .child(Icon::new(IconName::ChevronRight).size_3())
        }))
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(request.name.clone()),
        )
}

impl CollectionRunner {
    /// Show the run's results for the chosen filter from the start.
    pub(super) fn reset_results(&mut self, cx: &mut Context<Self>) {
        let filter = self.results.filter;
        let selected = self.results.selected;
        self.results = ResultsState {
            filter,
            selected,
            ..Default::default()
        };

        if let Some(run) = &self.run {
            for index in 0..run.results.len() {
                self.results.add(run, index);
            }
        }
        self.results.list.reset(self.results.rows.len());
        cx.notify();
    }

    /// Show the result the run just added.
    pub(super) fn result_added(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &self.run else {
            return;
        };
        let Some(index) = run.results.len().checked_sub(1) else {
            return;
        };

        let start = self.results.rows.len();
        self.results.add(run, index);
        let added = self.results.rows.len() - start;
        if added > 0 {
            self.results.list.splice(start..start, added);
        }
        cx.notify();
    }

    fn set_filter(&mut self, filter: ResultFilter, cx: &mut Context<Self>) {
        if self.results.filter != filter {
            self.results.filter = filter;
            self.reset_results(cx);
        }
    }

    pub(super) fn results_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(run) = &self.run else {
            return div().into_any_element();
        };
        let summary = &run.totals;

        v_flex()
            .debug_selector(|| "runner-results".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .px_4()
            .pb_2()
            .gap_3()
            .text_sm()
            .child(self.results_header(run, summary, cx))
            .child(summary_band(run, summary, cx))
            .child(self.filters(summary, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .items_start()
                    .child(self.result_list(run, cx))
                    .when_some(self.results.selected, |content, index| {
                        content.child(self.detail_pane(run, index, cx))
                    }),
            )
            .into_any_element()
    }

    fn results_header(
        &self,
        run: &Run,
        summary: &Totals,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let today = run.started_at.date_naive() == chrono::Local::now().date_naive();
        let ran = if today {
            format!("Ran today at {}", run.started_at.format("%H:%M:%S"))
        } else {
            format!("Ran on {}", run.started_at.format("%B %-d at %H:%M:%S"))
        };
        let status = match run.status {
            RunStatus::Running => Some(Tag::info().child("Running")),
            RunStatus::Paused => Some(Tag::warning().child("Paused")),
            RunStatus::Stopped => Some(Tag::secondary().child("Stopped")),
            RunStatus::Complete if summary.errors > 0 => Some(Tag::danger().child("Error")),
            RunStatus::Complete => None,
        };
        let active = run.is_active();
        let paused = run.status == RunStatus::Paused;

        v_flex()
            .flex_none()
            .gap_1()
            .child(
                h_flex()
                    .h_10()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "run-results-title".into())
                            .min_w_0()
                            .truncate()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} - Run results", self.name)),
                    )
                    .when_some(status, |row, status| {
                        row.child(
                            div()
                                .debug_selector(|| "run-status".into())
                                .flex_none()
                                .child(status.small()),
                        )
                    })
                    .child(div().flex_1())
                    .when(active, |row| {
                        row.child(
                            Button::new("pause-run")
                                .debug_selector(|| "pause-run".into())
                                .ghost()
                                .small()
                                .icon(if paused {
                                    IconName::Play
                                } else {
                                    IconName::Pause
                                })
                                .label(if paused { "Resume" } else { "Pause" })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if paused {
                                        this.resume(window, cx);
                                    } else {
                                        this.pause(cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new("stop-run")
                                .debug_selector(|| "stop-run".into())
                                .ghost()
                                .small()
                                .icon(Icon::default().path("icons/square.svg"))
                                .label("Stop")
                                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                        )
                    })
                    .when(!active, |row| {
                        row.child(
                            Button::new("run-again")
                                .debug_selector(|| "run-again".into())
                                .primary()
                                .small()
                                .icon(IconName::Play)
                                .label("Run Again")
                                .loading(self.preparing.is_some())
                                .disabled(self.preparing.is_some())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.run_again(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("new-run")
                                .debug_selector(|| "new-run".into())
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label("New Run")
                                .tooltip("Change the run configuration")
                                .on_click(cx.listener(|this, _, _, cx| this.new_run(cx))),
                        )
                        .child(
                            Button::new("export-results")
                                .debug_selector(|| "export-results".into())
                                .ghost()
                                .small()
                                .icon(Icon::default().path("icons/download.svg"))
                                .label("Export Results")
                                .tooltip("Save the results as JSON")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.export_results(window, cx)
                                })),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(ran)
                    .when(active, |line| {
                        let iteration = run
                            .results
                            .last()
                            .map_or(0, |result| result.position.iteration);
                        line.child("·").child(format!(
                            "Iteration {} of {}",
                            iteration + 1,
                            run.iterations
                        ))
                    })
                    .when(!run.missing.is_empty(), |line| {
                        line.child("·").child(
                            div()
                                .debug_selector(|| "run-missing".into())
                                .min_w_0()
                                .truncate()
                                .text_color(theme.warning)
                                .child(match run.missing.as_slice() {
                                    [name] => {
                                        format!("“{name}” is no longer saved, so it did not run")
                                    }
                                    names => format!(
                                        "{} requests are no longer saved, so they did not run",
                                        names.len()
                                    ),
                                }),
                        )
                    })
                    .when_some(self.exported.clone(), |line, exported| {
                        line.child("·").child(match exported {
                            Ok(path) => div()
                                .debug_selector(|| "run-exported".into())
                                .min_w_0()
                                .truncate()
                                .child(format!("Results saved to {}", path.display())),
                            Err(error) => div()
                                .debug_selector(|| "run-export-error".into())
                                .text_color(theme.danger)
                                .child(error),
                        })
                    }),
            )
    }

    fn filters(&self, summary: &Totals, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let filters = [
            (ResultFilter::All, "All", Some((summary.tests(), None))),
            (
                ResultFilter::Passed,
                "Passed",
                Some((summary.passed, Some(theme.success))),
            ),
            (
                ResultFilter::Failed,
                "Failed",
                Some((summary.failed, Some(theme.danger))),
            ),
            (
                ResultFilter::Skipped,
                "Skipped",
                Some((summary.skipped, None)),
            ),
            (
                ResultFilter::Errors,
                "Errors",
                Some((summary.errors, Some(theme.danger))),
            ),
            (ResultFilter::Console, "Console log", None),
        ];

        Tabs::new("run-result-filters")
            .flex()
            .flex_none()
            .flex_wrap()
            .gap_1()
            .children(filters.into_iter().map(|(filter, label, count)| {
                let selected = self.results.filter == filter;

                Tab::new(label)
                    .debug_selector(move || format!("run-filter-{label}"))
                    .selected(selected)
                    .accessibility_label(label)
                    .flex_none()
                    .h_8()
                    .px_2()
                    .gap_1()
                    .rounded(theme.radius_tokens().md)
                    .text_color(theme.muted_foreground)
                    .hover(|tab| tab.bg(theme.muted))
                    .when(selected, |tab| {
                        tab.bg(theme.muted).text_color(theme.foreground)
                    })
                    .child(label)
                    .when_some(count, |tab, (count, color)| {
                        tab.child(
                            div()
                                .text_color(match color {
                                    Some(color) if count > 0 => color,
                                    _ => theme.muted_foreground,
                                })
                                .child(count.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
            }))
    }

    fn result_list(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let empty = match self.results.filter {
            _ if run.results.is_empty() && run.is_active() => Some("Sending the first request…"),
            _ if run.results.is_empty() => Some("The run stopped before a request finished."),
            ResultFilter::All => None,
            ResultFilter::Passed => Some("No tests passed"),
            ResultFilter::Failed => Some("No tests failed"),
            ResultFilter::Skipped => Some("No requests were skipped"),
            ResultFilter::Errors => Some("No errors"),
            ResultFilter::Console => Some("Use console.log() in your scripts to see output here."),
        }
        .filter(|_| self.results.rows.is_empty());

        div()
            .debug_selector(|| "run-result-list".into())
            .relative()
            .h_full()
            .min_w_0()
            .map(|pane| {
                if self.results.selected.is_some() {
                    pane.w(relative(0.45)).flex_none()
                } else {
                    pane.flex_1()
                }
            })
            .child(match empty {
                Some(message) => div()
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child(message)
                    .into_any_element(),
                None => list(
                    self.results.list.clone(),
                    cx.processor(|this, index: usize, _, cx| this.result_row(index, cx)),
                )
                .size_full()
                .into_any_element(),
            })
            .when(empty.is_none(), |pane| {
                pane.child(Scrollbar::vertical(&self.results.list))
            })
    }

    fn result_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let (Some(run), Some(&row)) = (&self.run, self.results.rows.get(index)) else {
            return div().into_any_element();
        };
        let theme = cx.theme();

        match row {
            ResultRow::Iteration(iteration) => div()
                .pt_3()
                .pb_1()
                .px_2()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(format!("Iteration {}", iteration + 1))
                .into_any_element(),
            ResultRow::Result(result) => self.result_block(run, result, cx).into_any_element(),
            ResultRow::Log {
                result,
                report,
                log,
            } => {
                let run_result = &run.results[result];
                let report = &run_result.scripts[report];
                let entry = &report.logs[log];
                let request = &run.requests[run_result.position.index];
                let color = match entry.level.as_str() {
                    "error" => theme.danger,
                    "warn" => theme.warning,
                    _ => theme.muted_foreground,
                };

                v_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(badge(entry.level.to_uppercase(), color, cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!(
                                        "Iteration {} · {} · {}",
                                        run_result.position.iteration + 1,
                                        request.name,
                                        report.label()
                                    )),
                            ),
                    )
                    .child(div().font_family(theme.mono_font_family.clone()).child(
                        SelectableText::new(("run-log", index), entry.message.clone()),
                    ))
                    .into_any_element()
            }
            ResultRow::Note(note) => h_flex()
                .w_full()
                .px_2()
                .py_1p5()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .child(badge("RUNNER", theme.warning, cx))
                .child(div().min_w_0().child(SelectableText::new(
                    ("run-note", index),
                    run.notes[note].1.clone(),
                )))
                .into_any_element(),
        }
    }

    fn result_block(
        &self,
        run: &Run,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let result = &run.results[index];
        let request = &run.requests[result.position.index];
        let filter = self.results.filter;
        let selected = self.results.selected == Some(index);
        let passed = result.tests().filter(|test| test.error.is_none()).count();
        let failed = result.tests().filter(|test| test.error.is_some()).count();
        let url = result
            .url
            .clone()
            .unwrap_or_else(|| request.request.path.clone());

        let outcome = match &result.outcome {
            Outcome::Response { status, .. } => h_flex()
                .flex_none()
                .gap_1()
                .child(badge(status.as_u16().to_string(), status_color(*status, cx), cx))
                .when(passed + failed > 0, |row| {
                    row.child(div().text_color(theme.muted_foreground).child("•"))
                        .child(
                            h_flex()
                                .id(("run-result-tests", index))
                                .gap_1()
                                .when(passed > 0, |counts| {
                                    counts.child(badge(passed.to_string(), theme.success, cx))
                                })
                                .when(failed > 0, |counts| {
                                    counts.child(badge(failed.to_string(), theme.danger, cx))
                                })
                                .tooltip(move |window, cx| {
                                    let tests = passed + failed;
                                    let noun = if tests == 1 { "test" } else { "tests" };
                                    Tooltip::new(format!(
                                        "{tests} {noun} in this request: {passed} passed and {failed} failed"
                                    ))
                                    .build(window, cx)
                                }),
                        )
                })
                .into_any_element(),
            Outcome::Failed(_) => div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child("No response")
                .into_any_element(),
            Outcome::Skipped(_) => div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child("Skipped")
                .into_any_element(),
        };

        let mut lines = Vec::new();
        if matches!(
            filter,
            ResultFilter::All | ResultFilter::Errors | ResultFilter::Skipped
        ) {
            match &result.outcome {
                Outcome::Failed(message)
                    if result.scripts.iter().all(|report| report.error.is_none()) =>
                {
                    lines.push(line(
                        badge("REQUEST", theme.danger, cx),
                        message.clone(),
                        None,
                        cx,
                    ));
                }
                Outcome::Skipped(reason) => {
                    lines.push(line(
                        badge("SKIP", theme.muted_foreground, cx),
                        reason.clone(),
                        None,
                        cx,
                    ));
                }
                _ => {}
            }
        }
        if matches!(filter, ResultFilter::All | ResultFilter::Errors) {
            for report in &result.scripts {
                if let Some(error) = &report.error {
                    lines.push(line(
                        badge("SCRIPT", theme.danger, cx),
                        format!("{}: {error}", report.label()),
                        None,
                        cx,
                    ));
                }
            }
        }
        for test in result.tests() {
            let shown = match filter {
                ResultFilter::All => true,
                ResultFilter::Passed => test.error.is_none(),
                ResultFilter::Failed => test.error.is_some(),
                _ => false,
            };
            if shown {
                lines.push(match &test.error {
                    None => line(
                        badge("PASS", theme.success, cx),
                        test.name.clone(),
                        None,
                        cx,
                    ),
                    Some(error) => line(
                        badge("FAIL", theme.danger, cx),
                        test.name.clone(),
                        Some(error.clone()),
                        cx,
                    ),
                });
            }
        }
        let no_tests = lines.is_empty()
            && filter == ResultFilter::All
            && matches!(result.outcome, Outcome::Response { .. });

        v_flex()
            .id(("run-result", index))
            .debug_selector(move || format!("run-result-{index}"))
            .w_full()
            .px_2()
            .py_2()
            .gap_1()
            .rounded(theme.radius_tokens().md)
            .when(selected, |block| block.bg(theme.muted))
            .when(!selected, |block| {
                block.hover(|block| block.bg(theme.muted.opacity(0.5)))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .child(method_label(result.method.as_str(), cx)),
                    )
                    .child(request_title(request, cx)),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(url),
                    )
                    .child(div().text_xs().child(outcome)),
            )
            .when(no_tests, |block| {
                block.child(
                    div()
                        .pt_1()
                        .text_color(theme.muted_foreground)
                        .child("No tests found"),
                )
            })
            .when(!lines.is_empty(), |block| {
                block.child(v_flex().pt_1().gap_1().children(lines))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.select_result(index, window, cx)))
    }
}

/// A test or error under its request: a label, a message and, for a failed
/// test, why it failed. Each shows its first line; the response's Test
/// Results show all of it.
fn line(label: Div, text: impl Into<SharedString>, error: Option<String>, cx: &App) -> AnyElement {
    let first_line = |text: &str| text.lines().next().unwrap_or_default().to_owned();
    let text = first_line(&text.into());
    let error = error.map(|error| first_line(&error));

    h_flex()
        .w_full()
        .gap_2()
        .child(label)
        .child(
            div()
                .min_w_0()
                .when(error.is_some(), |text| {
                    text.flex_none().max_w(relative(0.5))
                })
                .truncate()
                .child(text),
        )
        .when_some(error, |line, error| {
            line.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("| {error}")),
            )
        })
        .into_any_element()
}

fn summary_band(run: &Run, summary: &Totals, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let values = [
        ("Source", "Runner".to_owned(), None),
        ("Environment", run.environment.to_string(), None),
        ("Iterations", run.iterations.to_string(), None),
        ("Duration", duration_label(run.duration()), None),
        ("All tests", summary.tests().to_string(), None),
        (
            "Errors",
            summary.errors.to_string(),
            (summary.errors > 0).then_some(theme.danger),
        ),
        (
            "Avg. Resp. Time",
            summary.average().map_or_else(
                || "–".to_owned(),
                |average| format!("{} ms", average.as_millis()),
            ),
            None,
        ),
    ];

    h_flex()
        .debug_selector(|| "run-summary".into())
        .flex_none()
        .flex_wrap()
        .px_4()
        .py_3()
        .gap_x_8()
        .gap_y_3()
        .rounded(theme.radius_tokens().lg)
        .bg(theme.secondary)
        .children(values.into_iter().map(|(label, value, color)| {
            v_flex()
                .gap_1()
                .child(div().text_color(theme.muted_foreground).child(label))
                .child(
                    div()
                        .debug_selector(move || format!("run-summary-{label}"))
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .when_some(color, |value, color| value.text_color(color))
                        .child(value),
                )
        }))
}
