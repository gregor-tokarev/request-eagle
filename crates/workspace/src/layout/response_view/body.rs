use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::view::ResponseView;

/// Cache the editor independently so selecting response details does not lay
/// out and paint an unchanged (potentially large) response body again.
pub(super) struct ResponseBodyEditor(pub(super) Entity<EditorState>);

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
            .text_size(px(13.))
            .aria_label("Response body")
    }
}

impl ResponseView {
    pub(super) fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = self.content.as_ref().unwrap();
        let view = cx.entity().downgrade();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .when(self.virtual_body.is_some(), |view| {
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
                    .h(px(30.))
                    .gap_2()
                    .child(
                        Button::new("response-format")
                            .ghost()
                            .small()
                            .label(if self.pretty { "JSON" } else { "Raw" })
                            .icon(IconName::ChevronDown)
                            .dropdown_menu(move |menu, _, cx| {
                                let raw_view = view.clone();
                                let json_view = view.clone();
                                let has_json = view.upgrade().is_some_and(|view| {
                                    view.read(cx)
                                        .content
                                        .as_ref()
                                        .is_some_and(|content| content.pretty.is_some())
                                });
                                menu.item(PopupMenuItem::new("Raw").on_click(
                                    move |_, window, cx| {
                                        let _ = raw_view.update(cx, |view, cx| {
                                            view.set_pretty(false, window, cx)
                                        });
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new("JSON").disabled(!has_json).on_click(
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
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(match content.language {
                                "json" => "JSON",
                                "html" => "HTML",
                                _ => "Text",
                            }),
                    )
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
                                if let Some(editor) = &this.editor {
                                    editor.update(cx, |editor, cx| {
                                        editor.set_soft_wrap(this.wrap, window, cx)
                                    });
                                }
                                if let Some(body) = &this.virtual_body {
                                    body.update(cx, |body, cx| body.set_wrap(this.wrap, cx));
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
            .when(self.body_search.is_some(), |view| {
                view.child(self.body_search_bar(cx))
            })
            .child(
                div()
                    .debug_selector(|| "response-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(if let Some(body) = &self.virtual_body {
                        body.clone()
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element()
                    } else {
                        self.editor_view
                            .as_ref()
                            .unwrap()
                            .clone()
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    pub(super) fn set_body_text(
        &mut self,
        text: SharedString,
        language: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.body_search = None;
        self.editor = None;
        self.editor_view = None;
        self.virtual_body = None;

        // Moderate HTML responses often contain script lines just over 16 KiB.
        // Keep the standard editor's highlighting and input behavior for these;
        // larger individual lines still incur expensive wrapping/search layouts.
        if text.len() > 256 * 1024 || text.split('\n').any(|line| line.len() > 32 * 1024) {
            self.virtual_body =
                Some(cx.new(|cx| super::virtual_body::VirtualBody::new(text, self.wrap, cx)));
        } else {
            let editor = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language(language)
                    .line_number(true)
                    .soft_wrap(self.wrap)
                    .searchable(true)
                    .replaceable(false)
                    .default_value(text)
            });
            self.editor_view = Some(cx.new(|_| ResponseBodyEditor(editor.clone())));
            self.editor = Some(editor);
        }
    }

    pub(super) fn set_pretty(&mut self, pretty: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(content) = &self.content {
            self.pretty = pretty && content.pretty.is_some();
            let text = if self.pretty {
                content.pretty.as_ref().unwrap()
            } else {
                &content.raw
            }
            .clone();
            let language = content.language;
            self.set_body_text(text, language, window, cx);
        }
        cx.notify();
    }
}
