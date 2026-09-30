use gpui_kit::{App, Hsla, component::ActiveTheme as _};

/// The color that identifies an HTTP method, or the gRPC or WebSocket
/// protocol, wherever it is shown.
pub fn method_color(method: &str, cx: &App) -> Hsla {
    let theme = cx.theme();

    match method {
        "GET" => theme.success,
        "POST" => theme.warning,
        "PUT" | "PATCH" | "gRPC" | "WS" => theme.info,
        "HEAD" | "OPTIONS" => theme.muted_foreground,
        _ => theme.danger,
    }
}
