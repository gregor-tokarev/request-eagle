use gpui_kit::base::{ElementExt as _, Tab, Tabs, TextSelectionScopeId};
use gpui_kit::component::{input::EditorState, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ExecutionError;

use super::body::ResponseBodyEditor;
use super::content::ResponseContent;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Body,
    Cookies,
    Headers,
}

pub struct ResponseView {
    pub(super) focus: FocusHandle,
    pub(super) content: Option<ResponseContent>,
    pub(super) editor: Option<Entity<EditorState>>,
    pub(super) editor_view: Option<Entity<ResponseBodyEditor>>,
    pub(super) message: SharedString,
    pub(super) loading: bool,
    error: bool,
    section: Section,
    pub(super) pretty: bool,
    pub(super) wrap: bool,
    pub(super) virtual_body: Option<Entity<super::virtual_body::VirtualBody>>,
    pub(super) body_search: Option<super::search::BodySearch>,
    pub(super) headers_list: ListState,
    pub(super) cookies_list: ListState,
    pub(super) detail_open: [bool; 3],
    background_selection_scope: TextSelectionScopeId,
}

impl ResponseView {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: None,
            editor: None,
            editor_view: None,
            message: "Send a request to see the response".into(),
            loading: false,
            error: false,
            section: Section::Body,
            pretty: false,
            wrap: true,
            virtual_body: None,
            body_search: None,
            headers_list: ListState::new(0, ListAlignment::Top, px(0.)),
            cookies_list: ListState::new(0, ListAlignment::Top, px(0.)),
            detail_open: [false; 3],
            background_selection_scope: TextSelectionScopeId::new(),
        }
    }

    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        self.content = None;
        self.editor = None;
        self.editor_view = None;
        self.body_search = None;
        self.virtual_body = None;
        self.loading = true;
        self.error = false;
        self.message = "Sending request…".into();
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.loading = false;
        self.message = "Request cancelled".into();
        cx.notify();
    }

    pub fn finish(
        &mut self,
        result: Result<ResponseContent, ExecutionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading = false;
        self.body_search = None;
        self.virtual_body = None;

        match result {
            Ok(content) => {
                self.headers_list.reset(content.headers.len());
                self.cookies_list.reset(content.cookies.len());
                self.pretty = content.pretty.is_some();
                let text = content.pretty.as_ref().unwrap_or(&content.raw).clone();
                self.set_body_text(text, content.language, window, cx);
                self.content = Some(content);
                self.error = false;
            }
            Err(error) => {
                self.content = None;
                self.editor = None;
                self.editor_view = None;
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
            .when(self.content.is_some(), |row| row.child(self.metadata(cx)))
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
            .key_context("Response")
            .track_focus(&self.focus)
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
            }))
            // Plain SelectableText updates its selection without invalidating
            // the owning view. Paint the changing highlight while dragging.
            .on_mouse_move(cx.listener(|_, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    cx.notify();
                }
            }))
            .size_full()
            .min_h_0()
            .min_w_0()
            .pt_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(self.content.is_some(), |view| view.child(self.toolbar(cx)))
            .child(content)
            // Root owns the active scope. While a hover card is open, exclude
            // the response behind it from that scope; the card opts back in.
            .text_selection_scope(if self.detail_open.iter().any(|open| *open) {
                self.background_selection_scope
            } else {
                TextSelectionScopeId::default()
            })
    }
}
