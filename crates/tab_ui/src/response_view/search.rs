use std::ops::Range;

use aho_corasick::AhoCorasickBuilder;
use gpui_kit::component::{
    button::*,
    input::{Enter, Escape, Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;

use super::view::ResponseView;

pub(super) struct BodySearch {
    pub(super) input: Entity<InputState>,
    pub(super) matches: Vec<usize>,
    current: usize,
    query_len: usize,
    case_sensitive: bool,
    _subscription: Subscription,
}

impl BodySearch {
    fn update_query(&mut self, text: &str, query: &str) {
        // Reuse offsets across keystrokes. Searching a large body must not
        // copy its text or allocate a new list of ranges for every character.
        self.matches.clear();
        self.current = 0;
        self.query_len = query.len();

        if query.is_empty() {
            return;
        }

        let matcher = AhoCorasickBuilder::new()
            .ascii_case_insensitive(!self.case_sensitive)
            .build([query])
            .expect("response search query");
        self.matches
            .extend(matcher.find_iter(text).map(|found| found.start()));
    }

    fn current_range(&self) -> Option<Range<usize>> {
        self.matches
            .get(self.current)
            .map(|&start| start..start + self.query_len)
    }

    fn label(&self) -> String {
        if self.matches.is_empty() {
            "0/0".into()
        } else {
            format!("{}/{}", self.current + 1, self.matches.len())
        }
    }
}

impl ResponseView {
    pub(super) fn open_response_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.virtual_body.is_none() {
            if let Some(editor) = &self.editor {
                editor.update(cx, |editor, cx| editor.open_search(false, cx));
            }
            return;
        }

        if self.body_search.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search response"));
            let subscription =
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.update_body_search(window, cx);
                    }
                });
            self.body_search = Some(BodySearch {
                input,
                matches: Vec::new(),
                current: 0,
                query_len: 0,
                case_sensitive: false,
                _subscription: subscription,
            });
        }

        self.body_search
            .as_ref()
            .unwrap()
            .input
            .update(cx, |input, cx| {
                input.select_all(window, cx);
                input.focus(window, cx);
            });
        cx.notify();
    }

    fn update_body_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(search) = &mut self.body_search else {
            return;
        };
        let Some(body) = &self.virtual_body else {
            return;
        };
        search.update_query(&body.read(cx).source, &search.input.read(cx).value());
        self.select_body_match(window, cx);
    }

    fn move_body_match(&mut self, previous: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(search) = &mut self.body_search else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }
        if previous {
            search.current = if search.current == 0 {
                search.matches.len() - 1
            } else {
                search.current - 1
            };
        } else {
            search.current = (search.current + 1) % search.matches.len();
        }
        self.select_body_match(window, cx);
    }

    fn select_body_match(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let search = self.body_search.as_ref().unwrap();
        let range = search.current_range();
        if let Some(body) = &self.virtual_body {
            body.update(cx, |body, cx| body.select_match(range, cx));
        }
        cx.notify();
    }

    fn close_body_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.body_search = None;
        if let Some(body) = &self.virtual_body {
            body.update(cx, |body, cx| {
                body.select_match(None, cx);
                window.focus(&body.focus, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn body_search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let search = self.body_search.as_ref().unwrap();
        h_flex()
            .debug_selector(|| "response-body-search".into())
            .gap_1()
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                this.move_body_match(action.shift, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                this.close_body_search(window, cx);
                cx.stop_propagation();
            }))
            .child(
                Input::new(&search.input)
                    .small()
                    .flex_1()
                    .min_w_0()
                    .aria_label("Search response"),
            )
            .child(
                Button::new("response-search-case")
                    .ghost()
                    .small()
                    .label("Aa")
                    .selected(search.case_sensitive)
                    .tooltip("Match case")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(search) = &mut this.body_search {
                            search.case_sensitive = !search.case_sensitive;
                        }
                        this.update_body_search(window, cx);
                    })),
            )
            .child(div().text_xs().child(search.label()))
            .child(
                Button::new("response-search-previous")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .accessibility_label("Previous match")
                    .disabled(search.matches.is_empty())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.move_body_match(true, window, cx)),
                    ),
            )
            .child(
                Button::new("response-search-next")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronRight)
                    .accessibility_label("Next match")
                    .disabled(search.matches.is_empty())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.move_body_match(false, window, cx)),
                    ),
            )
            .child(
                Button::new("response-search-close")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .accessibility_label("Close search")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_body_search(window, cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::ResponseView;
    use crate::response_view::virtual_body::VirtualBody;
    use gpui_kit::{AppContext as _, SharedString, TestAppContext};

    #[gpui_kit::test]
    fn search_borrows_large_text_and_preserves_navigation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            preferences::init(cx);
            request_eagle_theme::init(cx);
        });
        let prefix = "large response row ".repeat(700_000);
        let first = prefix.len();
        let source: SharedString = format!("{prefix}Needle ☃ / needle ☃ / Needle ☃").into();
        let query = "Needle ☃";
        let step = query.len() + " / ".len();

        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut view = ResponseView::new(cx);
            view.virtual_body = Some(cx.new(|cx| VirtualBody::new(source.clone(), true, cx)));
            view.open_response_search(window, cx);
            view
        });

        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let search = view.body_search.as_mut().unwrap();
                let allocated = crate::test_allocator::allocated_by(|| {
                    search.update_query(&source, query);
                });
                assert!(allocated < 128 * 1024, "search allocated {allocated} bytes");
                assert_eq!(search.matches, [first, first + step, first + 2 * step]);
                assert_eq!(search.label(), "1/3");

                view.move_body_match(true, window, cx);
                assert_eq!(view.body_search.as_ref().unwrap().label(), "3/3");
                view.move_body_match(false, window, cx);
                let body = view.virtual_body.as_ref().unwrap().read(cx);
                assert_eq!(body.selection, first..first + query.len());

                let search = view.body_search.as_mut().unwrap();
                let buffer = search.matches.as_ptr();
                search.case_sensitive = true;
                search.update_query(&source, query);
                assert_eq!(search.matches, [first, first + 2 * step]);
                assert_eq!(search.matches.as_ptr(), buffer);

                for query in ["absent", ""] {
                    search.update_query(&source, query);
                    assert!(search.current_range().is_none());
                    assert_eq!(search.label(), "0/0");
                }
            });
        });
    }
}
