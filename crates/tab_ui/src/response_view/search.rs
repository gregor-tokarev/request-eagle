use gpui_kit::base::input::SearchMatcher;
use gpui_kit::component::{
    button::*,
    input::{Enter, Escape, Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;

use super::view::ResponseView;

pub(super) struct BodySearch {
    pub(super) input: Entity<InputState>,
    pub(super) matcher: SearchMatcher,
    case_sensitive: bool,
    _subscription: Subscription,
}

impl ResponseView {
    pub(super) fn open_response_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(body) = &self.virtual_body else {
            if let Some(editor) = &self.editor {
                editor.update(cx, |editor, cx| editor.open_search(false, cx));
            }
            return;
        };

        if self.body_search.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search response"));
            let mut matcher = SearchMatcher::new();
            matcher.update(&body.read(cx).text);
            let subscription =
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.update_body_search(window, cx);
                    }
                });
            self.body_search = Some(BodySearch {
                input,
                matcher,
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
        search
            .matcher
            .update_query(&search.input.read(cx).value(), !search.case_sensitive);
        self.select_body_match(window, cx);
    }

    fn move_body_match(&mut self, previous: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(search) = &mut self.body_search else {
            return;
        };
        if previous {
            search.matcher.next_back();
        } else {
            search.matcher.next();
        }
        self.select_body_match(window, cx);
    }

    fn select_body_match(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let search = self.body_search.as_ref().unwrap();
        let matches = search.matcher.matched_ranges();
        let range = search.matcher.current().map(|index| matches[index].clone());
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
            .child(div().text_xs().child(search.matcher.label()))
            .child(
                Button::new("response-search-previous")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .accessibility_label("Previous match")
                    .disabled(search.matcher.is_empty())
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
                    .disabled(search.matcher.is_empty())
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
