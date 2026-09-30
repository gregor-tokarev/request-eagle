use std::fs;

use crate::actions::{NewTab, ToggleLeftSidebar};
use crate::bottom_panel::TOGGLE_SIDEBAR_BUTTON;
use crate::main_view::{Page, PageTab};
use crate::workspace::{Workspace, on_toggle_sidebar};
use collection::CollectionRegistry;
use environment::GlobalEnvironments;
use gpui_kit::{
    AppContext as _, Entity, Focusable, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    TestAppContext, VisualTestContext, component::Root, px,
};
use settings_ui::CloseSettings;

/// Set up the globals the workspace uses, with animations reduced.
pub(crate) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
        cx.set_reduce_motion(true);
    });
}

/// A workspace in a new window. Like the application, it is inside a Root,
/// which shows dialogs.
pub(crate) fn workspace(
    collections: CollectionRegistry,
    environments: GlobalEnvironments,
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    let mut workspace = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            Workspace::new(
                collections,
                environments,
                updater::init("1.2.3", cx),
                window,
                cx,
            )
        });
        workspace = Some(view.clone());

        Root::new(view, window, cx)
    });

    (workspace.unwrap(), cx)
}

/// A catalog in a missing directory, for tests that do not use global environments.
pub(crate) fn no_environments() -> GlobalEnvironments {
    GlobalEnvironments::new("/nonexistent/request-eagle/environments")
}

/// Collections of GET requests, loaded through the real parser: 100 requests
/// per collection, in folders of 20. Their files are removed once loaded.
pub(crate) fn collections(request_count: usize) -> CollectionRegistry {
    if request_count == 0 {
        return CollectionRegistry::new();
    }

    let directory = tempfile::tempdir().unwrap();

    for index in 0..request_count {
        let folder = directory.path().join(format!(
            "collection-{:02}/folder-{:02}",
            index / 100,
            index % 100 / 20
        ));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("request-{index:04}.toml")), format!(
            "id = \"request-{index}\"\nname = \"Get resource {index}\"\nschema_version = 1\n[request]\ntype = \"http\"\nmethod = \"GET\"\npath = \"/resources/{index}\"\nheaders = []\n"
        )).unwrap();
    }

    CollectionRegistry::from_path(directory.path()).unwrap()
}

/// Click an element, found in a fresh layout.
pub(crate) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
}

impl PageTab {
    /// The request draft shown in this tab.
    pub(crate) fn draft(&self) -> gpui_kit::Entity<tab_ui::RequestDraft> {
        let Page::Request(draft) = &self.page else {
            panic!("{} is not a request tab", self.title);
        };

        draft.clone()
    }

    /// The WebSocket draft shown in this tab.
    pub(crate) fn websocket(&self) -> gpui_kit::Entity<tab_ui::WebSocketDraft> {
        let Page::WebSocket(draft) = &self.page else {
            panic!("{} is not a WebSocket tab", self.title);
        };

        draft.clone()
    }

    /// Where the tab's request is saved.
    pub(crate) fn location(&self, cx: &gpui_kit::App) -> Option<tab_ui::RequestLocation> {
        self.page.location(cx).cloned()
    }
}

#[gpui_kit::test]
fn settings_survives_closing_and_reopening(cx: &mut TestAppContext) {
    init(cx);
    // Overrides saved before CloseSettings moved crates must still resolve.
    cx.update(|cx| {
        keybindings_service::set_override("workspace::CloseSettings", Some("ctrl-w"), cx).unwrap();
    });

    let (layout, cx) = workspace(CollectionRegistry::new(), no_environments(), cx);

    // Launching does not build the settings screen.
    assert!(cx.read(|cx| layout.read(cx).settings.is_none()));
    let mut settings = None;
    assert!(cx.debug_bounds("settings").is_none());
    assert!(cx.debug_bounds("main-view").is_some());
    let sidebar_focus = cx.read(|cx| layout.read(cx).sidebar.focus_handle(cx));
    cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));

    for attempt in 0..2 {
        cx.update(|window, cx| {
            layout.update(cx, |layout, cx| layout.open_settings(window, cx));
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("settings").is_some());
        assert!(cx.debug_bounds("main-view").is_none());
        let opened = cx.read(|cx| {
            assert!(layout.read(cx).settings_visible);
            layout.read(cx).settings.clone()
        });
        assert!(opened.is_some());
        assert_eq!(settings.get_or_insert(opened.clone()), &opened);

        // Reopening an already visible screen must not replace the saved focus.
        cx.update(|window, cx| {
            layout.update(cx, |layout, cx| layout.open_settings(window, cx));
            if attempt == 0 {
                window.dispatch_action(Box::new(CloseSettings), cx);
            }
        });
        if attempt == 1 {
            cx.simulate_keystrokes("ctrl-w");
        }
        cx.run_until_parked();

        cx.read(|cx| {
            assert!(!layout.read(cx).settings_visible);
            assert_eq!(layout.read(cx).settings, opened);
        });
        assert!(cx.debug_bounds("settings").is_none());
        assert!(cx.debug_bounds("main-view").is_some());
        cx.update(|window, _| assert!(sidebar_focus.is_focused(window)));
    }
}

#[gpui_kit::test]
fn toggle_sidebar_action(cx: &mut TestAppContext) {
    init(cx);
    let (layout, cx) = workspace(CollectionRegistry::new(), no_environments(), cx);
    cx.update(|_, cx| on_toggle_sidebar(&layout, cx));

    let sidebar_visible =
        |cx: &TestAppContext| cx.read(|cx| *layout.read(cx).sidebar_visible.read(cx));

    assert!(sidebar_visible(cx));

    let main_split = cx.read(|cx| layout.read(cx).main_split.clone());
    cx.update(|window, cx| {
        main_split.update(cx, |split, cx| split.resize_panel(0, px(300.), window, cx));
    });
    cx.run_until_parked();

    let panel_sizes = cx.read(|cx| main_split.read(cx).sizes().clone());
    assert_eq!(panel_sizes[0], px(300.));

    let main_bounds = cx
        .debug_bounds("main-view")
        .expect("main view should be rendered");

    // The tooltip hint shows this binding.
    cx.update(|window, _| {
        let binding = window
            .highest_precedence_binding_for_action(&ToggleLeftSidebar)
            .expect("ToggleLeftSidebar should be bound");
        assert_eq!(
            binding.keystrokes()[0].inner(),
            &gpui_kit::Keystroke::parse("secondary-b").unwrap()
        );
    });

    cx.simulate_keystrokes("secondary-b");
    assert!(!sidebar_visible(cx));

    let expanded_bounds = cx
        .debug_bounds("main-view")
        .expect("main view should remain rendered");
    assert_eq!(
        expanded_bounds.size.width,
        main_bounds.size.width + px(300.)
    );

    cx.simulate_keystrokes("secondary-b");
    assert!(sidebar_visible(cx));

    assert_eq!(cx.debug_bounds("main-view"), Some(main_bounds));

    assert_eq!(
        cx.read(|cx| main_split.read(cx).sizes().clone()),
        panel_sizes
    );

    let button_bounds = cx
        .debug_bounds(TOGGLE_SIDEBAR_BUTTON)
        .expect("toggle-sidebar button should be rendered");
    cx.simulate_click(button_bounds.center(), Modifiers::default());
    assert!(!sidebar_visible(cx));
}

#[gpui_kit::test]
fn collection_panel_receives_initial_focus_and_keyboard_navigation(cx: &mut TestAppContext) {
    init(cx);
    let (layout, cx) = workspace(collections(2), no_environments(), cx);
    cx.update(|window, cx| {
        window.activate_window();
        assert!(layout.read(cx).sidebar.focus_handle(cx).is_focused(window));
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("collection-row-1").is_some());

    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    assert!(cx.debug_bounds("collection-row-1").is_none());
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!(cx.debug_bounds("collection-row-1").is_some());

    cx.simulate_keystrokes("down right f2");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_none());
    cx.update(|window, cx| {
        assert!(layout.read(cx).sidebar.focus_handle(cx).is_focused(window));
    });
}

#[gpui_kit::test]
fn sidebar_sections_fold_and_reopen(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    let (layout, cx) = workspace(
        collections(2),
        GlobalEnvironments::new(directory.path()),
        cx,
    );

    // Cached sections record their bounds in a fresh layout.
    let height = |cx: &mut VisualTestContext, selector| {
        cx.update(|window, _| window.refresh());
        cx.debug_bounds(selector).map(|bounds| bounds.size.height)
    };

    // Open sections share the sidebar height.
    let environments = height(cx, "environments-sidebar").unwrap();
    assert_eq!(height(cx, "collections-sidebar"), Some(environments));

    // A folded section gives its height away. Its rows give up focus to the
    // header, which opens it again from the keyboard.
    click(cx, "collections-section");
    assert_eq!(height(cx, "collections-sidebar"), None);
    assert!(height(cx, "environments-sidebar").unwrap() > environments);
    let header = cx.read(|cx| layout.read(cx).collections_header.clone());
    cx.update(|window, cx| {
        assert!(header.is_focused(window));
        assert!(window.is_action_available(&NewTab, cx));
    });

    let keystroke = Keystroke::parse("enter").unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    assert!(height(cx, "collections-sidebar").is_some());
    cx.update(|window, _| assert!(header.is_focused(window)));

    // Creating from a folded section opens it.
    click(cx, "environments-section");
    assert_eq!(height(cx, "environments-sidebar"), None);
    click(cx, "new-environment");
    assert!(height(cx, "environments-sidebar").is_some());
    cx.read(|cx| assert_eq!(layout.read(cx).main_view.read(cx).tabs.len(), 2));

    // So does importing, which shows the imported collection in the tree.
    click(cx, "collections-section");
    assert_eq!(height(cx, "collections-sidebar"), None);
    click(cx, "import-collection");
    cx.run_until_parked();
    assert!(height(cx, "collections-sidebar").is_some());
    assert!(cx.debug_bounds("import-dialog").is_some());
}
