use gpui_kit::component::{button::*, *};
use gpui_kit::*;
use request_eagle_theme::shortcut_keycaps;

use crate::actions::{
    ImportCollection, NewCollection, NewFlow, NewGrpcTab, NewTab, NewWebSocketTab,
    ToggleCommandPalette,
};

/// What the main view shows while no tab is open: the app's mark and ways to
/// open a tab. The commands run from `focus`, the main view's, so they reach
/// the workspace wherever focus is.
pub(crate) fn placeholder(focus: &FocusHandle, window: &Window, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let commands: [(&'static str, &'static str, &dyn Action); 4] = [
        ("placeholder-new-request", "New request", &NewTab),
        ("placeholder-import", "Import", &ImportCollection),
        (
            "placeholder-find",
            "Find in workspace",
            &ToggleCommandPalette,
        ),
        (
            "placeholder-new-collection",
            "New collection",
            &NewCollection,
        ),
    ];
    let kinds: [(&'static str, &'static str, &'static str, &dyn Action); 4] = [
        (
            "placeholder-new-http",
            "icons/protocol-http.svg",
            "New HTTP request",
            &NewTab,
        ),
        (
            "placeholder-new-grpc",
            "icons/protocol-grpc.svg",
            "New gRPC request",
            &NewGrpcTab,
        ),
        (
            "placeholder-new-websocket",
            "icons/protocol-websocket.svg",
            "New WebSocket request",
            &NewWebSocketTab,
        ),
        (
            "placeholder-new-flow",
            "icons/workflow.svg",
            "New flow",
            &NewFlow,
        ),
    ];

    let run = |action: &dyn Action| {
        let focus = focus.clone();
        let action = action.boxed_clone();

        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            focus.dispatch_action(action.as_ref(), window, cx)
        }
    };

    let command_rows = commands.map(|(id, label, action)| {
        Button::new(id)
            .debug_selector(move || id.into())
            .ghost()
            .w_full()
            .text_color(theme.muted_foreground)
            .accessibility_label(label)
            .child(div().flex_1().child(label))
            .children(shortcut(action, window, cx))
            .on_click(run(action))
    });
    let kind_buttons = kinds.map(|(id, icon, label, action)| {
        Button::new(id)
            .debug_selector(move || id.into())
            .ghost()
            .icon(Icon::default().path(icon))
            .text_color(theme.muted_foreground)
            .accessibility_label(label)
            .tooltip_with_action(label, action, Some("Workspace"))
            .on_click(run(action))
    });

    v_flex()
        .debug_selector(|| "tabs-placeholder".into())
        .size_full()
        .items_center()
        .justify_center()
        .gap_8()
        .child(
            div()
                .flex_none()
                .size(rems(10.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius_full())
                .bg(theme.muted)
                .child(
                    Icon::default()
                        .path("icons/logo.svg")
                        .size(rems(6.))
                        .text_color(theme.background),
                ),
        )
        .child(
            v_flex()
                .flex_none()
                .w(rems(18.))
                .children(command_rows)
                .child(h_flex().mt_3().children(kind_buttons)),
        )
}

/// The keycaps of the shortcut that runs `action` in the workspace, if it has one.
fn shortcut(action: &dyn Action, window: &Window, cx: &App) -> Option<AnyElement> {
    let context = KeyContext::parse("Workspace").ok()?;
    let binding = window.highest_precedence_binding_for_action_in_context(action, context)?;

    Some(
        h_flex()
            .flex_none()
            .gap_1()
            .children(
                binding
                    .keystrokes()
                    .iter()
                    .map(|stroke| shortcut_keycaps(stroke.as_keystroke(), cx)),
            )
            .into_any_element(),
    )
}
