use gpui_kit::base::{SelectableText, Tab, Tabs};
use gpui_kit::component::{
    button::*,
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Execution, HttpResponse, Response, ScriptReport};
use request_eagle_theme::method_label;

use super::results::request_title;
use super::run::{KEPT_BODY_LIMIT, Kept, Outcome, RunResult, count_label};
use super::runner::{CollectionRunner, Run};
use crate::response_view::{ResponseContent, ResponseView, script_results};
use crate::section_count::section_count;

/// A result's response beside the list, with the same request's results in
/// other iterations.
impl CollectionRunner {
    pub(super) fn close_detail(&mut self, cx: &mut Context<Self>) {
        self.results.selected = None;
        self.detail = None;
        self.detail_task = None;
        cx.notify();
    }

    /// Show a result's response beside the list.
    pub(super) fn select_result(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(result) = self.run.as_ref().and_then(|run| run.results.get(index)) else {
            return;
        };

        self.results.selected = Some(index);
        self.detail_task = None;

        let Outcome::Response {
            response: Kept::Response(execution),
            ..
        } = &result.outcome
        else {
            self.detail = None;
            cx.notify();
            return;
        };

        let execution = execution.clone();
        let scripts = result.scripts.clone();
        let url = result.url.clone().unwrap_or_default();
        let view = self
            .detail
            .get_or_insert_with(|| cx.new(|cx| ResponseView::new(cx).with_request_section()))
            .clone();
        view.update(cx, |view, cx| view.start(cx));

        // Copying and formatting a large body takes a while, as it does
        // after sending.
        let content = cx.background_spawn(async move {
            ResponseContent::new(shown(&execution, scripts)).named_after(&url)
        });
        self.detail_task = Some(cx.spawn_in(window, async move |_, cx| {
            let content = content.await;
            let _ = view.update_in(cx, |view, window, cx| view.finish(Ok(content), window, cx));
        }));
        cx.notify();
    }

    pub(super) fn detail_pane(
        &self,
        run: &Run,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let result = &run.results[index];
        let request = &run.requests[result.position.index];
        let url = result
            .url
            .clone()
            .unwrap_or_else(|| request.request.path.clone());

        let content = match (&self.detail, &result.outcome) {
            (
                Some(view),
                Outcome::Response {
                    response: Kept::Response(_),
                    ..
                },
            ) => view.clone().into_any_element(),
            (
                _,
                Outcome::Response {
                    response: Kept::OverLimit,
                    ..
                },
            ) => self.without_response(
                result,
                Icon::new(IconName::Inbox),
                "Response not kept",
                format!(
                    "A run keeps {} MB of responses, and earlier responses used it up.",
                    KEPT_BODY_LIMIT / (1024 * 1024)
                ),
                cx,
            ),
            (_, Outcome::Response { .. }) => self.without_response(
                result,
                Icon::new(IconName::Inbox),
                "Response not kept",
                "Turn on Persist responses for a session in Advanced settings to see responses after a run.".into(),
                cx,
            ),
            (_, Outcome::Failed(message)) => self.without_response(
                result,
                Icon::new(IconName::TriangleAlert).text_color(theme.danger),
                "No response",
                message.clone(),
                cx,
            ),
            (_, Outcome::Skipped(reason)) => self.without_response(
                result,
                Icon::new(IconName::Inbox),
                "Request skipped",
                reason.clone(),
                cx,
            ),
        };

        v_flex()
            .debug_selector(|| "run-result-detail".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .pl_3()
            .gap_1()
            .border_l_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .h_10()
                    .flex_none()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .child(method_label(result.method.as_str(), cx)),
                    )
                    .child(
                        request_title("run-detail-title", request, self.folder_depth(), cx)
                            .flex_1(),
                    )
                    .child(
                        Button::new("close-run-detail")
                            .debug_selector(|| "close-run-detail".into())
                            .ghost()
                            .xsmall()
                            .flex_none()
                            .icon(IconName::Close)
                            .tooltip("Close")
                            .on_click(cx.listener(|this, _, _, cx| this.close_detail(cx))),
                    ),
            )
            .child(
                // The steps move under the address when both do not fit.
                h_flex()
                    .flex_none()
                    .min_w_0()
                    .flex_wrap()
                    .items_start()
                    .gap_x_3()
                    .gap_y_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w(rems(12.))
                            .max_w_full()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SelectableText::new("run-detail-url", url)),
                    )
                    .children(self.iteration_navigation(run, index, cx)),
            )
            .child(div().flex_1().min_h_0().flex().child(content))
    }

    /// Steps through the same request's results in other iterations. A
    /// request that scripts send again in its iteration counts its runs.
    fn iteration_navigation(
        &self,
        run: &Run,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let result = &run.results[index];
        let same = run
            .results
            .iter()
            .enumerate()
            .filter(|(_, other)| other.position.index == result.position.index)
            .map(|(other, other_result)| (other, other_result.position.iteration))
            .collect::<Vec<_>>();
        if same.len() < 2 {
            return None;
        }

        let place = same.iter().position(|(other, _)| *other == index)?;
        let runs = same
            .iter()
            .filter(|(_, iteration)| *iteration == result.position.iteration)
            .map(|(other, _)| *other)
            .collect::<Vec<_>>();
        let mut label = format!(
            "Iteration {} of {}",
            count_label(result.position.iteration + 1),
            count_label(run.iterations)
        );
        if runs.len() > 1 {
            let number = runs.iter().position(|other| *other == index).unwrap_or(0) + 1;
            label += &format!(" · run {number} of {}", runs.len());
        }
        let previous = place.checked_sub(1).map(|place| same[place].0);
        let next = same.get(place + 1).map(|(other, _)| *other);
        let step = |id: &'static str, icon: IconName, tooltip: &'static str, to: Option<usize>| {
            Button::new(id)
                .debug_selector(move || id.into())
                .ghost()
                .xsmall()
                .icon(icon)
                .accessibility_label(tooltip)
                .tooltip(tooltip)
                .disabled(to.is_none())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(to) = to {
                        this.select_result(to, window, cx);
                    }
                }))
        };

        Some(
            h_flex()
                .flex_none()
                .gap_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(step(
                    "run-detail-previous",
                    IconName::ChevronLeft,
                    "Previous run of this request",
                    previous,
                ))
                .child(
                    div()
                        .debug_selector(|| "run-detail-iteration".into())
                        .child(label),
                )
                .child(step(
                    "run-detail-next",
                    IconName::ChevronRight,
                    "Next run of this request",
                    next,
                )),
        )
    }

    /// A result without a response to show. Its tests and console output
    /// are kept, so they stay one click away.
    fn without_response(
        &self,
        result: &RunResult,
        icon: Icon,
        title: &'static str,
        description: String,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let tests = result
            .scripts
            .iter()
            .map(|report| report.tests.len() + usize::from(report.error.is_some()))
            .sum::<usize>();
        let logs = result
            .scripts
            .iter()
            .map(|report| report.logs.len())
            .sum::<usize>();
        if tests + logs == 0 {
            return empty_state(icon, title, Some(description.into()));
        }

        let console = self.detail_console;

        v_flex()
            .debug_selector(|| "run-detail-unavailable".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .flex_none()
                    .items_start()
                    .gap_2()
                    .p_3()
                    .rounded(theme.radius_tokens().lg)
                    .bg(theme.secondary)
                    .child(
                        div()
                            .mt_0p5()
                            .text_color(theme.muted_foreground)
                            .child(icon),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().font_weight(FontWeight::MEDIUM).child(title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(description),
                            ),
                    ),
            )
            .child(
                Tabs::new("run-detail-sections")
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .min_w_0()
                    .gap_1()
                    .children(
                        [(false, "Test Results", tests), (true, "Console", logs)].map(
                            |(section, label, count)| {
                                let selected = console == section;

                                Tab::new(label)
                                    .selected(selected)
                                    .h_8()
                                    .px_2()
                                    .gap_1()
                                    .rounded(theme.radius_tokens().md)
                                    .text_color(theme.muted_foreground)
                                    .when(selected, |tab| {
                                        tab.bg(theme.muted).text_color(theme.foreground)
                                    })
                                    .hover(|tab| tab.bg(theme.muted))
                                    .child(label)
                                    .when(count > 0, |tab| {
                                        tab.child(section_count(count, selected, cx))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.detail_console = section;
                                        cx.notify();
                                    }))
                            },
                        ),
                    ),
            )
            .child(script_results(
                &result.scripts,
                console,
                "run the requests again",
                cx,
            ))
            .into_any_element()
    }
}

/// A list or pane with nothing to show: an icon, a title and what to do.
pub(super) fn empty_state(
    media: impl IntoElement,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
) -> AnyElement {
    let header = EmptyHeader::new()
        .media(
            EmptyMedia::new()
                .with_variant(EmptyMediaVariant::Icon)
                .child(media),
        )
        .title(EmptyTitle::new().child(title.into()));
    let header = match description {
        Some(description) => header.description(EmptyDescription::new().child(description)),
        None => header,
    };

    div()
        .debug_selector(|| "run-empty".into())
        .flex()
        .flex_1()
        .min_h_0()
        .child(Empty::new().header(header))
        .into_any_element()
}

/// A kept response to show, with the result's scripts' reports.
fn shown(execution: &Execution, scripts: Vec<ScriptReport>) -> Execution {
    let Response::Http(response) = &execution.response;

    Execution {
        response: Response::Http(HttpResponse {
            status: response.status,
            version: response.version,
            headers: response.headers.clone(),
            body: response.body.clone(),
            metrics: response.metrics,
        }),
        elapsed: execution.elapsed,
        scripts,
        sent: execution.sent.clone(),
    }
}
