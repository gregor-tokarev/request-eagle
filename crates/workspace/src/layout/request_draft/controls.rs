use collection::Method;
use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::draft::{RequestDraft, RequestSection};

fn method_color(method: Method, cx: &App) -> Hsla {
    match method {
        Method::Get => cx.theme().success,
        Method::Post => cx.theme().warning,
        Method::Put => cx.theme().info,
        Method::Delete => cx.theme().danger,
    }
}

impl RequestDraft {
    pub(super) fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .flex_none()
            .h(px(40.))
            .gap_3()
            .child(
                v_flex()
                    .flex_none()
                    .size(px(28.))
                    .justify_center()
                    .items_center()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(px(7.))
                    .text_color(cx.theme().info)
                    .text_size(px(8.))
                    .font_weight(FontWeight::BOLD)
                    .child("HTTP"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Untitled Request"),
            )
            .child(
                div()
                    .debug_selector(|| "request-collection".into())
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child("No collection"),
            )
            .child(div().flex_1())
            .child(
                Button::new("save-request")
                    .ghost()
                    .small()
                    .label("Save")
                    .icon(IconName::File)
                    .disabled(true),
            )
            .child(
                Button::new("save-request-options")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronDown)
                    .disabled(true),
            )
            .child(
                Button::new("share-request")
                    .small()
                    .label("Share")
                    .icon(IconName::ExternalLink)
                    .disabled(true),
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

        h_flex()
            .flex_none()
            .gap_2()
            .child(
                h_flex()
                    .debug_selector(|| "request-url-bar".into())
                    .flex_1()
                    .min_w_0()
                    .h(px(40.))
                    .border_1()
                    .border_color(cx.theme().input)
                    .rounded(px(7.))
                    .child(
                        Button::new("request-method")
                            .debug_selector(|| "request-method".into())
                            .ghost()
                            .h(px(38.))
                            .w(px(110.))
                            .justify_between()
                            .px_3()
                            .accessibility_label(format!("Request method: {}", method.as_str()))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(method_color(method, cx))
                                    .child(method.as_str()),
                            )
                            .child(
                                Icon::new(IconName::ChevronDown)
                                    .size(px(12.))
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .dropdown_menu(move |mut menu, _, _| {
                                for option in
                                    [Method::Get, Method::Post, Method::Put, Method::Delete]
                                {
                                    let draft = draft.clone();
                                    menu =
                                        menu.item(
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
                            }),
                    )
                    .child(div().w(px(1.)).h(px(24.)).bg(cx.theme().border))
                    .child(
                        div()
                            .debug_selector(|| "request-url".into())
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&url).appearance(false).aria_label("Request URL")),
                    ),
            )
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .rounded(px(7.))
                    .overflow_hidden()
                    .bg(rgb(0x2e66ce))
                    .child(
                        Button::new("send-request")
                            .debug_selector(|| "send-request".into())
                            .ghost()
                            .h_full()
                            .w(px(84.))
                            .label(if sending { "Cancel" } else { "Send" })
                            .accessibility_label(if sending {
                                "Cancel request"
                            } else {
                                "Send request"
                            })
                            .text_color(rgb(0xffffff))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.task.is_some() {
                                    this.cancel(cx);
                                } else {
                                    this.send(window, cx);
                                }
                            })),
                    )
                    .child(div().h_full().w(px(1.)).bg(rgb(0x2554a8)))
                    .child(
                        Button::new("send-request-options")
                            .ghost()
                            .h_full()
                            .w(px(30.))
                            .icon(Icon::new(IconName::ChevronDown).size(px(13.)))
                            .text_color(rgb(0xffffff))
                            .disabled(true),
                    ),
            )
    }

    pub(super) fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sections = [
            ("Docs", None),
            ("Params", Some(RequestSection::Params)),
            ("Authorization", None),
            ("Headers", Some(RequestSection::Headers)),
            ("Body", self.supports_body().then_some(RequestSection::Body)),
            ("Scripts", None),
            ("Settings", None),
        ];

        h_flex()
            .flex_none()
            .gap_2()
            .min_w_0()
            .child(
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
                            Some(RequestSection::Headers) => self.request.headers.len(),
                            _ => 0,
                        };

                        Tab::new(label)
                            .debug_selector(move || format!("request-section-{label}"))
                            .selected(selected)
                            .disabled(section.is_none())
                            .accessibility_label(label)
                            .flex_none()
                            .h(px(30.))
                            .px_2()
                            .gap_1()
                            .rounded(px(4.))
                            .text_color(cx.theme().muted_foreground)
                            .when(selected, |this| {
                                this.bg(cx.theme().muted).text_color(cx.theme().foreground)
                            })
                            .when(section.is_some(), |this| {
                                this.hover(|this| this.bg(cx.theme().muted))
                            })
                            .when(label == "Docs", |this| {
                                this.child(Icon::new(IconName::Menu).size(px(13.)))
                            })
                            .child(label)
                            .when(count > 0, |this| {
                                this.child(div().text_size(px(11.)).child(count.to_string()))
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
            .child(
                Button::new("request-cookies")
                    .ghost()
                    .small()
                    .label("Cookies")
                    .disabled(true),
            )
    }
}
