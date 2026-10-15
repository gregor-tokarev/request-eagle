use gpui_kit::base::TextSelection;
use gpui_kit::*;

use super::draft::GrpcDraft;
use crate::cite::{AddToAgent, CiteOrigin, CiteSource, CiteToAgent, Quote, editor_focused};

impl GrpcDraft {
    /// Quote what has focus in the agent's message: the selection in the
    /// server URL, the message, a script or the response, or the whole call.
    pub(super) fn add_to_agent(
        &mut self,
        _: &AddToAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(quote) = self.quote(window, cx) else {
            cx.propagate();
            return;
        };

        let cite = quote.cite(CiteOrigin {
            page: cx.entity_id(),
            name: self
                .storage
                .name()
                .unwrap_or_else(|| "Unsaved request".into()),
            request: format!("gRPC {} {}", self.request.url, self.request.method),
            status: None,
            file: self
                .storage
                .location()
                .map(|location| location.path.clone()),
            unsaved_changes: self.is_dirty(),
        });
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

        if let Some(message) = self
            .message
            .as_ref()
            .filter(|message| editor_focused(message, self.message_vim.as_ref(), window, cx))
        {
            let message = message.read(cx);
            return Quote::of(
                CiteSource::Message,
                &message.value(),
                message.selected_range(),
                Some("json"),
            );
        }

        if let Some(quote) = self
            .scripts
            .as_ref()
            .and_then(|scripts| scripts.read(cx).quote(window, cx))
        {
            return Some(quote);
        }

        // The messages and metadata received are selectable text.
        if TextSelection::has_selection(window, cx)
            && let Some(quote) = Quote::of(
                CiteSource::Response,
                &TextSelection::selected_text(window, cx),
                0..0,
                None,
            )
        {
            return Some(quote);
        }

        let call = format!(
            "gRPC {} {}\n\n{}",
            self.request.url, self.request.method, self.request.message
        );
        Quote::of(CiteSource::Request, call.trim_end(), 0..0, None)
    }
}
