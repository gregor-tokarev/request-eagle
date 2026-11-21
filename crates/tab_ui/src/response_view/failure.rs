use std::time::Duration;

use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ErrorSummary;

use super::view::{ResponseState, ResponseView};

/// How long the copy button confirms that the diagnostic chain was copied.
const COPY_FEEDBACK_DURATION: Duration = Duration::from_secs(2);

impl ResponseView {
    pub(super) fn failure_state(
        &self,
        summary: &ErrorSummary,
        details_open: bool,
        details_copied: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = EmptyHeader::new()
            .max_w(rems(28.))
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .child(Icon::new(IconName::TriangleAlert).text_color(cx.theme().danger)),
            )
            .title(
                EmptyTitle::new().child(
                    div()
                        .debug_selector(|| "response-error-title".into())
                        .child(summary.title.clone()),
                ),
            )
            .description(
                EmptyDescription::new().child(
                    div()
                        .debug_selector(|| "response-error-hint".into())
                        .cursor_text()
                        .child(SelectableText::new(
                            ("response-error-hint-text", self.responses),
                            summary.hint.clone(),
                        )),
                ),
            );

        let content = summary.details.as_ref().map(|details| {
            EmptyContent::new()
                .max_w(rems(36.))
                .gap_3()
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_center()
                        .gap_2()
                        .child(
                            Button::new("response-error-toggle")
                                .debug_selector(|| "response-error-toggle".into())
                                .ghost()
                                .small()
                                .icon(if details_open {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .label(if details_open {
                                    "Hide details"
                                } else {
                                    "Show details"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_error_details(cx);
                                })),
                        )
                        .child(
                            Button::new("response-error-copy")
                                .debug_selector(|| "response-error-copy".into())
                                .ghost()
                                .small()
                                .icon(if details_copied {
                                    IconName::Check
                                } else {
                                    IconName::Copy
                                })
                                .label(if details_copied {
                                    "Copied"
                                } else {
                                    "Copy details"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.copy_error_details(cx);
                                })),
                        ),
                )
                .when(details_open, |content| {
                    content.child(
                        div()
                            .debug_selector(|| "response-error-details".into())
                            .w_full()
                            .min_w_0()
                            .p_3()
                            .rounded(cx.theme().radius_tokens().lg)
                            .bg(cx.theme().secondary)
                            .border_1()
                            .border_color(cx.theme().border)
                            .text_left()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .line_height(relative(1.5))
                            .text_color(cx.theme().foreground)
                            .whitespace_normal()
                            .cursor_text()
                            .child(
                                SelectableText::new(
                                    ("response-error-details-text", self.responses),
                                    details.clone(),
                                )
                                .document_order(1),
                            ),
                    )
                })
        });

        v_flex()
            .debug_selector(|| "response-empty".into())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .items_center()
            .p_6()
            .child(div().flex_1())
            .child(
                Empty::new()
                    .flex_none()
                    .p_0()
                    .header(header)
                    .when_some(content, |empty, content| empty.content(content)),
            )
            .child(div().flex_1())
            .overflow_y_scrollbar()
            .id(("response-error-scroll", cx.entity_id()))
            .into_any_element()
    }

    pub(super) fn failure_banner(&self, summary: &ErrorSummary, cx: &App) -> Div {
        let message = if summary.details.is_some() || summary.title != "Request failed" {
            format!("{}: {}", summary.title, summary.hint)
        } else {
            summary.hint.clone()
        };

        div().px_2().text_color(cx.theme().danger).child(message)
    }

    fn toggle_error_details(&mut self, cx: &mut Context<Self>) {
        if let ResponseState::Failed { details_open, .. } = &mut self.state {
            *details_open = !*details_open;
            cx.notify();
        }
    }

    fn copy_error_details(&mut self, cx: &mut Context<Self>) {
        let ResponseState::Failed {
            summary,
            details_copied,
            ..
        } = &mut self.state
        else {
            return;
        };

        let text = summary.details.as_deref().unwrap_or(&summary.hint);
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        *details_copied = true;

        let response = self.responses;
        self.copy_reset_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPY_FEEDBACK_DURATION).await;
            let _ = this.update(cx, |this, cx| {
                if this.responses == response
                    && let ResponseState::Failed { details_copied, .. } = &mut this.state
                {
                    *details_copied = false;
                    cx.notify();
                }
            });
        }));

        cx.notify();
    }
}
