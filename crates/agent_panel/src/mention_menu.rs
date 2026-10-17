//! The menu that `@` opens in the message box: the saved requests, flows and
//! collections whose name has what is typed after it. Picking one puts a
//! mention of it in the message.

use std::ops::Range;

use gpui_kit::component::{input::InlineToken, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request_eagle_theme::method_label;

use crate::mention::{Mention, MentionKind, matching, typed_mention};
use crate::panel::AgentPanel;
use crate::thread::Reference;

/// How many rows show before the list scrolls.
const VISIBLE_ROWS: usize = 8;

/// Lists what a message can mention.
type ListMentionable = Box<dyn Fn(&App) -> Vec<Mention>>;

pub(crate) struct MentionMenu {
    list: ListMentionable,
    /// The message's text and caret the menu last followed, while the
    /// message box has focus and nothing is selected in it.
    followed: Option<(SharedString, usize)>,
    /// The `@` whose menu was closed. It stays closed while the mention
    /// after it is typed.
    dismissed: Option<usize>,
    open: Option<OpenMenu>,
}

struct OpenMenu {
    /// The `@` and what is typed after it, in the message's text.
    range: Range<usize>,
    /// What the message can mention, listed when the menu opened.
    mentions: Vec<Mention>,
    /// The mentions that match what is typed, by index, best first.
    matches: Vec<usize>,
    selected: usize,
    scroll: UniformListScrollHandle,
}

/// A key the open menu takes from the message box.
pub(crate) enum MenuKey {
    Up,
    Down,
    Pick,
    Close,
}

impl MentionMenu {
    pub(crate) fn new(list: impl Fn(&App) -> Vec<Mention> + 'static) -> Self {
        Self {
            list: Box::new(list),
            followed: None,
            dismissed: None,
            open: None,
        }
    }
}

impl AgentPanel {
    /// Open the menu when an `@` is typed, filter it by what is typed after
    /// the `@`, and close it once the caret leaves that.
    pub(crate) fn follow_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let composer = self.composer.read(cx);
        let followed = (composer.focus_handle(cx).is_focused(window)
            && composer.selected_range().is_empty())
        .then(|| (composer.value(), composer.cursor()));
        if followed == self.mention_menu.followed {
            return;
        }
        let tokens: Vec<Range<usize>> = composer.tokens().iter().map(|span| span.range()).collect();

        let menu = &mut self.mention_menu;
        menu.followed = followed;
        let typed = menu
            .followed
            .as_ref()
            .and_then(|(text, cursor)| Some((text, typed_mention(text, *cursor, &tokens)?)));
        match typed {
            None => {
                menu.dismissed = None;
                menu.open = None;
            }
            Some((_, range)) if menu.dismissed == Some(range.start) => menu.open = None,
            Some((text, range)) => {
                let query = &text[range.start + 1..range.end];
                let open = menu.open.get_or_insert_with(|| OpenMenu {
                    range: range.clone(),
                    mentions: (menu.list)(cx),
                    matches: Vec::new(),
                    selected: 0,
                    scroll: UniformListScrollHandle::new(),
                });
                open.matches = matching(&open.mentions, query);
                open.range = range;
                open.selected = 0;
                open.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
            }
        }
        cx.notify();
    }

    /// Take a key from the message box while the menu is open. Without
    /// matches, Enter, Tab and the arrow keys work in the message as usual.
    pub(crate) fn mention_menu_key(
        &mut self,
        key: MenuKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = &mut self.mention_menu.open else {
            return;
        };
        let count = open.matches.len();
        let selected = open.selected;

        match key {
            MenuKey::Close => self.close_mention_menu(cx),
            MenuKey::Pick if count > 0 => self.pick_mention(selected, window, cx),
            MenuKey::Up | MenuKey::Down if count > 0 => {
                open.selected = match key {
                    MenuKey::Down => (selected + 1) % count,
                    _ => (selected + count - 1) % count,
                };
                open.scroll
                    .scroll_to_item(open.selected, ScrollStrategy::Nearest);
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    /// Close the menu and leave what was typed as it is.
    fn close_mention_menu(&mut self, cx: &mut Context<Self>) {
        let menu = &mut self.mention_menu;
        if let Some(open) = menu.open.take() {
            menu.dismissed = Some(open.range.start);
        }
        cx.notify();
    }

    /// Put a mention of the match at `row` in place of the `@` and what was
    /// typed after it.
    fn pick_mention(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.mention_menu.open.take() else {
            return;
        };
        let Some(mention) = open
            .matches
            .get(row)
            .map(|index| open.mentions[*index].clone())
        else {
            return;
        };

        let id = self.reference_id("mention");
        let token = InlineToken::new(id.clone(), format!("@{}", mention.name))
            .with_label(mention.name.clone());
        let inserted = self.composer.update(cx, |composer, cx| {
            let inserted = composer.replace_range_with_token(open.range, token, window, cx);
            if inserted.is_ok() {
                composer.replace(" ", window, cx);
            }

            inserted
        });

        if inserted.is_ok() {
            self.references.push((id, Reference::Mention(mention)));
        }
        cx.notify();
    }

    /// The menu, above the message box, while it is open there.
    pub(crate) fn render_mention_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.mention_menu.open.as_ref()?;
        if !self.composer.focus_handle(cx).is_focused(window) {
            return None;
        }

        let theme = cx.theme();
        let count = open.matches.len();
        let list = if count == 0 {
            div()
                .p_2()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No matching requests, flows or collections")
                .into_any_element()
        } else {
            uniform_list(
                "agent-mention-list",
                count,
                cx.processor(|this, rows: Range<usize>, _, cx| {
                    rows.map(|row| this.mention_row(row, cx)).collect()
                }),
            )
            .w_full()
            .h(rems(2. * count.min(VISIBLE_ROWS) as f32))
            .track_scroll(&open.scroll)
            .into_any_element()
        };

        Some(
            v_flex()
                .id("agent-mentions")
                .debug_selector(|| "agent-mentions".into())
                .absolute()
                .bottom_full()
                .left_0()
                .w_full()
                .mb_1()
                .p_1()
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius_tokens().lg)
                .shadow_md()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_mention_menu(cx)))
                .child(
                    v_flex()
                        .id("agent-mention-options")
                        .role(Role::ListBox)
                        .aria_label("Requests, flows and collections to mention")
                        .child(list),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("↑↓ Navigate · Enter Insert · Esc Close"),
                )
                .into_any_element(),
        )
    }

    /// A match with what identifies it: a request's method, or an icon, then
    /// its name and where it is.
    fn mention_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = &self.mention_menu.open else {
            return div().into_any_element();
        };
        let mention = &open.mentions[open.matches[row]];
        let selected = row == open.selected;
        let location = mention.location();
        let theme = cx.theme();
        let badge = match &mention.kind {
            MentionKind::Request { method, .. } => method_label(*method, cx),
            MentionKind::Flow => Icon::default()
                .path("icons/workflow.svg")
                .size_4()
                .text_color(theme.chart_4)
                .into_any_element(),
            MentionKind::Collection => Icon::default()
                .path("icons/package.svg")
                .size_4()
                .text_color(theme.muted_foreground)
                .into_any_element(),
        };

        h_flex()
            .id(("agent-mention", row))
            .role(Role::ListBoxOption)
            .aria_label(format!("{}, {location}", mention.name))
            .aria_selected(selected)
            .aria_position_in_set(row + 1)
            .aria_size_of_set(open.matches.len())
            .debug_selector(move || format!("agent-mention-{row}"))
            .w_full()
            .h_8()
            .px_2()
            .gap_2()
            .rounded(theme.radius_tokens().md)
            .when(selected, |this| {
                this.bg(theme.accent).text_color(theme.accent_foreground)
            })
            .hover(|this| this.bg(theme.muted))
            .child(div().flex_none().w(rems(2.5)).child(badge))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .child(mention.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(rems(12.))
                    .text_xs()
                    .text_ellipsis()
                    .text_color(theme.muted_foreground)
                    .child(location),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.pick_mention(row, window, cx);
                }),
            )
            .into_any_element()
    }
}
