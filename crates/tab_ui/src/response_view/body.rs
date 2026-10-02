use std::sync::Once;

use gpui_kit::component::{
    button::*,
    highlighter::{LanguageConfig, LanguageRegistry},
    input::{Editor, EditorState},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{
    content::{Preview, ResponseContent},
    hex::{HEX_LIMIT, hex_dump},
    html::HtmlPreview,
    image::ImagePreview,
    metadata::size_label,
    pdf::PdfPreview,
    search::BodySearch,
    view::ResponseView,
    virtual_body::VirtualBody,
};

/// How the response body is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BodyMode {
    /// What the body represents: a page, an image or a document.
    Preview,
    /// Highlighted text, reformatted when it is JSON or XML.
    Pretty,
    Raw,
    /// The body's bytes.
    Hex,
}

impl BodyMode {
    fn label(self) -> &'static str {
        match self {
            Self::Preview => "Preview",
            Self::Pretty => "Pretty",
            Self::Raw => "Raw",
            Self::Hex => "Hex",
        }
    }
}

impl ResponseContent {
    /// The modes this body can be shown in.
    pub(super) fn modes(&self) -> Vec<BodyMode> {
        [
            (BodyMode::Preview, self.preview.is_some()),
            (BodyMode::Pretty, self.pretty.is_some()),
            (BodyMode::Raw, !self.binary),
            (BodyMode::Hex, true),
        ]
        .into_iter()
        .filter_map(|(mode, available)| available.then_some(mode))
        .collect()
    }

    /// Images and documents show as themselves; a page shows its source.
    pub(super) fn default_mode(&self) -> BodyMode {
        match self.preview {
            Some(Preview::Image(_) | Preview::Pdf) => BodyMode::Preview,
            _ if self.pretty.is_some() => BodyMode::Pretty,
            _ if !self.binary => BodyMode::Raw,
            _ => BodyMode::Hex,
        }
    }
}

/// The displayed response body. Raw text and hex dumps use the virtual
/// viewer, which stays responsive for large responses; pretty text uses the
/// highlighted editor.
pub(super) enum Body {
    Raw {
        view: Entity<VirtualBody>,
        search: Option<BodySearch>,
    },
    Pretty(Entity<ResponseBodyEditor>),
    Html(Entity<HtmlPreview>),
    Image(Entity<ImagePreview>),
    Pdf(Entity<PdfPreview>),
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
        let mode = self.mode;
        let modes = content.modes();
        let text = matches!(mode, BodyMode::Pretty | BodyMode::Raw);
        let image = match &content.preview {
            Some(Preview::Image(image)) => Some(image.clone()),
            _ => None,
        };
        let size = content.http().body.len();
        let pages = match body {
            Body::Pdf(pdf) => pdf.read(cx).page_count(),
            _ => None,
        };
        let view = cx.entity().downgrade();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .when(matches!(body, Body::Raw { .. }), |view| {
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
                            .disabled(modes.len() < 2)
                            .ghost()
                            .small()
                            .label(mode.label())
                            .when(modes.len() > 1, |button| button.icon(IconName::ChevronDown))
                            .dropdown_menu(move |menu, _, _| {
                                modes.iter().fold(menu, |menu, &option| {
                                    let view = view.clone();
                                    menu.item(
                                        PopupMenuItem::new(option.label())
                                            .checked(option == mode)
                                            .on_click(move |_, window, cx| {
                                                let _ = view.update(cx, |view, cx| {
                                                    view.show(option, window, cx)
                                                });
                                            }),
                                    )
                                })
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "response-kind".into())
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match pages {
                                Some(1) => "PDF · 1 page".into(),
                                Some(pages) => format!("PDF · {pages} pages").into(),
                                None => SharedString::from(content.label()),
                            }),
                    )
                    .when(content.raw_only && text, |row| {
                        row.child(
                            div()
                                .debug_selector(|| "response-raw-only".into())
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Large response · Raw only"),
                        )
                    })
                    .when(mode == BodyMode::Hex && size > HEX_LIMIT, |row| {
                        row.child(
                            div()
                                .debug_selector(|| "response-hex-limit".into())
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "First {} of {} · Save to keep all of it",
                                    size_label(HEX_LIMIT),
                                    size_label(size)
                                )),
                        )
                    })
                    .child(div().flex_1())
                    .when_some(self.saved.as_ref(), |row, saved| {
                        row.child(match saved {
                            Ok(path) => {
                                let path = path.clone();

                                Button::new("response-saved")
                                    .debug_selector(|| "response-saved".into())
                                    .ghost()
                                    .small()
                                    .min_w_0()
                                    .max_w(rems(16.))
                                    .label(format!(
                                        "Saved {}",
                                        path.file_name().unwrap_or_default().to_string_lossy()
                                    ))
                                    .tooltip("Show in folder")
                                    .on_click(move |_, _, cx| cx.reveal_path(&path))
                                    .into_any_element()
                            }
                            Err(error) => div()
                                .debug_selector(|| "response-save-error".into())
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(error.clone())
                                .into_any_element(),
                        })
                    })
                    .when(text, |row| {
                        row.child(
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
                                            view.update(cx, |view, cx| {
                                                view.set_wrap(this.wrap, cx)
                                            });
                                        }
                                        Some(Body::Pretty(editor)) => {
                                            let editor = editor.read(cx).0.clone();
                                            editor.update(cx, |editor, cx| {
                                                editor.set_soft_wrap(this.wrap, window, cx)
                                            });
                                        }
                                        _ => {}
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .when(text || mode == BodyMode::Hex, |row| {
                        row.child(
                            Button::new("response-search")
                                .ghost()
                                .small()
                                .icon(IconName::Search)
                                .accessibility_label("Search response")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_response_search(window, cx);
                                })),
                        )
                    })
                    .when(!content.binary || image.is_some(), |row| {
                        row.child(
                            Button::new("response-copy")
                                .debug_selector(|| "response-copy".into())
                                .ghost()
                                .small()
                                .icon(IconName::Copy)
                                .accessibility_label(if image.is_some() {
                                    "Copy image"
                                } else {
                                    "Copy response body"
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(image) = &image {
                                        cx.write_to_clipboard(ClipboardItem::new_image(image));
                                    } else if let Some(content) = &this.content {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            content.raw.to_string(),
                                        ));
                                    }
                                })),
                        )
                    })
                    .child(
                        Button::new("response-save")
                            .debug_selector(|| "response-save".into())
                            .ghost()
                            .small()
                            .icon(Icon::default().path("icons/download.svg"))
                            .accessibility_label("Save response to a file")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_body(window, cx);
                            })),
                    ),
            )
            .when_some(
                match body {
                    Body::Raw { search, .. } => search.as_ref(),
                    _ => None,
                },
                |view, search| view.child(Self::body_search_bar(search, cx)),
            )
            .child(
                div()
                    .debug_selector(|| "response-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(match body {
                        Body::Raw { view, .. } => AnyView::from(view.clone())
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element(),
                        Body::Pretty(editor) => AnyView::from(editor.clone())
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element(),
                        Body::Html(preview) => AnyView::from(preview.clone())
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element(),
                        Body::Image(preview) => AnyView::from(preview.clone())
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element(),
                        Body::Pdf(preview) => AnyView::from(preview.clone())
                            .cached(StyleRefinement::default().size_full())
                            .into_any_element(),
                    }),
            )
            .into_any_element()
    }

    /// Show the body in a mode. A mode the body does not have falls back to
    /// raw text, or to hex for a binary body.
    pub(super) fn show(&mut self, mode: BodyMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = &self.content else {
            return;
        };
        let mode = if content.modes().contains(&mode) {
            mode
        } else if !content.binary {
            BodyMode::Raw
        } else {
            BodyMode::Hex
        };
        let wrap = self.wrap;

        self.body = Some(match (mode, &content.preview) {
            (BodyMode::Preview, Some(Preview::Html)) => {
                let html = content.raw.clone();
                Body::Html(cx.new(|cx| HtmlPreview::new(html, cx)))
            }
            (BodyMode::Preview, Some(Preview::Image(image))) => {
                let image = image.clone();
                Body::Image(cx.new(|cx| ImagePreview::new(image, cx)))
            }
            (BodyMode::Preview, Some(Preview::Pdf)) => {
                let bytes = content.http().body.clone();
                Body::Pdf(cx.new(|cx| PdfPreview::new(bytes, cx)))
            }
            (BodyMode::Pretty, _) => {
                if content.language == "xml" {
                    register_xml();
                }
                let text = content.pretty.clone().unwrap_or_default();
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
            (BodyMode::Hex, _) => {
                let body = &content.http().body;
                let dump = hex_dump(&body[..body.len().min(HEX_LIMIT)]);
                Body::Raw {
                    view: cx.new(|cx| VirtualBody::new(dump.into(), false, cx)),
                    search: None,
                }
            }
            _ => Body::Raw {
                view: cx.new(|cx| VirtualBody::new(content.raw.clone(), wrap, cx)),
                search: None,
            },
        });
        self.mode = mode;
        cx.notify();
    }
}

/// GPUI Kit has no XML grammar of its own.
pub(crate) fn register_xml() {
    static REGISTER: Once = Once::new();

    REGISTER.call_once(|| {
        LanguageRegistry::singleton().register(
            "xml",
            &LanguageConfig::new(
                "xml",
                tree_sitter::Language::new(tree_sitter_xml::LANGUAGE_XML),
                Vec::new(),
                tree_sitter_xml::XML_HIGHLIGHT_QUERY,
                "",
                "",
            ),
        );
    });
}
