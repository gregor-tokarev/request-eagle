use gpui_kit::*;

use super::main_view::MainView;
use crate::history::HistoryEntry;

impl MainView {
    pub(super) fn record_history(&mut self, entry: HistoryEntry, cx: &mut Context<Self>) {
        self.history.push(entry);
        self.save_history(cx);
        cx.notify();
    }

    pub(super) fn clear_history(&mut self, cx: &mut Context<Self>) {
        if self.history.clear() {
            self.save_history(cx);
            cx.notify();
        }
    }

    pub(super) fn save_history(&mut self, cx: &mut Context<Self>) {
        let Some(write) = self.history.checkpoint() else {
            return;
        };
        let revision = write.revision();
        let task = cx.background_executor().spawn(async move { write.write() });

        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if !this.history.finish_write(revision, result.is_ok()) {
                    return;
                }

                this.history_error = result
                    .err()
                    .map(|error| format!("Could not save request history: {error}"));
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn flush_history(&mut self) {
        if let Err(error) = self.history.flush() {
            self.history_error = Some(format!("Could not save request history: {error}"));
        }
    }
}
