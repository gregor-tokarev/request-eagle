use gpui_kit::component::{
    button::*,
    input::{InputEvent, InputState},
    kbd::Kbd,
    tooltip::Tooltip,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use keybindings_service::{self as keybindings, Command};

use super::recorder::Recording;

pub(crate) struct KeybindingsPage {
    pub(super) search: Entity<InputState>,
    pub(super) search_focus: FocusHandle,
    pub(super) search_by_shortcut: bool,
    pub(super) search_keystroke: Option<Keystroke>,

    pub(super) recorder_focus: FocusHandle,
    pub(super) recorder_scope: FocusHandle,
    pub(super) recording: Option<Recording>,
    pub(super) error: Option<String>,

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

        h_flex()
            .debug_selector(move || format!("keybinding-row-{id}"))
            .w_full()
            .h(px(56.))
            .flex_none()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
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
                    .w(if compact { px(176.) } else { px(216.) })
                    .h_9()
                    .flex_none()
                    .child(match recording {
                        Some(recording) => self.render_recorder(recording, cx).into_any_element(),
                        None => Button::new(SharedString::from(format!("record-{id}")))
                            .debug_selector(move || format!("record-{id}"))
                            .ghost()
                            .w_full()
                            .h_9()
                            .px_3()
                            .justify_end()
                            .overflow_hidden()
                            .tooltip("Click to record a shortcut")
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
                    .w(px(80.))
                    .flex_none()
                    .justify_end()
                    .child(match recording {
                        Some(recording) => self.recorder_buttons(recording, cx).into_any_element(),
                        None => h_flex()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!("remove-{id}")))
                                    .debug_selector(move || format!("remove-{id}"))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Delete)
                                    .when(command.binding.is_some(), |this| {
                                        this.text_color(cx.theme().muted_foreground)
                                    })
                                    .disabled(command.binding.is_none())
                                    .tooltip("Remove shortcut")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.error = keybindings::set_override(id, None, cx)
                                            .err()
                                            .map(|error| error.to_string());

                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("reset-{id}")))
                                    .debug_selector(move || format!("reset-{id}"))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Undo2)
                                    .disabled(!command.is_modified())
                                    .tooltip("Reset to default")
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.reset(Some(id), cx)),
                                    ),
                            )
                            .into_any_element(),
                    }),
            )
    }
}

impl Render for KeybindingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(680.);

        let mut commands = keybindings::commands(cx);
        commands.sort_by_key(|command| command.label);
        let modified = commands.iter().any(Command::is_modified);

        let query = self.search.read(cx).value();
        let visible = commands
            .iter()
            .filter(|command| self.matches_search(command, &query))
            .collect::<Vec<_>>();

        v_flex()
            .w_full()
            .max_w(px(880.))
            .gap_6()
            .child(
                h_flex()
                    .justify_between()
                    .flex_wrap()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(rems(1.625))
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
            .child(
                v_flex().w_full().children(
                    visible
                        .iter()
                        .map(|command| self.render_command(command, compact, cx)),
                ),
            )
            .when(visible.is_empty(), |this| {
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

fn shortcut(keys: Option<&str>, cx: &App) -> AnyElement {
    match keys {
        Some(keys) => h_flex()
            .w_full()
            .justify_end()
            .gap_1()
            .children(
                keys.split_whitespace()
                    .filter_map(|key| Keystroke::parse(key).ok())
                    .map(|stroke| shortcut_keycaps(&stroke, cx)),
            )
            .into_any_element(),
        None => div()
            .w_full()
            .text_right()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("Not set")
            .into_any_element(),
    }
}

pub(super) fn shortcut_keycaps(stroke: &Keystroke, cx: &App) -> AnyElement {
    let mac = cfg!(target_os = "macos");
    let modifiers = [
        (stroke.modifiers.platform, if mac { "⌘" } else { "Win" }),
        (stroke.modifiers.control, if mac { "⌃" } else { "Ctrl" }),
        (stroke.modifiers.alt, if mac { "⌥" } else { "Alt" }),
        (stroke.modifiers.shift, if mac { "⇧" } else { "Shift" }),
        (stroke.modifiers.function, "Fn"),
    ];

    let mut key = stroke.clone();
    key.modifiers = Modifiers::default();

    let labels = modifiers
        .into_iter()
        .filter(|(pressed, _)| *pressed)
        .map(|(_, label)| label.to_owned())
        .chain(std::iter::once(Kbd::format(&key)));

    h_flex()
        .gap_1()
        .children(labels.map(|label| {
            div()
                .h_6()
                .min_w_6()
                .px_1()
                .flex_none()
                .rounded_md()
                .bg(cx.theme().foreground.opacity(0.06))
                .text_color(cx.theme().muted_foreground)
                .text_sm()
                .text_center()
                .line_height(px(24.))
                .child(label)
        }))
        .into_any_element()
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
