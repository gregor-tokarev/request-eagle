use gpui_kit::{App, TitlebarOptions, WindowBounds, WindowKind, WindowOptions, point, px};

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
            traffic_light_position: Some(point(px(9.0), px(9.0))),
        }),
        window_bounds,
        app_id: Some("com.egortokarev.requesteagle".into()),
        is_movable: true,
        kind: WindowKind::Normal,
        display_id,
        window_min_size: Some(gpui_kit::Size {
            width: px(360.0),
            height: px(240.0),
        }),
        ..Default::default()
    }
}
