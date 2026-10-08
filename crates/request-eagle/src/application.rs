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

        // Compile shared syntax queries off the UI thread so the first editor
        // can reuse them. The window does not wait: creating it takes longer
        // than compiling, and editors only open when a Body or Scripts
        // section is shown.
        cx.background_executor()
            .spawn(async {
                for language in ["json", "javascript"] {
                    gpui_kit::component::highlighter::SyntaxHighlighter::new(language);
                }
            })
            .detach();

        #[cfg(target_os = "macos")]
        cx.set_reduce_motion(
            objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion(),
        );

        let Some(home) = std::env::home_dir() else {
            eprintln!("Failed to load preferences: could not locate the home directory.");
            cx.quit();
            return;
        };

        // Read how the app was last left while the preferences load.
        let session_path = home.join(".request-eagle/session.json");
        let session = cx
            .background_executor()
            .spawn(async move { workspace::Session::load(session_path) });

        if let Err(error) =
            keybindings_service::load_overrides(home.join(".request-eagle/keybindings.json"), cx)
        {
            eprintln!("Failed to load key bindings: {error}");
        }

        let preferences = preferences::load(home.join(".request-eagle"), cx);

        cx.spawn(async move |cx| {
            // The app opens with defaults, and Settings explains why changes
            // cannot be saved until the file is fixed.
            if let Err(error) = preferences.await {
                eprintln!("Failed to load preferences: {error:#}");
            }

            let session = session.await;
            cx.update(|cx| open_workspace(&home, session, cx));
        })
        .detach();
    });
}

fn open_workspace(home: &std::path::Path, session: workspace::Session, cx: &mut App) {
    request_eagle_theme::init(cx);

    let updater = updater::init(env!("CARGO_PKG_VERSION"), cx);
    actions::init(updater.clone(), cx);

    analytics::init(&home.join(".request-eagle"), env!("CARGO_PKG_VERSION"), cx);
    analytics::capture(
        "app_opened",
        serde_json::json!({ "update_track": cx.global::<preferences::Preferences>().update_track }),
        cx,
    );

    let collections_directory = std::env::var_os("REQUEST_EAGLE_COLLECTIONS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".request-eagle/collections"));
    let mut collections = collection::CollectionRegistry::from_path(&collections_directory);
    let flows_directory = home.join(".request-eagle/flows");
    let moved = flow::move_flows_out_of_collections(
        collections
            .skipped()
            .iter()
            .map(|skipped| skipped.path.as_path()),
        &flows_directory,
    );
    if !moved.is_empty() {
        for path in &moved {
            eprintln!("Moved a flow out of its collection to {}", path.display());
        }
        collections = collection::CollectionRegistry::from_path(&collections_directory);
    }
    for skipped in collections.skipped() {
        eprintln!("Left out {}: {}", skipped.path.display(), skipped.error);
    }
    let environments =
        environment::GlobalEnvironments::new(home.join(".request-eagle/environments"));
    let flows = flow::FlowLibrary::load(flows_directory);
    for skipped in flows.skipped() {
        eprintln!("Left out {}: {}", skipped.path.display(), skipped.error);
    }
    let cookies_path = home.join(".request-eagle/cookies.json");
    let cookies = request::CookieJar::open(&cookies_path).map_err(|error| {
        // The unreadable file stays as it is.
        format!(
            "Saved cookies could not be read from {}: {error}. Cookies are kept until you \
             quit. To save them again, fix or delete the file and restart Request Eagle.",
            cookies_path.display()
        )
    });
    let history = request_history::History::new(home.join(".request-eagle/history"));

    let window_options = crate::window_options::use_window_options(&session, cx);
    gpui_kit::open_window(window_options, cx, move |window, cx| {
        window
            .observe_window_appearance(|window, cx| {
                request_eagle_theme::apply_preferences(window.appearance(), cx);
            })
            .detach();

        let workspace = workspace::init(
            collections,
            environments,
            flows,
            cookies,
            history,
            updater,
            session,
            window,
            cx,
        );
        cx.new(|_| ApplicationView { workspace })
    })
    .expect("Failed to open the window");

    // After an update, the installer restores the previous version unless
    // this one confirms that it started.
    cx.background_executor()
        .spawn(async { updater::confirm_startup() })
        .detach();

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
