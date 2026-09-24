use gpui_kit::component::{
    button::*,
    input::{InputEvent, InputState},
    tooltip::Tooltip,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use keybindings_service::{self as keybindings, Command};
use std::collections::HashMap;

use super::{
    recorder::Recording,
    row::{CommandRow, row_button},
    shortcut,
};

pub(crate) struct KeybindingsPage {
    pub(super) search: Entity<InputState>,
    pub(super) search_focus: FocusHandle,
    pub(super) search_by_shortcut: bool,
    pub(super) search_keystroke: Option<Keystroke>,

    pub(super) recorder_focus: FocusHandle,
    pub(super) recorder_scope: FocusHandle,
    pub(super) recording: Option<Recording>,
    pub(super) error: Option<String>,

    pub(super) scroll_handle: ScrollHandle,
    rows: HashMap<&'static str, Entity<CommandRow>>,
    visible_commands: Vec<&'static str>,

    _subscriptions: Vec<Subscription>,
}

impl KeybindingsPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search commands or shortcuts…"));
        let search_subscription = cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify());

        let recorder_scope = cx.focus_handle().tab_stop(false);
        let mut subscriptions = vec![search_subscription];
        subscriptions.extend(Self::recorder_subscriptions(&recorder_scope, window, cx));

        Self {
            search,
            search_focus: cx.focus_handle(),
            search_by_shortcut: false,
            search_keystroke: None,
            recorder_focus: cx.focus_handle(),
            recorder_scope,
            recording: None,
            error: None,
            scroll_handle: ScrollHandle::new(),
            rows: HashMap::new(),
            visible_commands: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_by_shortcut {
            window.focus(&self.search_focus, cx);
        } else {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        }
    }

    fn reset(&mut self, id: Option<&str>, cx: &mut Context<Self>) {
        let result = match id {
            Some(id) => keybindings::reset_command(id, cx),
            None => keybindings::reset_all(cx),
        };

        self.error = result.err().map(|error| error.to_string());

        cx.notify();
    }

    pub(super) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_by_shortcut = false;
        self.search_keystroke = None;
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.focus_search(window, cx);

        cx.notify();
    }

    pub(super) fn render_command(
        &self,
        command: &Command,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let record_command = command.clone();
        let id = command.id;
        let recording = self
            .recording
            .as_ref()
            .filter(|recording| recording.command.id == id);
        let error = match recording {
            Some(recording) => recording.error.as_ref(),
            None => command.binding_error.as_ref(),
        };

        let shortcut_width = if compact { rems(11.) } else { rems(13.5) };

        div()
            .debug_selector(move || format!("keybinding-row-{id}"))
            .relative()
            .w_full()
            .h(rems(3.5))
            .flex_none()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .absolute()
                    .left_0()
                    .right(shortcut_width + rems(6.5))
                    .top_0()
                    .bottom_0()
                    .justify_center()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(command.label),
                    )
                    .when_some(error, |this, error| {
                        let error = error.clone();

                        this.child(
                            div()
                                .id(SharedString::from(format!("keybinding-error-{id}")))
                                .h_4()
                                .text_xs()
                                .truncate()
                                .text_color(cx.theme().danger)
                                .child(error.clone())
                                .tooltip(move |window, cx| {
                                    Tooltip::new(error.clone()).build(window, cx)
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .debug_selector(move || format!("shortcut-slot-{id}"))
                    .absolute()
                    .right(rems(5.75))
                    .top_0()
                    .bottom_0()
                    .w(shortcut_width)
                    .flex()
                    .items_center()
                    .flex_none()
                    .child(match recording {
                        Some(recording) => self.render_recorder(recording, cx).into_any_element(),
                        None => row_button(
                            SharedString::from(format!("record-{id}")),
                            "Click to record a shortcut",
                            false,
                            cx,
                        )
                        .debug_selector(move || format!("record-{id}"))
                        .w_full()
                        .h_9()
                        .px_3()
                        .justify_end()
                        .overflow_hidden()
                        .child(shortcut(
                            command.binding.as_ref().map(|b| b.keystrokes.as_str()),
                            cx,
                        ))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_recording(record_command.clone(), window, cx)
                        }))
                        .into_any_element(),
                    }),
            )
            .child(
                h_flex()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w_20()
                    .flex_none()
                    .justify_end()
                    .child(match recording {
                        Some(recording) => self.recorder_buttons(recording, cx).into_any_element(),
                        None => h_flex()
                            .gap_1()
                            .child(
                                row_button(
                                    SharedString::from(format!("remove-{id}")),
                                    "Remove shortcut",
                                    command.binding.is_none(),
                                    cx,
                                )
                                .debug_selector(move || format!("remove-{id}"))
                                .child(Icon::new(IconName::Delete).size_3p5())
                                .when(command.binding.is_some(), |this| {
                                    this.text_color(cx.theme().muted_foreground)
                                })
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.error = keybindings::set_override(id, None, cx)
                                            .err()
                                            .map(|error| error.to_string());

                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                row_button(
                                    SharedString::from(format!("reset-{id}")),
                                    "Reset to default",
                                    !command.is_modified(),
                                    cx,
                                )
                                .debug_selector(move || format!("reset-{id}"))
                                .child(Icon::new(IconName::Undo2).size_3p5())
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.reset(Some(id), cx)),
                                ),
                            )
                            .into_any_element(),
                    }),
            )
    }

    fn command_row(
        &mut self,
        command: &Command,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The recorder changes on each keystroke; ordinary rows only change
        // when their command changes. Keep cursor blinks out of their layout.
        if self
            .recording
            .as_ref()
            .is_some_and(|recording| recording.command.id == command.id)
        {
            return self.render_command(command, compact, cx).into_any_element();
        }

        let page = cx.entity().downgrade();
        let row = self.rows.entry(command.id).or_insert_with(|| {
            cx.new(|_| CommandRow {
                command: command.clone(),
                page: page.clone(),
            })
        });
        if row.read(cx).command != *command {
            // Replace the cached content and handlers with the new binding.
            *row = cx.new(|_| CommandRow {
                command: command.clone(),
                page,
            });
        }

        // GPUI invalidates cached views when their bounds/text style change or
        // the window refreshes. Theme application already refreshes all windows.
        row.clone()
            .cached(StyleRefinement::default().w_full().h(rems(3.5)).flex_none())
            .into_any_element()
    }
}

impl Render for KeybindingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = crate::geometry::is_narrow(window);

        let mut commands = keybindings::commands(cx);
        commands.sort_by_key(|command| command.label);
        let modified = commands.iter().any(Command::is_modified);

        let query = self.search.read(cx).value();
        let visible = commands
            .into_iter()
            .filter(|command| self.matches_search(command, &query))
            .collect::<Vec<_>>();
        let empty = visible.is_empty();
        let visible_commands = visible.iter().map(|command| command.id).collect::<Vec<_>>();
        if self.visible_commands != visible_commands {
            self.visible_commands = visible_commands;
            self.scroll_handle.scroll_to_item(0);
        }

        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .max_w(crate::geometry::PAGE_WIDTH)
            .gap_6()
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .flex_wrap()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Keybindings"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Search and customize application shortcuts."),
                            ),
                    )
                    .child(
                        Button::new("reset-all-keybindings")
                            .small()
                            .outline()
                            .icon(IconName::Undo2)
                            .label("Reset all")
                            .disabled(!modified)
                            .on_click(cx.listener(|this, _, _, cx| this.reset(None, cx))),
                    ),
            )
            .child(self.render_search(window, cx))
            .when_some(keybindings::storage_error(cx), |this, error| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_owned()),
                )
            })
            .when_some(self.error.as_ref(), |this, error| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.clone()),
                )
            })
            .when(!empty, |this| {
                this.child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(
                            v_flex()
                                .id("keybinding-commands")
                                .size_full()
                                .pr_4()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll_handle)
                                .children(
                                    visible
                                        .iter()
                                        .map(|command| self.command_row(command, compact, cx)),
                                ),
                        )
                        .child(scroll::Scrollbar::vertical(&self.scroll_handle)),
                )
            })
            .when(empty, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .py_12()
                        .items_center()
                        .gap_3()
                        .child(
                            Icon::new(IconName::Search)
                                .size_6()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child("No keybindings found"),
                        )
                        .child(
                            Button::new("clear-keybinding-search")
                                .ghost()
                                .small()
                                .label("Clear search")
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.clear_search(window, cx)
                                    }),
                                ),
                        ),
                )
            })
    }
}

fn search_text(value: &str) -> String {
    value
        .to_lowercase()
        .replace('⌘', " cmd ")
        .replace("command", "cmd")
        .replace("super", "cmd")
        .replace('⌃', " ctrl ")
        .replace("control", "ctrl")
        .replace('⌥', " alt ")
        .replace("option", "alt")
        .replace('⇧', " shift ")
        .replace('⎋', " escape ")
        .replace(['-', '+'], " ")
}

pub(super) fn matches_search(command: &Command, query: &str) -> bool {
    let keys = command
        .binding
        .as_ref()
        .map(|b| b.keystrokes.as_str())
        .unwrap_or("not set unassigned");
    let text = search_text(&format!(
        "{} {} {} {} {}",
        command.label, command.description, command.category, command.id, keys
    ));

    search_text(query)
        .split_whitespace()
        .all(|word| text.contains(word))
}
