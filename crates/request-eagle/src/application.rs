use gpui_kit::{component::Root, *};
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

        let preferences =
            std::env::home_dir().map(|home| preferences::load(home.join(".request-eagle"), cx));

        cx.spawn(async move |cx| {
            if let Some(load) = preferences
                && let Err(error) = load.await
            {
                eprintln!("Failed to load preferences: {error:#}");
            }

            cx.update(open_workspace);
        })
        .detach();
    });
}

fn open_workspace(cx: &mut App) {
    request_eagle_theme::init(cx);

    let updater = updater::init(env!("CARGO_PKG_VERSION"), cx);
    actions::init(updater.clone(), cx);

    let collections = match std::env::var_os("REQUEST_EAGLE_COLLECTIONS_DIR") {
        Some(path) => collection::CollectionRegistry::from_path(path),
        None => collection::CollectionRegistry::load(),
    }
    .expect("Failed to load collections");

    let window_options = crate::window_options::use_window_options(cx);
    cx.open_window(window_options, move |window, cx| {
        let workspace = workspace::init(collections, updater, window, cx);
        let view = cx.new(|_| ApplicationView { workspace });

        cx.new(|cx| Root::new(view, window, cx))
    })
    .expect("Failed to open the window");

    menu::init(cx);
}

struct ApplicationView {
    workspace: AnyView,
}

impl Render for ApplicationView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let view = div().relative().size_full().child(self.workspace.clone());

        #[cfg(feature = "dev-profiler")]
        let view = view.child(gpui_fps::fps_monitor(_window, _cx));

        view
    }
}
