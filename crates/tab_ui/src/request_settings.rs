//! Rows of a request's Settings tab. Their options follow Settings until a
//! request changes them.

use std::rc::Rc;

use gpui_kit::component::{button::*, input::InputState, switch::Switch, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use preferences::Preferences;
use request::RequestPreferences;

/// A setting's title and description beside its control.
pub(crate) fn row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .justify_between()
        .gap_4()
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(div().flex_none().child(control))
}

/// A switch showing the request's value, or the preference until the request
/// changes it. A changed value can be reset to follow the preference again.
pub(crate) fn override_switch(
    id: &'static str,
    label: &'static str,
    setting: Option<bool>,
    preference: bool,
    on_change: impl Fn(&Option<bool>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_change = Rc::new(on_change);
    let reset = on_change.clone();

    h_flex()
        .debug_selector(move || id.into())
        .gap_2()
        .when(setting.is_some(), |this| {
            this.child(
                Button::new(SharedString::from(format!("{id}-reset")))
                    .debug_selector(move || format!("{id}-reset"))
                    .ghost()
                    .xsmall()
                    .label("Reset")
                    .tooltip("Follow Settings")
                    .on_click(move |_, window, cx| reset(&None, window, cx)),
            )
        })
        .child(
            Switch::new(id)
                .accessibility_label(label)
                .checked(setting.unwrap_or(preference))
                .on_click(move |checked, window, cx| on_change(&Some(*checked), window, cx)),
        )
}

/// An input for a timeout in milliseconds. Empty follows the preference,
/// which its placeholder shows.
pub(crate) fn timeout_input(
    setting: Option<u64>,
    window: &mut Window,
    cx: &mut Context<InputState>,
) -> InputState {
    InputState::new(window, cx)
        .placeholder(preferences(cx).timeout_ms.to_string())
        .default_value(
            setting
                .map(|timeout| timeout.to_string())
                .unwrap_or_default(),
        )
        .validate(|value, _| value.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The timeout an input holds; empty or out of range follows the preference.
pub(crate) fn timeout_value(input: &InputState) -> Option<u64> {
    input.value().trim().parse().ok()
}

pub(crate) fn preferences(cx: &App) -> RequestPreferences {
    cx.try_global::<Preferences>()
        .map(|preferences| preferences.request.clone())
        .unwrap_or_default()
}
