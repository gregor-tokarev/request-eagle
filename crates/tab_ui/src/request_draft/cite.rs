use gpui_kit::*;
use request::{Body, RawLanguage};

use super::draft::RequestDraft;
use crate::cite::{
    AddToAgent, CiteOrigin, CiteSource, CiteToAgent, Quote, editor_focused, http_message,
};

impl RequestDraft {
    /// Quote what has focus in the agent's message: the selection in the
    /// URL, the body, a script or the response, or the whole request.
    pub(super) fn add_to_agent(
        &mut self,
        _: &AddToAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(quote) = self.quote(window, cx) else {
            // The workspace still shows the agent.
            cx.propagate();
            return;
        };

        let cite = quote.cite(self.cite_origin(cx));
        window.dispatch_action(Box::new(CiteToAgent(cite)), cx);
    }

    fn quote(&self, window: &mut Window, cx: &mut App) -> Option<Quote> {
        if let Some(url) = self
            .url
            .as_ref()
            .filter(|url| url.focus_handle(cx).is_focused(window))
        {
            let url = url.read(cx);
            return Quote::of(CiteSource::Url, &url.value(), url.selected_range(), None);
        }

        if let Some(body) = self
            .body
            .as_ref()
            .filter(|body| editor_focused(body, self.body_vim.as_ref(), window, cx))
        {
            let language = match &self.request.body {
                Some(Body::Raw {
                    language: RawLanguage::Json,
                    ..
                }) => Some("json"),
                Some(Body::Raw {
                    language: RawLanguage::Xml,
                    ..
                }) => Some("xml"),
                _ => None,
            };
            let body = body.read(cx);
            return Quote::of(
                CiteSource::RequestBody,
                &body.value(),
                body.selected_range(),
                language,
            );
        }

        if let Some(quote) = self
            .scripts
            .as_ref()
            .and_then(|scripts| scripts.read(cx).quote(window, cx))
        {
            return Some(quote);
        }

        if self.response.read(cx).contains_focus(window, cx)
            && let Some(quote) = self
                .response
                .update(cx, |response, cx| response.quote(window, cx))
        {
            return Some(quote);
        }

        Quote::of(
            CiteSource::Request,
            &http_message(&self.request),
            0..0,
            None,
        )
    }

    fn cite_origin(&self, cx: &Context<Self>) -> CiteOrigin {
        CiteOrigin {
            page: cx.entity_id(),
            name: self
                .storage
                .name()
                .unwrap_or_else(|| "Unsaved request".into()),
            request: format!("{} {}", self.request.method.as_str(), self.request.path),
            status: self.response.read(cx).status(),
            file: self
                .storage
                .location()
                .map(|location| location.path.clone()),
            unsaved_changes: self.is_dirty(),
        }
    }
}
