use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

/// Serves Request Eagle's own icons, falling back to the icons bundled with
/// GPUI Kit.
pub struct Assets;

const LOCAL_ICONS: [(&str, &[u8]); 55] = [
    // The app's mark, shown while no tab is open.
    ("icons/logo.svg", include_bytes!("../assets/icons/logo.svg")),
    (
        "icons/arrow-up-down.svg",
        include_bytes!("../assets/icons/arrow-up-down.svg"),
    ),
    (
        "icons/import.svg",
        include_bytes!("../assets/icons/import.svg"),
    ),
    (
        "icons/keyboard.svg",
        include_bytes!("../assets/icons/keyboard.svg"),
    ),
    (
        "icons/package.svg",
        include_bytes!("../assets/icons/package.svg"),
    ),
    (
        "icons/send-horizontal.svg",
        include_bytes!("../assets/icons/send-horizontal.svg"),
    ),
    (
        "icons/layout-sidebar-filled.svg",
        include_bytes!("../assets/icons/layout-sidebar-filled.svg"),
    ),
    (
        "icons/layout-sidebar-inactive.svg",
        include_bytes!("../assets/icons/layout-sidebar-inactive.svg"),
    ),
    // Lucide icons that GPUI Kit does not bundle by default.
    ("icons/lock.svg", include_bytes!("../assets/icons/lock.svg")),
    (
        "icons/shield-check.svg",
        include_bytes!("../assets/icons/shield-check.svg"),
    ),
    (
        "icons/lock-open.svg",
        include_bytes!("../assets/icons/lock-open.svg"),
    ),
    (
        "icons/wand-sparkles.svg",
        include_bytes!("../assets/icons/wand-sparkles.svg"),
    ),
    (
        "icons/trash.svg",
        include_bytes!("../assets/icons/trash.svg"),
    ),
    (
        "icons/cookie.svg",
        include_bytes!("../assets/icons/cookie.svg"),
    ),
    (
        "icons/refresh-cw.svg",
        include_bytes!("../assets/icons/refresh-cw.svg"),
    ),
    (
        "icons/circle-alert.svg",
        include_bytes!("../assets/icons/circle-alert.svg"),
    ),
    (
        "icons/file-code.svg",
        include_bytes!("../assets/icons/file-code.svg"),
    ),
    (
        "icons/code-xml.svg",
        include_bytes!("../assets/icons/code-xml.svg"),
    ),
    (
        "icons/download.svg",
        include_bytes!("../assets/icons/download.svg"),
    ),
    // The Collection Runner: its tab, run sequence handles and Stop.
    (
        "icons/square-play.svg",
        include_bytes!("../assets/icons/square-play.svg"),
    ),
    (
        "icons/grip-vertical.svg",
        include_bytes!("../assets/icons/grip-vertical.svg"),
    ),
    (
        "icons/square.svg",
        include_bytes!("../assets/icons/square.svg"),
    ),
    // gRPC method kinds: a doubled arrow marks the side that streams.
    (
        "icons/grpc-unary.svg",
        include_bytes!("../assets/icons/grpc-unary.svg"),
    ),
    (
        "icons/grpc-client-streaming.svg",
        include_bytes!("../assets/icons/grpc-client-streaming.svg"),
    ),
    (
        "icons/grpc-server-streaming.svg",
        include_bytes!("../assets/icons/grpc-server-streaming.svg"),
    ),
    (
        "icons/grpc-bidi-streaming.svg",
        include_bytes!("../assets/icons/grpc-bidi-streaming.svg"),
    ),
    // Request protocols, shown where a request has no HTTP method to show.
    (
        "icons/protocol-http.svg",
        include_bytes!("../assets/icons/protocol-http.svg"),
    ),
    (
        "icons/protocol-grpc.svg",
        include_bytes!("../assets/icons/protocol-grpc.svg"),
    ),
    (
        "icons/protocol-websocket.svg",
        include_bytes!("../assets/icons/protocol-websocket.svg"),
    ),
    // Flows: the flow item and its block types, and the canvas controls.
    (
        "icons/workflow.svg",
        include_bytes!("../assets/icons/workflow.svg"),
    ),
    (
        "icons/square-function.svg",
        include_bytes!("../assets/icons/square-function.svg"),
    ),
    (
        "icons/split.svg",
        include_bytes!("../assets/icons/split.svg"),
    ),
    (
        "icons/git-branch.svg",
        include_bytes!("../assets/icons/git-branch.svg"),
    ),
    (
        "icons/timer.svg",
        include_bytes!("../assets/icons/timer.svg"),
    ),
    (
        "icons/merge.svg",
        include_bytes!("../assets/icons/merge.svg"),
    ),
    (
        "icons/repeat.svg",
        include_bytes!("../assets/icons/repeat.svg"),
    ),
    (
        "icons/list-ordered.svg",
        include_bytes!("../assets/icons/list-ordered.svg"),
    ),
    (
        "icons/combine.svg",
        include_bytes!("../assets/icons/combine.svg"),
    ),
    (
        "icons/monitor.svg",
        include_bytes!("../assets/icons/monitor.svg"),
    ),
    (
        "icons/scroll-text.svg",
        include_bytes!("../assets/icons/scroll-text.svg"),
    ),
    ("icons/type.svg", include_bytes!("../assets/icons/type.svg")),
    ("icons/hash.svg", include_bytes!("../assets/icons/hash.svg")),
    (
        "icons/toggle-left.svg",
        include_bytes!("../assets/icons/toggle-left.svg"),
    ),
    (
        "icons/circle-off.svg",
        include_bytes!("../assets/icons/circle-off.svg"),
    ),
    (
        "icons/clock.svg",
        include_bytes!("../assets/icons/clock.svg"),
    ),
    (
        "icons/mouse-pointer-click.svg",
        include_bytes!("../assets/icons/mouse-pointer-click.svg"),
    ),
    (
        "icons/braces.svg",
        include_bytes!("../assets/icons/braces.svg"),
    ),
    ("icons/list.svg", include_bytes!("../assets/icons/list.svg")),
    (
        "icons/log-in.svg",
        include_bytes!("../assets/icons/log-in.svg"),
    ),
    (
        "icons/log-out.svg",
        include_bytes!("../assets/icons/log-out.svg"),
    ),
    ("icons/flag.svg", include_bytes!("../assets/icons/flag.svg")),
    (
        "icons/sticky-note.svg",
        include_bytes!("../assets/icons/sticky-note.svg"),
    ),
    (
        "icons/zoom-in.svg",
        include_bytes!("../assets/icons/zoom-in.svg"),
    ),
    (
        "icons/zoom-out.svg",
        include_bytes!("../assets/icons/zoom-out.svg"),
    ),
    ("icons/scan.svg", include_bytes!("../assets/icons/scan.svg")),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = LOCAL_ICONS.iter().find(|(icon_path, _)| *icon_path == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }

        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(
            LOCAL_ICONS
                .iter()
                .map(|(icon_path, _)| *icon_path)
                .filter(|icon_path| icon_path.starts_with(path))
                .map(SharedString::from),
        );

        Ok(paths)
    }
}
