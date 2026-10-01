use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

/// Serves Request Eagle's own icons, falling back to the icons bundled with
/// GPUI Kit.
pub struct Assets;

const LOCAL_ICONS: [(&str, &[u8]); 19] = [
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
