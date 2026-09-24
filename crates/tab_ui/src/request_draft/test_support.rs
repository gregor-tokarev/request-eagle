use gpui_kit::{App, Context};

use super::RequestDraft;

impl RequestDraft {
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
