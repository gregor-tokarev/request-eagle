use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{input::EditorState, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ExecutionError;

use super::content::{ResponseContent, size_label};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Body,
    Cookies,
    Headers,
}

pub(in crate::layout) struct ResponseView {
    focus: FocusHandle,
    pub(super) content: Option<ResponseContent>,
    pub(super) editor: Option<Entity<EditorState>>,
    pub(super) message: SharedString,
    pub(super) loading: bool,
    error: bool,
    section: Section,
    pub(super) pretty: bool,
    pub(super) wrap: bool,
}

impl ResponseView {
    pub(in crate::layout) fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: None,
            editor: None,
            message: "Send a request to see the response".into(),
            loading: false,
            error: false,
            section: Section::Body,
            pretty: false,
            wrap: true,
        }
    }

    pub(in crate::layout) fn start(&mut self, cx: &mut Context<Self>) {
        self.content = None;
        self.editor = None;
        self.loading = true;
        self.error = false;
        self.message = "Sending request…".into();
        cx.notify();
    }

    pub(in crate::layout) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.loading = false;
        self.message = "Request cancelled".into();
        cx.notify();
    }

    pub(in crate::layout) fn finish(
        &mut self,
        result: Result<ResponseContent, ExecutionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading = false;

        match result {
            Ok(content) => {
                self.pretty = content.pretty.is_some();
                let text = content.pretty.as_ref().unwrap_or(&content.raw).clone();
                self.editor = Some(cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language(content.language)
                        .line_number(true)
                        .soft_wrap(self.wrap)
                        .replaceable(false)
                        .default_value(text)
                }));
                self.content = Some(content);
                self.error = false;
            }
            Err(error) => {
                self.content = None;
                self.editor = None;
                self.error = true;
                self.message = error.to_string().into();
            }
        }

        cx.notify();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let headers = self
            .content
            .as_ref()
            .map_or(0, |content| content.http().headers.len());
        let cookies = self.content.as_ref().map_or(0, |content| {
            content.http().headers.get_all("set-cookie").iter().count()
        });

        h_flex()
            .flex_none()
            .min_w_0()
            .min_h(px(44.))
            .gap_3()
            .flex_wrap()
            .child(
                Tabs::new("response-sections")
                    .flex()
                    .flex_row()
                    .flex_none()
                    .gap_1()
                    .children(
                        [
                            (Section::Body, "Body", 0),
                            (Section::Cookies, "Cookies", cookies),
                            (Section::Headers, "Headers", headers),
                        ]
                        .into_iter()
                        .map(|(section, label, count)| {
                            Tab::new(label)
                                .debug_selector(move || format!("response-section-{label}"))
                                .selected(self.section == section)
                                .h(px(30.))
                                .px_2()
                                .gap_2()
                                .rounded(px(4.))
                                .text_color(cx.theme().muted_foreground)
                                .when(self.section == section, |tab| {
                                    tab.bg(cx.theme().muted).text_color(cx.theme().foreground)
                                })
                                .child(label)
                                .when(count > 0, |tab| {
                                    tab.child(div().text_size(px(11.)).child(count.to_string()))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.section = section;
                                    cx.notify();
                                }))
                        }),
                    ),
            )
            .child(div().flex_1())
            .when_some(self.content.as_ref(), |row, content| {
                let response = content.http();
                let color = if response.status.is_success() {
                    cx.theme().success
                } else if response.status.is_redirection() {
                    cx.theme().warning
                } else {
                    cx.theme().danger
                };

                row.child(
                    h_flex()
                        .debug_selector(|| "response-metadata".into())
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            div()
                                .debug_selector(|| "response-status".into())
                                .px_2()
                                .py_1()
                                .rounded(px(5.))
                                .bg(color.opacity(0.15))
                                .text_color(color)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(response.status.to_string()),
                        )
                        .child("•")
                        .child(format!("{} ms", content.execution.elapsed.as_millis()))
                        .child("•")
                        .child(size_label(response.body.len()))
                        .child("•")
                        .child(format!("{:?}", response.version)),
                )
            })
    }
}

impl Render for ResponseView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.content.is_some() {
            match self.section {
                Section::Body => self.body(cx),
                Section::Headers => self.headers(false, cx),
                Section::Cookies => self.headers(true, cx),
            }
        } else {
            v_flex()
                .debug_selector(|| "response-empty".into())
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap_2()
                .px_6()
                .text_color(if self.error {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                })
                .when(self.loading, |view| {
                    view.child(Icon::new(IconName::Loader).size(px(20.)).with_animation(
                        "response-loading",
                        Animation::new(std::time::Duration::from_secs(1)).repeat(),
                        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                    ))
                })
                .child(self.message.clone())
                .into_any_element()
        };

        v_flex()
            .debug_selector(|| "response-panel".into())
            .track_focus(&self.focus)
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    window.focus(&this.focus, cx);
                }
            }))
            .size_full()
            .min_h_0()
            .min_w_0()
            .pt_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.toolbar(cx))
            .child(content)
    }
}
