use gpui_kit::*;

use super::view::{CallState, GrpcResponse};
use crate::cite::{CiteSource, Quote, editor_focused};

impl GrpcResponse {
    /// The selection in a unary call's response message, or the whole
    /// message, while it has focus.
    pub(crate) fn quote(&self, window: &Window, cx: &App) -> Option<Quote> {
        let body = self
            .body
            .as_ref()
            .filter(|body| editor_focused(body, None, window, cx))?
            .read(cx);

        Quote::of(
            CiteSource::ResponseBody,
            &body.value(),
            body.selected_range(),
            Some("json"),
        )
    }

    /// How the call ended, such as `16 UNAUTHENTICATED`.
    pub(crate) fn final_status(&self) -> Option<String> {
        match &self.state {
            CallState::Finished { status, .. } => {
                Some(format!("{} {}", status.code, status.name()))
            }
            _ => None,
        }
    }
}
