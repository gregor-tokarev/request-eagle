use std::ops::Range;

use aho_corasick::AhoCorasickBuilder;
use gpui_kit::component::{
    button::*,
    input::{Enter, Escape, Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;

use super::body::Body;
use super::view::{ResponseState, ResponseView};
use super::virtual_body::VirtualBody;

pub(super) struct BodySearch {
    pub(super) input: Entity<InputState>,
    case_sensitive: bool,
    _subscription: Subscription,
}

/// Where a query occurs in a body, and which occurrence is current.
#[derive(Default)]
pub(super) struct Matches {
    starts: Vec<usize>,
    len: usize,
    current: usize,
}

impl Matches {
    pub(super) fn update(&mut self, text: &str, query: &str, case_sensitive: bool) {
        // Reuse offsets across keystrokes. Searching a large body must not
        // copy its text or allocate a new list of ranges for every character.
        self.starts.clear();
        self.current = 0;
        self.len = query.len();

        if query.is_empty() {
            return;
        }

        let matcher = AhoCorasickBuilder::new()
            .ascii_case_insensitive(!case_sensitive)
            .build([query])
            .expect("response search query");
        self.starts
            .extend(matcher.find_iter(text).map(|found| found.start()));
    }

    pub(super) fn step(&mut self, previous: bool) {
        if self.starts.is_empty() {
            return;
        }

        self.current = if previous {
            self.current.checked_sub(1).unwrap_or(self.starts.len() - 1)
        } else {
            (self.current + 1) % self.starts.len()
        };
    }

    pub(super) fn current(&self) -> Option<Range<usize>> {
        self.starts
            .get(self.current)
            .map(|&start| start..start + self.len)
    }

    /// The matches that reach into `range`, and whether each is the current one.
    pub(super) fn within(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (Range<usize>, bool)> + '_ {
        // Matches are sorted and do not overlap.
        let first = self
            .starts
            .partition_point(|&start| start + self.len <= range.start);
        self.starts[first..]
            .iter()
            .take_while(move |&&start| start < range.end)
            .enumerate()
            .map(move |(index, &start)| (start..start + self.len, first + index == self.current))
    }

    fn label(&self) -> String {
        if self.starts.is_empty() {
            "0/0".into()
        } else {
            format!("{}/{}", self.current + 1, self.starts.len())
        }
    }
}

impl ResponseView {
    pub(super) fn open_response_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &mut self.state {
            ResponseState::Received {
                body: Body::Raw { search, .. },
                ..
            } => {
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
            ResponseState::Received {
                body: Body::Pretty(editor),
                ..
            } => {
                let editor = editor.read(cx).0.clone();
                editor.update(cx, |editor, cx| editor.open_search(false, cx));
            }
            _ => {}
        }
    }

    fn update_body_search(&mut self, cx: &mut Context<Self>) {
        let ResponseState::Received {
            body:
                Body::Raw {
                    view,
                    search: Some(search),
                },
            ..
        } = &mut self.state
        else {
            return;
        };

        let query = search.input.read(cx).value();
        let case_sensitive = search.case_sensitive;
        view.update(cx, |view, cx| view.search(&query, case_sensitive, cx));
        cx.notify();
    }

    fn move_body_match(&mut self, previous: bool, cx: &mut Context<Self>) {
        let ResponseState::Received {
            body: Body::Raw {
                view,
                search: Some(_),
            },
            ..
        } = &self.state
        else {
            return;
        };

        view.update(cx, |view, cx| view.step_match(previous, cx));
        cx.notify();
    }

    fn close_body_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let ResponseState::Received {
            body: Body::Raw { view, search },
            ..
        } = &mut self.state
        {
            *search = None;
            view.update(cx, |view, cx| {
                view.clear_search(cx);
                window.focus(&view.focus, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn body_search_bar(
        search: &BodySearch,
        view: &Entity<VirtualBody>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let matches = &view.read(cx).matches;
        let label = matches.label();
        let empty = matches.current().is_none();

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
                        if let ResponseState::Received {
                            body:
                                Body::Raw {
                                    search: Some(search),
                                    ..
                                },
                            ..
                        } = &mut this.state
                        {
                            search.case_sensitive = !search.case_sensitive;
                        }
                        this.update_body_search(cx);
                    })),
            )
            .child(div().text_xs().child(label))
            .child(
                Button::new("response-search-previous")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .accessibility_label("Previous match")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| this.move_body_match(true, cx))),
            )
            .child(
                Button::new("response-search-next")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronRight)
                    .accessibility_label("Next match")
                    .disabled(empty)
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
