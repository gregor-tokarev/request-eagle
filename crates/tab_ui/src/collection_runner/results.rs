use std::sync::Arc;

use gpui_kit::base::{SelectableText, Tab, Tabs};
use gpui_kit::component::{
    alert::Alert, button::*, progress::Progress, scroll::Scrollbar, tag::Tag, tooltip::Tooltip, *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;

use super::detail::empty_state;
use super::run::{Outcome, RunRequest, RunResult, count_label, duration_label, failure_summary};
use super::runner::{CollectionRunner, Run, RunStatus};
use super::setup::wrapped_tooltip;
use crate::response_view::{badge, log_line, status_color};

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
    /// The request and script whose console lines follow.
    LogSource {
        result: usize,
        report: usize,
    },
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
            // Rows are measured as they render. Rendering below the visible
            // rows measures the next ones, so the list can scroll to its end.
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            selected: None,
        }
    }
}

impl ResultsState {
    /// Add the rows of the run's result at `index`.
    fn add(&mut self, run: &Run, index: usize) {
        let result = &run.results[index];
        // Why an iteration ended early follows the result that ended it.
        let notes = run
            .notes
            .iter()
            .enumerate()
            .filter(|(_, (after, _))| *after == index)
            .map(|(note, _)| ResultRow::Note(note));

        if self.filter == ResultFilter::Console {
            for (report_index, report) in result.scripts.iter().enumerate() {
                if report.logs.is_empty() {
                    continue;
                }

                self.rows.push(ResultRow::LogSource {
                    result: index,
                    report: report_index,
                });
                self.rows
                    .extend((0..report.logs.len()).map(|log| ResultRow::Log {
                        result: index,
                        report: report_index,
                        log,
                    }));
            }
            self.rows.extend(notes);
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
        if self.filter == ResultFilter::All {
            self.rows.extend(notes);
        }
    }
}

/// The request's name after its folders below the run's own `depth`.
/// A deep path keeps its first and last folder, and the folders give way
/// to the name when the row is narrow. The whole path is in a tooltip.
pub(super) fn request_title(
    id: impl Into<ElementId>,
    request: &RunRequest,
    depth: usize,
    cx: &App,
) -> Stateful<Div> {
    let folders = request.folders.get(depth..).unwrap_or_default();
    let shown = match folders {
        [first, .., last] if folders.len() > 2 => vec![first.clone(), "…".into(), last.clone()],
        folders => folders.to_vec(),
    };
    let path = folders
        .iter()
        .chain([&request.name])
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" › ");
    let long = folders.len() > 2 || path.chars().count() > 48;
    let path = SharedString::from(path);

    // Each folder keeps its space after it, so folders that give way
    // leave none behind.
    h_flex()
        .id(id)
        .min_w_0()
        .overflow_hidden()
        .when(!shown.is_empty(), |title| {
            title.child(
                h_flex()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(cx.theme().muted_foreground)
                    .children(shown.into_iter().map(|folder| {
                        h_flex()
                            .min_w_0()
                            .gap_1()
                            .child(div().min_w_0().truncate().child(folder))
                            .child(
                                Icon::new(IconName::ChevronRight)
                                    .size_3()
                                    .flex_none()
                                    .mr_1(),
                            )
                    })),
            )
        })
        .child(
            div()
                .flex_shrink_0()
                .min_w_0()
                .max_w_full()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(request.name.clone()),
        )
        .when(long, |title| {
            title.tooltip(move |window, cx| wrapped_tooltip(path.clone(), window, cx))
        })
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

        v_flex()
            .debug_selector(|| "runner-results".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .px_4()
            .pb_2()
            .gap_2()
            .text_sm()
            .child(self.results_header(run, cx))
            .child(summary_band(run, cx))
            .child(self.filters(run, cx))
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

    fn results_header(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let today = run.started_at.date_naive() == chrono::Local::now().date_naive();
        let ran = if today {
            format!("Ran today at {}", run.started_at.format("%H:%M:%S"))
        } else {
            format!("Ran on {}", run.started_at.format("%B %-d at %H:%M:%S"))
        };
        // Outlined tags keep their text readable in every theme.
        let status = match run.status {
            RunStatus::Running => Some(Tag::info().outline().child("Running")),
            RunStatus::Paused => Some(Tag::warning().outline().child("Paused")),
            RunStatus::Stopped => Some(Tag::secondary().outline().child("Stopped")),
            RunStatus::Complete if run.totals.errors > 0 => {
                Some(Tag::danger().outline().child("Error"))
            }
            RunStatus::Complete => None,
        };
        let active = run.is_active();
        let paused = run.status == RunStatus::Paused;
        let sending = run
            .in_flight()
            .filter(|_| run.status == RunStatus::Running)
            .map(|request| request.name.clone());
        let saved = self
            .exported
            .clone()
            .and_then(|exported| exported.ok())
            .filter(|_| !active);
        let export_error = self.exported.clone().and_then(|exported| exported.err());

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
                            .child(format!("{} — Run results", self.title())),
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
                                .flex_none()
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
                                .flex_none()
                                .icon(Icon::default().path("icons/square.svg"))
                                .label("Stop")
                                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                        )
                    })
                    .when_some(saved, |row, path| {
                        row.child(
                            Button::new("run-exported")
                                .debug_selector(|| "run-exported".into())
                                .ghost()
                                .small()
                                .min_w_0()
                                .max_w(rems(18.))
                                .icon(IconName::Check)
                                .label(format!(
                                    "Saved {}",
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .tooltip("Show in folder")
                                .on_click(move |_, _, cx| cx.reveal_path(&path)),
                        )
                    })
                    .when(!active, |row| {
                        row.child(
                            Button::new("run-again")
                                .debug_selector(|| "run-again".into())
                                .primary()
                                .small()
                                .flex_none()
                                .icon(Icon::default().path("icons/square-play.svg"))
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
                                .flex_none()
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
                                .flex_none()
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
                    .min_w_0()
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_none().child(ran))
                    .when(active, |line| {
                        line.child("·").child(div().flex_none().child(format!(
                            "Iteration {} of {}",
                            count_label(run.current_iteration() + 1),
                            count_label(run.iterations)
                        )))
                    })
                    .when_some(sending, |line, name| {
                        line.child("·").child(
                            div()
                                .debug_selector(|| "run-sending".into())
                                .min_w_0()
                                .truncate()
                                .child(format!("Sending “{name}”…")),
                        )
                    })
                    .when(run.status == RunStatus::Stopped, |line| {
                        line.child("·").child(
                            div()
                                .debug_selector(|| "run-stopped".into())
                                .flex_none()
                                .child(format!(
                                    "Stopped after {} of {} requests",
                                    count_label(run.results.len()),
                                    count_label(run.planned().max(run.results.len()))
                                )),
                        )
                    })
                    .when_some(export_error, |line, error| {
                        line.child("·").child(
                            div()
                                .debug_selector(|| "run-export-error".into())
                                .min_w_0()
                                .truncate()
                                .text_color(theme.danger)
                                .child(error),
                        )
                    }),
            )
            .when(!run.missing.is_empty(), |header| {
                header.child(
                    div().debug_selector(|| "run-missing".into()).pt_1().child(
                        Alert::warning(
                            "run-missing",
                            match run.missing.as_slice() {
                                [name] => {
                                    format!("“{name}” is no longer saved, so it did not run.")
                                }
                                names => format!(
                                    "{} requests are no longer saved, so they did not run: {}.",
                                    names.len(),
                                    names.join(", ")
                                ),
                            },
                        )
                        .small(),
                    ),
                )
            })
    }

    fn filters(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let totals = &run.totals;
        // Each tab counts the rows it lists.
        let filters = [
            (ResultFilter::All, "All", run.results.len(), None),
            (
                ResultFilter::Passed,
                "Passed",
                totals.passing_requests,
                Some(theme.success),
            ),
            (
                ResultFilter::Failed,
                "Failed",
                totals.failing_requests,
                Some(theme.danger),
            ),
            (ResultFilter::Skipped, "Skipped", totals.skipped, None),
            (
                ResultFilter::Errors,
                "Errors",
                totals.errors,
                Some(theme.danger),
            ),
            (
                ResultFilter::Console,
                "Console log",
                totals.logs + run.notes.len(),
                None,
            ),
        ];

        Tabs::new("run-result-filters")
            .flex()
            .flex_none()
            .flex_wrap()
            .gap_1()
            .pb_2()
            .border_b_1()
            .border_color(theme.border)
            .children(filters.into_iter().map(|(filter, label, count, color)| {
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
                    .child(
                        div()
                            .text_color(match color {
                                Some(color) if count > 0 => color,
                                _ => theme.muted_foreground,
                            })
                            .child(count_label(count)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
            }))
    }

    fn result_list(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let empty = if !self.results.rows.is_empty() {
            None
        } else if run.results.is_empty() && run.is_active() {
            Some(empty_state(
                Icon::new(IconName::Loader)
                    .text_color(theme.muted_foreground)
                    .with_animation(
                        "run-first-request",
                        Animation::new(std::time::Duration::from_secs(1)).repeat(),
                        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                    ),
                "Sending the first request",
                run.in_flight().map(|request| request.name.clone()),
            ))
        } else if run.results.is_empty() {
            Some(empty_state(
                Icon::new(IconName::Inbox).text_color(theme.muted_foreground),
                "No results",
                Some("The run stopped before a request finished.".into()),
            ))
        } else {
            let (icon, title, description): (IconName, &str, Option<&str>) = match self
                .results
                .filter
            {
                ResultFilter::All => (IconName::Inbox, "No results", None),
                ResultFilter::Passed => (IconName::Inbox, "No tests passed", None),
                ResultFilter::Failed => (IconName::CircleCheck, "No tests failed", None),
                ResultFilter::Skipped => (IconName::CircleCheck, "No requests were skipped", None),
                ResultFilter::Errors => (IconName::CircleCheck, "No errors", None),
                ResultFilter::Console => (
                    IconName::Inbox,
                    "Nothing was logged",
                    Some("Use console.log() in your scripts to see output here."),
                ),
            };
            Some(empty_state(
                Icon::new(icon).text_color(theme.muted_foreground),
                title,
                description.map(SharedString::from),
            ))
        };

        div()
            .debug_selector(|| "run-result-list".into())
            .relative()
            .flex()
            .h_full()
            .min_w_0()
            .map(|pane| {
                if self.results.selected.is_some() {
                    pane.w(relative(0.45)).flex_none()
                } else {
                    pane.flex_1()
                }
            })
            .map(|pane| match empty {
                Some(empty) => pane.child(empty),
                // The right padding keeps the badges clear of the scrollbar.
                None => pane
                    .child(
                        list(
                            self.results.list.clone(),
                            cx.processor(|this, index: usize, _, cx| this.result_row(index, cx)),
                        )
                        .size_full()
                        .pt_1()
                        .pr_3(),
                    )
                    .child(Scrollbar::vertical(&self.results.list)),
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
                .child(format!("Iteration {}", count_label(iteration + 1)))
                .into_any_element(),
            ResultRow::Result(result) => self.result_block(run, result, cx).into_any_element(),
            ResultRow::LogSource { result, report } => {
                let run_result = &run.results[result];
                let request = &run.requests[run_result.position.index];

                div()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .pt_3()
                    .pb_1()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "Iteration {} · {} · {}",
                        count_label(run_result.position.iteration + 1),
                        request.name,
                        run_result.scripts[report].label()
                    ))
                    .into_any_element()
            }
            ResultRow::Log {
                result,
                report,
                log,
            } => {
                let entry = &run.results[result].scripts[report].logs[log];

                log_line(("run-log", index), &entry.level, entry.message.clone(), cx)
                    .px_2()
                    .py_0p5()
                    .into_any_element()
            }
            ResultRow::Note(note) => h_flex()
                .w_full()
                .items_start()
                .px_2()
                .py_1p5()
                .gap_2()
                .child(badge("RUNNER", theme.warning, cx).mt_0p5())
                .child(div().flex_1().min_w_0().child(SelectableText::new(
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

        // The outcome follows the request's name: its status, then what its
        // tests found.
        let outcome = match &result.outcome {
            Outcome::Response { status, .. } => {
                badge(status.as_u16().to_string(), status_color(*status, cx), cx)
            }
            Outcome::Failed(_) => badge("No response", theme.danger, cx),
            Outcome::Skipped(_) => badge("Skipped", theme.muted_foreground, cx),
        };
        let counts = h_flex()
            .flex_none()
            .gap_2()
            .text_xs()
            .when(passed > 0, |counts| {
                counts.child(
                    div()
                        .text_color(theme.success)
                        .child(format!("{passed} passed")),
                )
            })
            .when(failed > 0, |counts| {
                counts.child(
                    div()
                        .text_color(theme.danger)
                        .child(format!("{failed} failed")),
                )
            });

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
                        failure_summary(message),
                        None,
                        cx,
                    ));
                }
                Outcome::Skipped(reason) => {
                    lines.push(line(
                        badge("SKIP", theme.muted_foreground, cx),
                        reason,
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
                        &format!("{}: {error}", report.label()),
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
                    None => line(badge("PASS", theme.success, cx), &test.name, None, cx),
                    Some(error) => {
                        line(badge("FAIL", theme.danger, cx), &test.name, Some(error), cx)
                    }
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
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .child(method_label(result.method.as_str(), cx)),
                    )
                    .child(request_title(
                        ("run-result-title", index),
                        request,
                        self.folder_depth(),
                        cx,
                    ))
                    .child(outcome)
                    .child(counts),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(url),
            )
            .when(no_tests, |block| {
                block.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("No tests"),
                )
            })
            .when(!lines.is_empty(), |block| {
                block.child(v_flex().pt_1().gap_1p5().children(lines))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.select_result(index, window, cx)))
    }
}

/// A test or error under its request: a label, the first line of its
/// text and, for a failed test, the first line of why it failed. The
/// response's Test Results show the rest.
fn line(label: Div, text: &str, error: Option<&str>, cx: &App) -> AnyElement {
    let first_line = |text: &str| text.lines().next().unwrap_or_default().to_owned();

    h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .child(label.mt_0p5())
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(first_line(text)))
                .when_some(error, |test, error| {
                    let more = error.lines().count().saturating_sub(1);

                    test.child(
                        h_flex()
                            .min_w_0()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().min_w_0().truncate().child(first_line(error)))
                            .when(more > 0, |error| {
                                error.child(div().flex_none().child(match more {
                                    1 => "+1 more line".to_owned(),
                                    more => format!("+{more} more lines"),
                                }))
                            }),
                    )
                }),
        )
        .into_any_element()
}

/// The run's environment, how long it took and what its tests found. The
/// cells keep their place as the numbers change during a run.
fn summary_band(run: &Run, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let totals = &run.totals;
    let tabular = FontFeatures(Arc::new(vec![("tnum".into(), 1)]));
    let cell = |label: &'static str, min_width: f32, value: AnyElement| {
        v_flex()
            .flex_none()
            .min_w(rems(min_width))
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .debug_selector(move || format!("run-summary-{label}"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_features(tabular.clone())
                    .child(value),
            )
    };
    let environment = match &run.environment {
        Some(name) => {
            let tooltip = name.clone();

            div()
                .id("run-summary-environment")
                .max_w(rems(14.))
                .truncate()
                .child(name.clone())
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .into_any_element()
        }
        None => div()
            .text_color(theme.muted_foreground)
            .child("No environment")
            .into_any_element(),
    };
    let tests = if totals.tests() == 0 {
        div()
            .text_color(theme.muted_foreground)
            .child("None")
            .into_any_element()
    } else {
        h_flex()
            .gap_1()
            .child(
                div()
                    .text_color(theme.success)
                    .child(format!("{} passed", count_label(totals.passed))),
            )
            .child(div().text_color(theme.muted_foreground).child("·"))
            .child(
                div()
                    .when(totals.failed > 0, |failed| failed.text_color(theme.danger))
                    .child(format!("{} failed", count_label(totals.failed))),
            )
            .into_any_element()
    };
    let progress = run.is_active().then(|| {
        let done = run.results.len() as f32 / run.planned().max(1) as f32;
        (done * 100.).min(100.)
    });

    v_flex()
        .debug_selector(|| "run-summary".into())
        .flex_none()
        .px_4()
        .py_2()
        .gap_2()
        .rounded(theme.radius_tokens().lg)
        .bg(theme.secondary)
        .child(
            h_flex()
                .flex_wrap()
                .gap_x_6()
                .gap_y_2()
                .child(cell("Environment", 5.5, environment))
                .child(cell(
                    "Iterations",
                    4.,
                    div().child(count_label(run.iterations)).into_any_element(),
                ))
                .child(cell(
                    "Duration",
                    4.,
                    div()
                        .child(duration_label(run.duration()))
                        .into_any_element(),
                ))
                .child(cell("Tests", 8., tests))
                .child(cell(
                    "Errors",
                    3.,
                    div()
                        .when(totals.errors > 0, |errors| errors.text_color(theme.danger))
                        .child(count_label(totals.errors))
                        .into_any_element(),
                ))
                .child(cell(
                    "Avg. response",
                    5.,
                    div()
                        .child(
                            totals
                                .average()
                                .map_or_else(|| "–".to_owned(), duration_label),
                        )
                        .into_any_element(),
                )),
        )
        .when_some(progress, |band, progress| {
            band.child(
                Progress::new("run-progress")
                    .accessibility_label("Run progress")
                    .small()
                    .value(progress),
            )
        })
}
