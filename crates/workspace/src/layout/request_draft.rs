use collection::{HttpRequest, Method};
use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::request_fields::{FieldsChanged, RequestFields};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestSection {
    Params,
    Headers,
    Body,
}

pub(super) struct MethodChanged(pub Method);

fn method_color(method: Method, cx: &App) -> Hsla {
    match method {
        Method::Get => cx.theme().success,
        Method::Post => cx.theme().warning,
        Method::Put => cx.theme().info,
        Method::Delete => cx.theme().danger,
    }
}

/// An unsaved request owned by one tab, independent of the collections registry.
pub(super) struct RequestDraft {
    pub(super) request: HttpRequest,
    pub(super) url: Option<Entity<InputState>>,
    pub(super) section: RequestSection,
    params: Option<Entity<RequestFields>>,
    headers: Option<Entity<RequestFields>>,
    body: Option<Entity<TextareaState>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MethodChanged> for RequestDraft {}

impl RequestDraft {
    pub(super) fn new() -> Self {
        Self {
            request: HttpRequest::default(),
            url: None,
            section: RequestSection::Headers,
            params: None,
            headers: None,
            body: None,
            _subscriptions: Vec::new(),
        }
    }

    pub(super) fn set_method(&mut self, method: Method, cx: &mut Context<Self>) {
        self.request.method = method;
        cx.emit(MethodChanged(method));
        cx.notify();
    }

    pub(super) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Initialize newly activated controls before drawing. Their setup can
        // notify GPUI; doing it inside render schedules an unnecessary frame.
        self.url_state(window, cx);

        if self.section != RequestSection::Body {
            self.fields_state(window, cx);
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

    fn url_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        // Unvisited tabs only need request data. Creating an InputState also
        // registers window and keystroke listeners, so wait until it is visible.
        self.url
            .get_or_insert_with(|| {
                let url = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Enter URL or paste text")
                        .default_value(self.request.path.clone())
                });
                self._subscriptions.push(cx.subscribe(
                    &url,
                    |this, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.request.path = input.read(cx).value().to_string();
                        }
                    },
                ));

                url
            })
            .clone()
    }

    fn url_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let url = self.url_state(window, cx);
        let method = self.request.method;
        let draft = cx.entity().downgrade();

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
                                            .on_click(move |_, _, cx| {
                                                let _ = draft.update(cx, |draft, cx| {
                                                    draft.set_method(option, cx)
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
                            .ghost()
                            .h_full()
                            .w(px(84.))
                            .label("Send")
                            .text_color(rgb(0xffffff))
                            .disabled(true),
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

    fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sections = [
            ("Docs", None),
            ("Params", Some(RequestSection::Params)),
            ("Authorization", None),
            ("Headers", Some(RequestSection::Headers)),
            ("Body", Some(RequestSection::Body)),
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
                                this.on_click(cx.listener(move |this, _, _, cx| {
                                    this.section = section;
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

    fn fields_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RequestFields> {
        let is_headers = self.section == RequestSection::Headers;
        let slot = if is_headers {
            &mut self.headers
        } else {
            &mut self.params
        };

        if slot.is_none() {
            let id = if is_headers { "headers" } else { "params" };
            let fields = cx.new(|cx| RequestFields::new(id, window, cx));
            let subscription = cx.subscribe(&fields, move |this, _, event: &FieldsChanged, cx| {
                if is_headers {
                    this.request.headers = event.0.clone();
                } else {
                    this.request.query = (!event.0.is_empty()).then(|| event.0.clone());
                }

                cx.notify();
            });
            self._subscriptions.push(subscription);
            *slot = Some(fields);
        }

        slot.as_ref().unwrap().clone()
    }

    fn fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fields = self.fields_state(window, cx);
        let is_headers = self.section == RequestSection::Headers;

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .h(px(24.))
                    .gap_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(if is_headers {
                        "Headers"
                    } else {
                        "Query Params"
                    }),
            )
            .child(fields)
            .into_any_element()
    }

    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.body.is_none() {
            let body = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Enter request body")
                    .rows(8)
            });
            let subscription = cx.subscribe(&body, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let body = input.read(cx).value();
                    this.request.body = (!body.is_empty()).then(|| body.as_bytes().to_vec());
                }
            });
            self._subscriptions.push(subscription);
            self.body = Some(body);
        }

        v_flex()
            .gap_2()
            .child(
                div()
                    .h(px(24.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Raw body"),
            )
            .child(
                div().debug_selector(|| "request-body".into()).child(
                    Textarea::new(self.body.as_ref().unwrap())
                        .h(px(220.))
                        .aria_label("Request body"),
                ),
            )
            .into_any_element()
    }
}

impl Render for RequestDraft {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.section {
            RequestSection::Headers | RequestSection::Params => self.fields(window, cx),
            RequestSection::Body => self.body(window, cx),
        };

        v_flex()
            .debug_selector(|| "request-draft".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_4()
            .gap_2()
            .text_size(px(13.))
            .child(self.header(cx))
            .child(self.url_bar(window, cx))
            .child(self.section_tabs(cx))
            .child(
                div()
                    .id("request-section-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
    }
}
