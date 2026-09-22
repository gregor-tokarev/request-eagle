use super::super::request_fields::{FieldsChanged, RequestFields};
use collection::{HttpRequest, Method};
use gpui_kit::component::resizable::{ResizableState, resizable_panel, v_resizable};
use gpui_kit::component::{
    input::{EditorState, InputEvent, InputState},
    *,
};
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum RequestSection {
    Params,
    Headers,
    Body,
    Authentication,
}

/// An editable request snapshot owned by one tab, independent of the collections registry.
pub(in crate::layout) struct RequestDraft {
    pub(in crate::layout) name: SharedString,
    pub(in crate::layout) collection: Option<SharedString>,
    pub(in crate::layout) folders: Vec<SharedString>,
    pub(in crate::layout) request: HttpRequest,
    pub(in crate::layout) saved_request: HttpRequest,
    pub(in crate::layout) environment_path: Option<std::path::PathBuf>,
    pub(in crate::layout) url: Option<Entity<InputState>>,
    pub(in crate::layout) section: RequestSection,
    pub(super) params: Option<Entity<RequestFields>>,
    pub(super) headers: Option<Entity<RequestFields>>,
    pub(super) generated_headers: Vec<(String, String)>,
    pub(super) body: Option<Entity<EditorState>>,
    pub(super) url_form: Option<Entity<super::form_body::FormEditor>>,
    pub(super) multipart_form: Option<Entity<super::form_body::FormEditor>>,
    pub(super) authentication_editor: Option<Entity<super::authentication::AuthenticationEditor>>,
    pub(super) response: Option<Entity<super::super::response_view::ResponseView>>,
    pub(super) split: Option<Entity<ResizableState>>,
    pub(super) task: Option<Task<()>>,
    pub(super) executor: Option<(request::RequestPreferences, request::RequestExecutor)>,
    address_view: Option<Entity<RequestAddress>>,
    configuration_view: Option<Entity<RequestConfiguration>>,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl RequestDraft {
    #[cfg(test)]
    pub(in crate::layout) fn response_for_test(
        &self,
    ) -> Entity<super::super::response_view::ResponseView> {
        self.response.as_ref().expect("prepared response").clone()
    }

    pub(in crate::layout) fn new() -> Self {
        Self {
            name: "Untitled Request".into(),
            collection: None,
            folders: Vec::new(),
            request: HttpRequest::default(),
            saved_request: HttpRequest::default(),
            environment_path: None,
            url: None,
            section: RequestSection::Headers,
            params: None,
            headers: None,
            generated_headers: super::execution::generated_headers(&HttpRequest::default()),
            body: None,
            url_form: None,
            multipart_form: None,
            authentication_editor: None,
            response: None,
            split: None,
            task: None,
            executor: None,
            address_view: None,
            configuration_view: None,
            _subscriptions: Vec::new(),
        }
    }

    pub(in crate::layout) fn from_saved(
        name: SharedString,
        collection: SharedString,
        request: HttpRequest,
    ) -> Self {
        Self {
            name,
            collection: Some(collection),
            saved_request: request.clone(),
            request,
            ..Self::new()
        }
    }

    pub(in crate::layout) fn is_dirty(&self) -> bool {
        self.request != self.saved_request
    }

    pub(in crate::layout) fn mark_saved(&mut self, request: HttpRequest, cx: &mut Context<Self>) {
        self.saved_request = request;
        cx.notify();
    }

    pub(in crate::layout) fn set_method(&mut self, method: Method, cx: &mut Context<Self>) {
        self.request.method = method;
        self.refresh_generated_headers(cx);

        if !self.supports_body() && self.section == RequestSection::Body {
            self.section = RequestSection::Headers;
        }

        cx.notify();
    }

    pub(in crate::layout) fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Initialize newly activated controls before drawing. Their setup can
        // notify GPUI; doing it inside render schedules an unnecessary frame.
        self.url_state(window, cx);

        match self.section {
            RequestSection::Body => {
                self.prepare_body(window, cx);
            }
            RequestSection::Authentication => {
                self.authentication_state(window, cx);
            }
            RequestSection::Params | RequestSection::Headers => {
                self.fields_state(window, cx);
            }
        }

        self.refresh_generated_headers(cx);

        self.response
            .get_or_insert_with(|| cx.new(|cx| super::super::response_view::ResponseView::new(cx)));
        self.split
            .get_or_insert_with(|| cx.new(|_| ResizableState::default()));
    }

    pub(super) fn url_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
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
                            this.refresh_generated_headers(cx);
                            cx.notify();
                        }
                    },
                ));

                url
            })
            .clone()
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
            let values = if is_headers {
                self.request.headers.as_slice()
            } else {
                self.request.query.as_deref().unwrap_or_default()
            };
            let generated = if is_headers {
                self.generated_headers.as_slice()
            } else {
                &[]
            };
            let fields = cx.new(|cx| RequestFields::new(id, values, generated, window, cx));
            let subscription = cx.subscribe(&fields, move |this, _, event: &FieldsChanged, cx| {
                if is_headers {
                    this.request.headers = event.0.clone();
                    this.refresh_generated_headers(cx);
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
}

impl Render for RequestDraft {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The first tab can render before it is focused. These panel states do
        // not install window listeners or notify during construction.
        self.response
            .get_or_insert_with(|| cx.new(|cx| super::super::response_view::ResponseView::new(cx)));
        self.split
            .get_or_insert_with(|| cx.new(|_| ResizableState::default()));

        let owner = cx.entity().downgrade();
        let address = self
            .address_view
            .get_or_insert_with(|| cx.new(|_| RequestAddress(owner.clone())))
            .clone();
        let configuration = self
            .configuration_view
            .get_or_insert_with(|| cx.new(|_| RequestConfiguration(owner)))
            .clone();

        v_flex()
            .debug_selector(|| "request-draft".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_2()
            .gap_2()
            .text_size(px(13.))
            .child(address.cached(StyleRefinement::default().w_full().h(px(88.)).flex_none()))
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    v_resizable("request-response-split")
                        .with_state(self.split.as_ref().expect("prepared request split"))
                        .child(
                            resizable_panel()
                                .size(px(330.))
                                .size_range(px(110.)..px(1200.))
                                .child(
                                    configuration.cached(StyleRefinement::default().size_full()),
                                ),
                        )
                        .child(
                            self.response
                                .as_ref()
                                .expect("prepared response")
                                .clone()
                                .into_any_element(),
                        ),
                ),
            )
    }
}

// Cache the editable controls independently of the response. Notifications
// from their draft or input states invalidate these views, while selecting a
// response does not redraw the address bar and request fields.
struct RequestAddress(WeakEntity<RequestDraft>);

impl Render for RequestAddress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(draft.header(cx))
                    .child(draft.url_bar(window, cx))
            })
            .unwrap_or_else(|_| div())
    }
}

struct RequestConfiguration(WeakEntity<RequestDraft>);

impl Render for RequestConfiguration {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |draft, cx| {
                let content = match draft.section {
                    RequestSection::Headers | RequestSection::Params => draft.fields(window, cx),
                    RequestSection::Body => draft.body(window, cx),
                    RequestSection::Authentication => draft.authentication(window, cx),
                };

                v_flex()
                    .size_full()
                    .min_h_0()
                    .min_w_0()
                    .gap_2()
                    .pb_3()
                    .child(draft.section_tabs(cx))
                    .child(
                        div()
                            .id("request-section-content")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(content),
                    )
            })
            .unwrap_or_else(|_| div())
    }
}
