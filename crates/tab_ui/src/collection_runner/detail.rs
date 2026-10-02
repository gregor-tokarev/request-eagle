use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Execution, HttpResponse, Response, ScriptReport};
use request_eagle_theme::method_label;

use super::results::request_title;
use super::run::{KEPT_BODY_LIMIT, Kept, Outcome};
use super::runner::{CollectionRunner, Run};
use crate::response_view::{ResponseContent, ResponseView};

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
        // The same request's results in other iterations. A request that
        // scripts send again in its iteration counts its runs: 1, 1.2, 1.3.
        let mut previous = None;
        let mut repeat = 1;
        let iterations = run
            .results
            .iter()
            .enumerate()
            .filter(|(_, other)| other.position.index == result.position.index)
            .map(|(other, other_result)| {
                let iteration = other_result.position.iteration + 1;
                repeat = if previous == Some(iteration) {
                    repeat + 1
                } else {
                    1
                };
                previous = Some(iteration);

                match repeat {
                    1 => (
                        other,
                        iteration.to_string(),
                        format!("Iteration {iteration}"),
                    ),
                    run => (
                        other,
                        format!("{iteration}.{run}"),
                        format!("Iteration {iteration}, run {run}"),
                    ),
                }
            })
            .collect::<Vec<_>>();

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
            ) => unavailable(
                IconName::Inbox,
                "Response not kept",
                format!(
                    "A run keeps {} MB of responses, and earlier responses used it up.",
                    KEPT_BODY_LIMIT / (1024 * 1024)
                ),
                cx,
            ),
            (_, Outcome::Response { .. }) => unavailable(
                IconName::Inbox,
                "Response not kept",
                "Turn on Persist responses for a session in Advanced settings to see responses after a run.",
                cx,
            ),
            (_, Outcome::Failed(message)) => {
                unavailable(IconName::TriangleAlert, "No response", message.clone(), cx)
            }
            (_, Outcome::Skipped(reason)) => {
                unavailable(IconName::Inbox, "Request skipped", reason.clone(), cx)
            }
        };

        h_flex()
            .debug_selector(|| "run-result-detail".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .items_start()
            .border_l_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .id("run-detail-iterations")
                    .flex_none()
                    .h_full()
                    .w_10()
                    .overflow_y_scrollbar()
                    .pt_1()
                    .gap_1()
                    .items_center()
                    .border_r_1()
                    .border_color(theme.border)
                    .children(iterations.into_iter().map(|(other, label, tooltip)| {
                        Button::new(("run-detail-iteration", other))
                            .ghost()
                            .xsmall()
                            .selected(other == index)
                            .label(label)
                            .tooltip(tooltip)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_result(other, window, cx)
                            }))
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px_3()
                    .gap_1()
                    .child(
                        h_flex()
                            .h_10()
                            .flex_none()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .child(method_label(result.method.as_str(), cx)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(theme.muted_foreground)
                                    .child(self.collection_name.clone()),
                            )
                            .child(
                                Icon::new(IconName::ChevronRight)
                                    .size_3()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(request_title(request, cx))
                            .child(
                                Button::new("close-run-detail")
                                    .debug_selector(|| "close-run-detail".into())
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Close")
                                    .on_click(cx.listener(|this, _, _, cx| this.close_detail(cx))),
                            ),
                    )
                    .when_some(result.url.clone(), |pane, url| {
                        pane.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SelectableText::new("run-detail-url", url)),
                        )
                    })
                    .child(div().flex_1().min_h_0().flex().child(content)),
            )
    }
}

fn unavailable(
    icon: IconName,
    title: &'static str,
    description: impl Into<SharedString>,
    cx: &App,
) -> AnyElement {
    let description = description.into();

    div()
        .debug_selector(|| "run-detail-unavailable".into())
        .flex()
        .flex_1()
        .min_h_0()
        .child(
            Empty::new().header(
                EmptyHeader::new()
                    .media(
                        EmptyMedia::new()
                            .with_variant(EmptyMediaVariant::Icon)
                            .child(Icon::new(icon).text_color(cx.theme().muted_foreground)),
                    )
                    .title(EmptyTitle::new().child(title))
                    .description(EmptyDescription::new().child(description)),
            ),
        )
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
