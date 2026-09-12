use gpui_kit::*;
use std::sync::Arc;

use crate::{actions, assets, menu};

pub fn run() {
    let user_agent = format!(
        "RequestEagle/{} ({}; {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let http_client = reqwest_client::ReqwestClient::user_agent(&user_agent)
        .expect("Failed to initialize the HTTP client");

    let app = gpui_kit::application()
        .with_assets(assets::Assets)
        .with_http_client(Arc::new(http_client));

    app.run(move |cx: &mut App| {
        gpui_kit::init(cx);

        #[cfg(target_os = "macos")]
        cx.set_reduce_motion(
            objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion(),
        );

        keybindings_service::init(cx);

        if let Some(home) = std::env::home_dir()
            && let Err(error) = keybindings_service::load_overrides(
                home.join(".request-eagle/keybindings.json"),
                cx,
            )
        {
            eprintln!("Failed to load key bindings: {error}");
        }

        if let Some(home) = std::env::home_dir()
            && let Err(error) = preferences::load(home.join(".request-eagle"), cx)
        {
            eprintln!("Failed to load preferences: {error:#}");
        }

        request_eagle_theme::init(cx);

        let updater = updater::init(env!("CARGO_PKG_VERSION"), cx);
        actions::init(updater.clone(), cx);

        let collections = match std::env::var_os("REQUEST_EAGLE_COLLECTIONS_DIR") {
            Some(path) => collection::CollectionRegistry::from_path(path),
            None => collection::CollectionRegistry::load(),
        }
        .expect("Failed to load collections");

        workspace::init(collections, updater, cx);

        #[cfg(feature = "dev-profiler")]
        for window in cx.windows() {
            window
                .update(cx, |_, window, _| {
                    window.set_debug_frame_overlay_mode(DebugFrameOverlayMode::Full);
                })
                .expect("Failed to enable the development frame monitor");
        }

        menu::init(cx);
    });
}
