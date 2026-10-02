use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Input, InputGroup, InputGroupAddon, InputGroupAddonAlignment},
    select::Select,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::definition::DefinitionState;
use super::draft::{GrpcDraft, GrpcSection};
use crate::actions::SendRequest;
use crate::variable_input::with_variables;

impl GrpcDraft {
    /// The TLS lock, server URL and method picker share one frame, followed
    /// by Invoke (Cancel while a call runs), as in Postman.
    pub(super) fn url_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let url = self.url_state(window, cx);
        let methods = self.methods_state(window, cx);
        let tls = self.request.uses_tls();
        let running = self.is_running();
        let placeholder: SharedString = match &self.definition {
            DefinitionState::Loading => "Loading methods…".into(),
            _ if self.request.method.is_empty() => "Select a method".into(),
            // A saved method stays visible before its definition loads.
            _ => {
                let (service, method) = self
                    .request
                    .method
                    .rsplit_once('/')
                    .unwrap_or(("", &self.request.method));

                format!(
                    "{} / {method}",
                    service.rsplit('.').next().unwrap_or(service)
                )
                .into()
            }
        };
        let empty: SharedString = match &self.definition {
            DefinitionState::Loading => "Loading methods…".into(),
            DefinitionState::Failed(error) => error.to_string().into(),
            DefinitionState::Loaded(_) => "No methods found".into(),
            DefinitionState::Idle => "Enter a URL to load methods with server reflection".into(),
        };

        h_flex()
            .flex_none()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "grpc-url-bar".into())
                    .flex_1()
                    .min_w_0()
                    .child(with_variables(
                        self.url_completion.as_ref().unwrap(),
                        InputGroup::new("grpc-url-group")
                            .input(Input::new(&url).aria_label("Server URL"))
                            .addon(
                                InputGroupAddon::new("grpc-tls-addon").p_1().child(
                                    Button::new("grpc-tls")
                                        .debug_selector(|| "grpc-tls".into())
                                        .ghost()
                                        .xsmall()
                                        .icon(
                                            Icon::default()
                                                .path(if tls {
                                                    "icons/lock.svg"
                                                } else {
                                                    "icons/lock-open.svg"
                                                })
                                                .text_color(if tls {
                                                    cx.theme().success
                                                } else {
                                                    cx.theme().muted_foreground
                                                }),
                                        )
                                        .selected(tls)
                                        .accessibility_label(if tls {
                                            "TLS enabled"
                                        } else {
                                            "TLS disabled"
                                        })
                                        .tooltip(if tls { "Disable TLS" } else { "Enable TLS" })
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.set_tls(!this.request.uses_tls(), window, cx)
                                        })),
                                ),
                            )
                            .addon(
                                InputGroupAddon::new("grpc-method-addon")
                                    .align(InputGroupAddonAlignment::InlineEnd)
                                    // Leave the URL room in narrow windows.
                                    .w(relative(0.4))
                                    .max_w(rems(24.))
                                    .p_0()
                                    .border_l_1()
                                    .border_color(cx.theme().input)
                                    .child(
                                        div()
                                            .debug_selector(|| "grpc-method".into())
                                            .w_full()
                                            .child(
                                                Select::new(&methods)
                                                    .small()
                                                    .appearance(false)
                                                    .accessibility_label("Method")
                                                    .placeholder(placeholder)
                                                    .search_placeholder("Search methods")
                                                    .menu_width(rems(28.))
                                                    .menu_max_h(rems(24.))
                                                    .empty(move |_, cx| {
                                                        div()
                                                            .p_3()
                                                            .text_sm()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(empty.clone())
                                                            .into_any_element()
                                                    }),
                                            ),
                                    ),
                            ),
                    )),
            )
            .child(
                Button::new("grpc-invoke")
                    .debug_selector(|| "grpc-invoke".into())
                    .primary()
                    .min_w_20()
                    .flex_none()
                    .label(if running { "Cancel" } else { "Invoke" })
                    .accessibility_label(if running {
                        "Cancel call"
                    } else {
                        "Invoke method"
                    })
                    .when(!running, |button| {
                        button.tooltip_with_action("Invoke", &SendRequest, Some("Workspace"))
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.is_running() {
                            this.cancel(cx);
                        } else {
                            this.invoke(window, cx);
                        }
                    })),
            )
    }

    /// Turn TLS on or off, switching a scheme in the URL to match.
    pub(super) fn set_tls(&mut self, tls: bool, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.request.url.clone();
        self.request.set_tls(tls);

        if self.request.url != url
            && let Some(input) = &self.url
        {
            let url = self.request.url.clone();
            input.update(cx, |input, cx| input.set_value(url, window, cx));
        }

        self.schedule_reflection(window, cx);
        self.redraw(cx);
    }

    pub(super) fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let definition_failed = matches!(self.definition, DefinitionState::Failed(_));
        let sections = [
            ("Message", GrpcSection::Message, 0),
            ("Auth", GrpcSection::Auth, 0),
            (
                "Metadata",
                GrpcSection::Metadata,
                self.request.metadata.len(),
            ),
            ("Service definition", GrpcSection::Definition, 0),
            ("Scripts", GrpcSection::Scripts, self.script_count()),
            ("Settings", GrpcSection::Settings, 0),
        ];

        h_flex().flex_none().gap_2().min_w_0().child(
            Tabs::new("grpc-sections")
                .flex_1()
                .min_w_0()
                .flex()
                .overflow_x_scroll()
                .gap_1()
                .children(sections.into_iter().map(|(label, section, count)| {
                    let selected = section == self.section;

                    Tab::new(label)
                        .debug_selector(move || format!("grpc-section-{label}"))
                        .selected(selected)
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
                        .hover(|this| this.bg(cx.theme().muted))
                        .child(label)
                        .when(count > 0, |this| {
                            this.child(crate::section_count::section_count(count, selected, cx))
                        })
                        .when(
                            section == GrpcSection::Definition && definition_failed,
                            |this| {
                                this.child(
                                    div()
                                        .debug_selector(|| "grpc-definition-error-dot".into())
                                        .size_1p5()
                                        .rounded(cx.theme().radius_full())
                                        .bg(cx.theme().danger),
                                )
                            },
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.section = section;
                            this.prepare(window, cx);
                            this.redraw(cx);
                        }))
                })),
        )
    }
}
