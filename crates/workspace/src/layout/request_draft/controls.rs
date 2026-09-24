use collection::Method;
use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Input, InputGroup, InputGroupAddon},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::draft::{RequestDraft, RequestSection};
use crate::actions::SendRequest;

fn method_color(method: Method, cx: &App) -> Hsla {
    match method {
        Method::Get => cx.theme().success,
        Method::Post => cx.theme().warning,
        Method::Put | Method::Patch => cx.theme().info,
        Method::Head | Method::Options => cx.theme().muted_foreground,
        Method::Delete => cx.theme().danger,
    }
}

impl RequestDraft {
    pub(super) fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .flex_none()
            .h_10()
            .gap_3()
            .child(
                v_flex()
                    .flex_none()
                    .h_8()
                    .px_2()
                    .justify_center()
                    .items_center()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_tokens().md)
                    .text_color(cx.theme().muted_foreground)
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .child("HTTP"),
            )
            .child(
                h_flex()
                    .debug_selector(|| "request-breadcrumbs".into())
                    .min_w_0()
                    .gap_1()
                    .text_sm()
                    .overflow_hidden()
                    .when_some(self.collection.clone(), |row, collection| {
                        row.child(
                            div()
                                .debug_selector(|| "request-collection".into())
                                .flex_none()
                                .text_color(cx.theme().muted_foreground)
                                .child(collection),
                        )
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size_3()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .children(self.folders.iter().enumerate().map(|(index, folder)| {
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .child(
                                div()
                                    .debug_selector(move || format!("request-folder-{index}"))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(folder.clone()),
                            )
                            .child(
                                Icon::new(IconName::ChevronRight)
                                    .size_3()
                                    .text_color(cx.theme().muted_foreground),
                            )
                    }))
                    .child(
                        div()
                            .debug_selector(|| "request-name".into())
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.name.clone()),
                    ),
            )
    }

    pub(super) fn url_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let url = self.url_state(window, cx);
        let method = self.request.method;
        let draft = cx.entity().downgrade();
        let sending = self.task.is_some();
        let method_button = Button::new("request-method")
            .debug_selector(|| "request-method".into())
            .ghost()
            .small()
            .w_24()
            .rounded(cx.theme().radius_tokens().sm)
            .justify_between()
            .px_3()
            .accessibility_label(format!("Request method: {}", method.as_str()))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        div()
                            .debug_selector(|| "request-method-label".into())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(method_color(method, cx))
                            .child(method.as_str()),
                    )
                    .child(
                        div()
                            .debug_selector(|| "request-method-arrow".into())
                            .child(
                                Icon::new(IconName::ChevronDown)
                                    .size_3()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    ),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for option in [
                    Method::Get,
                    Method::Post,
                    Method::Put,
                    Method::Patch,
                    Method::Delete,
                    Method::Head,
                    Method::Options,
                ] {
                    let draft = draft.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(method_color(option, cx))
                                .child(option.as_str())
                        })
                        .checked(option == method)
                        .on_click(move |_, window, cx| {
                            let _ = draft.update(cx, |draft, cx| {
                                draft.set_method(option, cx);
                                draft.prepare(window, cx);
                            });
                        }),
                    );
                }

                menu
            });

        h_flex()
            .flex_none()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "request-url-bar".into())
                    .flex_1()
                    .min_w_0()
                    .child(
                        div().debug_selector(|| "request-url".into()).child(
                            InputGroup::new("request-url-group")
                                .input(Input::new(&url).aria_label("Request URL"))
                                .addon(
                                    InputGroupAddon::new("request-method-addon")
                                        .p_1()
                                        .child(method_button),
                                ),
                        ),
                    ),
            )
            .child(
                Button::new("send-request")
                    .debug_selector(|| "send-request".into())
                    .primary()
                    .min_w_20()
                    .flex_none()
                    .label(if sending { "Cancel" } else { "Send" })
                    .accessibility_label(if sending {
                        "Cancel request"
                    } else {
                        "Send request"
                    })
                    .when(!sending, |button| {
                        button.tooltip_with_action("Send request", &SendRequest, Some("Workspace"))
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.task.is_some() {
                            this.cancel(cx);
                        } else {
                            this.send(window, cx);
                        }
                    })),
            )
    }

    pub(super) fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sections = [
            ("Params", Some(RequestSection::Params)),
            ("Headers", Some(RequestSection::Headers)),
            ("Body", self.supports_body().then_some(RequestSection::Body)),
        ];

        h_flex().flex_none().gap_2().min_w_0().child(
            Tabs::new("request-sections")
                .flex_1()
                .min_w_0()
                .flex()
                .overflow_x_scroll()
                .gap_1()
                .children(sections.into_iter().map(|(label, section)| {
                    let selected = section == Some(self.section);
                    let count = match section {
                        Some(RequestSection::Params) => {
                            self.request.query.as_ref().map_or(0, Vec::len)
                        }
                        Some(RequestSection::Headers) => {
                            self.request.headers.len() + self.generated_headers.len()
                        }
                        _ => 0,
                    };

                    Tab::new(label)
                        .debug_selector(move || format!("request-section-{label}"))
                        .selected(selected)
                        .disabled(section.is_none())
                        .accessibility_label(label)
                        .flex_none()
                        .h_8()
                        .px_2()
                        .gap_1()
                        .rounded(cx.theme().radius_tokens().md)
                        .text_color(cx.theme().muted_foreground)
                        .when(selected, |this| {
                            this.bg(cx.theme().muted).text_color(cx.theme().foreground)
                        })
                        .when(section.is_some(), |this| {
                            this.hover(|this| this.bg(cx.theme().muted))
                        })
                        .child(label)
                        .when(count > 0, |this| {
                            this.child(
                                div()
                                    .debug_selector(move || {
                                        format!("request-section-{label}-count-{count}")
                                    })
                                    .text_xs()
                                    .child(count.to_string()),
                            )
                        })
                        .when_some(section, |this, section| {
                            this.on_click(cx.listener(move |this, _, window, cx| {
                                this.section = section;
                                this.prepare(window, cx);
                                cx.notify();
                            }))
                        })
                })),
        )
    }
}
