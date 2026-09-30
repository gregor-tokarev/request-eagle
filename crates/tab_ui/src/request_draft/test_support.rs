use gpui_kit::{App, Context, Entity, component::input::InputState};

use super::RequestDraft;
use crate::response_view::ResponseView;

impl RequestDraft {
    /// The URL editor, created when the draft is first prepared.
    pub fn url_input(&self) -> Option<&Entity<InputState>> {
        self.url.as_ref()
    }

    pub fn is_sending(&self) -> bool {
        self.task.is_some()
    }

    pub fn response_for_test(&self) -> Entity<ResponseView> {
        self.response.clone()
    }

    pub fn body_text_for_test(&self, cx: &App) -> Option<String> {
        self.body
            .as_ref()
            .map(|body| body.read(cx).value().to_string())
    }

    pub fn hold_request_for_test(&mut self, cx: &mut Context<Self>) {
        self.task = Some(cx.spawn(async |_, _| std::future::pending::<()>().await));
        cx.notify();
    }
}
