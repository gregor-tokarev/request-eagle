use std::ops::Range;

use aho_corasick::AhoCorasickBuilder;
use gpui_kit::component::{
    button::*,
    input::{Enter, Escape, Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;

use super::{body::Body, view::ResponseView};

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
        match &mut self.body {
            Some(Body::Raw { search, .. }) => {
                let search = search.get_or_insert_with(|| {
                    let input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("Search response"));
                    let subscription =
                        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
                            if matches!(event, InputEvent::Change) {
                                this.update_body_search(cx);
                            }
                        });

                    BodySearch {
                        input,
                        matches: Vec::new(),
                        current: 0,
                        query_len: 0,
                        case_sensitive: false,
                        _subscription: subscription,
                    }
                });
                search.input.update(cx, |input, cx| {
                    input.select_all(window, cx);
                    input.focus(window, cx);
                });
                cx.notify();
            }
            Some(Body::Pretty(editor)) => {
                let editor = editor.read(cx).0.clone();
                editor.update(cx, |editor, cx| editor.open_search(false, cx));
            }
            _ => {}
        }
    }

    fn update_body_search(&mut self, cx: &mut Context<Self>) {
        let Some(Body::Raw {
            view,
            search: Some(search),
        }) = &mut self.body
        else {
            return;
        };

        search.update_query(&view.read(cx).source, &search.input.read(cx).value());
        let range = search.current_range();
        view.update(cx, |view, cx| view.select_match(range, cx));
        cx.notify();
    }

    fn move_body_match(&mut self, previous: bool, cx: &mut Context<Self>) {
        let Some(Body::Raw {
            view,
            search: Some(search),
        }) = &mut self.body
        else {
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
        let range = search.current_range();
        view.update(cx, |view, cx| view.select_match(range, cx));
        cx.notify();
    }

    fn close_body_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Body::Raw { view, search }) = &mut self.body {
            *search = None;
            view.update(cx, |view, cx| {
                view.select_match(None, cx);
                window.focus(&view.focus, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn body_search_bar(
        search: &BodySearch,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        h_flex()
            .debug_selector(|| "response-body-search".into())
            .gap_1()
            .capture_action(cx.listener(|this, action: &Enter, _, cx| {
                this.move_body_match(action.shift, cx);
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(Body::Raw {
                            search: Some(search),
                            ..
                        }) = &mut this.body
                        {
                            search.case_sensitive = !search.case_sensitive;
                        }
                        this.update_body_search(cx);
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
                    .on_click(cx.listener(|this, _, _, cx| this.move_body_match(true, cx))),
            )
            .child(
                Button::new("response-search-next")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronRight)
                    .accessibility_label("Next match")
                    .disabled(search.matches.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.move_body_match(false, cx))),
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
