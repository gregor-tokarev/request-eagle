use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{search::BodySearch, view::ResponseView, virtual_body::VirtualBody};

/// The displayed response body. Raw text always uses the virtual viewer, which
/// stays responsive for large responses; pretty JSON uses the highlighted editor.
pub(super) enum Body {
    Raw {
        view: Entity<VirtualBody>,
        search: Option<BodySearch>,
    },
    Pretty(Entity<ResponseBodyEditor>),
}

#[cfg(test)]
impl Body {
    pub(super) fn raw(&self) -> &Entity<VirtualBody> {
        match self {
            Body::Raw { view, .. } => view,
            Body::Pretty(_) => panic!("expected the raw body"),
        }
    }

    pub(super) fn search(&mut self) -> &mut BodySearch {
        match self {
            Body::Raw {
                search: Some(search),
                ..
            } => search,
            _ => panic!("expected an open raw body search"),
        }
    }
}

/// Cache the editor independently so selecting response details does not lay
/// out and paint an unchanged (potentially large) response body again.
pub(crate) struct ResponseBodyEditor(pub(crate) Entity<EditorState>);

impl Render for ResponseBodyEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Editor::new(&self.0)
            .h_full()
            .readonly(true)
            .appearance(false)
            .bordered(false)
            .bg(cx
                .theme()
                .highlight_theme
                .style
                .editor_background
                .unwrap_or_else(|| cx.theme().input_background()))
            .text_sm()
            .aria_label("Response body")
    }
}

impl ResponseView {
    pub(super) fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = self.content.as_ref().unwrap();
        let body = self.body.as_ref().unwrap();
        let pretty = matches!(body, Body::Pretty(_));
        let has_json = content.pretty.is_some();
        let view = cx.entity().downgrade();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .when(!pretty, |view| {
                view.capture_action(cx.listener(
                    |this, _: &gpui_kit::base::input::Search, window, cx| {
                        this.open_response_search(window, cx);
                        cx.stop_propagation();
                    },
                ))
            })
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .gap_2()
                    .child(
                        Button::new("response-format")
                            .debug_selector(|| "response-format".into())
                            .disabled(content.raw_only)
                            .ghost()
                            .small()
                            .label(if pretty { "Pretty" } else { "Raw" })
                            .when(!content.raw_only, |button| {
                                button.icon(IconName::ChevronDown)
                            })
                            .dropdown_menu(move |menu, _, _| {
                                let raw_view = view.clone();
                                let json_view = view.clone();
                                menu.item(PopupMenuItem::new("Raw").on_click(
                                    move |_, window, cx| {
                                        let _ = raw_view.update(cx, |view, cx| {
                                            view.set_pretty(false, window, cx)
                                        });
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new("Pretty").disabled(!has_json).on_click(
                                        move |_, window, cx| {
                                            let _ = json_view.update(cx, |view, cx| {
                                                view.set_pretty(true, window, cx)
                                            });
                                        },
                                    ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match content.language {
                                "json" => "JSON",
                                "html" => "HTML",
                                _ => "Text",
                            }),
                    )
                    .when(content.raw_only, |row| {
                        row.child(
                            div()
                                .debug_selector(|| "response-raw-only".into())
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Large response · Raw only"),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("response-wrap")
                            .debug_selector(|| "response-wrap".into())
                            .ghost()
                            .small()
                            .label("Wrap")
                            .selected(self.wrap)
                            .accessibility_label("Toggle response line wrapping")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.wrap = !this.wrap;
                                match &this.body {
                                    Some(Body::Raw { view, .. }) => {
                                        view.update(cx, |view, cx| view.set_wrap(this.wrap, cx));
                                    }
                                    Some(Body::Pretty(editor)) => {
                                        let editor = editor.read(cx).0.clone();
                                        editor.update(cx, |editor, cx| {
                                            editor.set_soft_wrap(this.wrap, window, cx)
                                        });
                                    }
                                    None => {}
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("response-search")
                            .ghost()
                            .small()
                            .icon(IconName::Search)
                            .accessibility_label("Search response")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_response_search(window, cx);
                            })),
                    )
                    .child(
                        Button::new("response-copy")
                            .debug_selector(|| "response-copy".into())
                            .ghost()
                            .small()
                            .icon(IconName::Copy)
                            .accessibility_label("Copy response body")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(content) = &this.content {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        String::from_utf8_lossy(&content.http().body).into_owned(),
                                    ));
                                }
                            })),
                    ),
            )
            .when_some(
                match body {
                    Body::Raw { search, .. } => search.as_ref(),
                    Body::Pretty(_) => None,
                },
                |view, search| view.child(Self::body_search_bar(search, cx)),
            )
            .child(
                div()
                    .debug_selector(|| "response-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        match body {
                            Body::Raw { view, .. } => AnyView::from(view.clone()),
                            Body::Pretty(editor) => editor.clone().into(),
                        }
                        .cached(StyleRefinement::default().size_full()),
                    ),
            )
            .into_any_element()
    }

    /// Show the pretty JSON editor when requested and available, else raw text.
    pub(super) fn set_pretty(&mut self, pretty: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = &self.content else {
            return;
        };
        let wrap = self.wrap;

        self.body = Some(match content.pretty.clone().filter(|_| pretty) {
            Some(text) => {
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language(content.language)
                        .line_number(true)
                        .soft_wrap(wrap)
                        .searchable(true)
                        .replaceable(false)
                        .default_value(text)
                });
                Body::Pretty(cx.new(|_| ResponseBodyEditor(editor)))
            }
            None => Body::Raw {
                view: cx.new(|cx| VirtualBody::new(content.raw.clone(), wrap, cx)),
                search: None,
            },
        });
        cx.notify();
    }
}
