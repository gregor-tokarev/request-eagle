use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::*;

/// The color that identifies an HTTP method, or the HTTP, gRPC or WebSocket
/// protocol, wherever it is shown.
pub fn method_color(method: &str, cx: &App) -> Hsla {
    let theme = cx.theme();

    match method {
        "GET" | "HTTP" => theme.success,
        "POST" | "WS" => theme.warning,
        "PUT" | "PATCH" | "gRPC" => theme.info,
        "HEAD" | "OPTIONS" => theme.muted_foreground,
        _ => theme.danger,
    }
}

/// The icon of a request protocol, `HTTP`, `gRPC` or `WS`, in its color.
pub fn protocol_icon(protocol: &str, cx: &App) -> Icon {
    let icon = Icon::default().text_color(method_color(protocol, cx));

    match protocol {
        "HTTP" => icon.path("icons/protocol-http.svg"),
        "gRPC" => icon.path("icons/protocol-grpc.svg"),
        "WS" => icon.path("icons/protocol-websocket.svg"),
        _ => icon,
    }
}

/// A request's HTTP method in its color, with the longest methods shortened
/// as in Postman. gRPC and WebSocket requests have no method and show their
/// protocol's icon instead.
pub fn method_label(method: impl Into<SharedString>, cx: &App) -> AnyElement {
    let method = method.into();

    if matches!(method.as_ref(), "gRPC" | "WS") {
        return protocol_icon(&method, cx)
            .size(rems(0.875))
            .into_any_element();
    }

    let label = match method.as_ref() {
        "DELETE" => "DEL".into(),
        "OPTIONS" => "OPT".into(),
        _ => method.clone(),
    };

    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(method_color(&method, cx))
        .child(label)
        .into_any_element()
}
