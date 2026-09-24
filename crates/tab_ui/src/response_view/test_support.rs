use gpui_kit::{App, FocusHandle};

use super::ResponseView;

impl ResponseView {
    pub fn focus_for_test(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn search_query_for_test(&self, cx: &App) -> String {
        if let Some(editor) = &self.editor {
            let search = editor.read(cx).search_session();
            assert!(search.open);
            search.query.to_string()
        } else {
            let search = self.body_search.as_ref().expect("response search is open");
            assert!(!search.matches.is_empty());
            search.input.read(cx).value().to_string()
        }
    }
}
