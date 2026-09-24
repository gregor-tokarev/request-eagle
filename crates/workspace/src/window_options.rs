use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, TitlebarOptions, WindowBounds, WindowKind, WindowOptions, point, px, rems};

pub(crate) fn use_window_options(cx: &mut App) -> WindowOptions {
    let display = cx.primary_display();

    let display_id = display.as_ref().map(|display| display.id());
    let window_bounds = display.map(|display| WindowBounds::Maximized(display.default_bounds()));

    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(
                std::env::var("REQUEST_EAGLE_WINDOW_TITLE")
                    .unwrap_or_else(|_| "Request Eagle".into())
                    .into(),
            ),
            appears_transparent: true,
            // Native window chrome uses platform pixels.
            traffic_light_position: Some(point(px(9.0), px(9.0))),
        }),
        window_bounds,
        app_id: Some("com.egortokarev.requesteagle".into()),
        is_movable: true,
        kind: WindowKind::Normal,
        display_id,
        window_min_size: Some(gpui_kit::Size {
            // The platform API needs pixels; reserve room for both work panes
            // using the interface scale selected when the window opens.
            width: rems(40.).to_pixels(cx.theme().font_size),
            height: rems(40.).to_pixels(cx.theme().font_size),
        }),
        ..Default::default()
    }
}
