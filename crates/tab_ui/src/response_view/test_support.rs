use gpui_kit::{App, FocusHandle};

use super::{ResponseView, body::Body};

impl ResponseView {
    pub fn focus_for_test(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The query of the raw response search, which must have matches.
    pub fn search_query_for_test(&self, cx: &App) -> String {
        let Some(Body::Raw {
            search: Some(search),
            ..
        }) = &self.body
        else {
            panic!("raw response search is open");
        };

        assert!(!search.matches.is_empty());
        search.input.read(cx).value().to_string()
    }
}
