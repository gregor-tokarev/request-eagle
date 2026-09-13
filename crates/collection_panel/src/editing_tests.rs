use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use collection::CollectionRegistry;
use gpui_kit::{
    AppContext, Entity, Focusable, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext, component::Root, px, size,
};

use super::CollectionPanel;

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Fixture(pub(super) PathBuf);

impl Fixture {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "request-eagle-sidebar-edits-{}-{}-{}",
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("API/Users")).unwrap();
        fs::create_dir_all(path.join("Other")).unwrap();
        fs::write(path.join("API/Users/list.toml"), "id = 'list'\nname = 'List users'\nschema_version = 1\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/users'\n").unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn sidebar<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<CollectionPanel>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut sidebar = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            CollectionPanel::new(
                CollectionRegistry::from_path(&fixture.0).unwrap(),
                window,
                cx,
            )
        });
        sidebar = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.simulate_resize(size(px(300.), px(500.)));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (sidebar.unwrap(), cx)
}

fn click_row(
    cx: &mut VisualTestContext,
    selector: &'static str,
    button: MouseButton,
    count: usize,
) {
    let position = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_event(MouseDownEvent {
        button,
        position,
        click_count: count,
        modifiers: Modifiers::default(),
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        button,
        position,
        click_count: count,
        modifiers: Modifiers::default(),
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn enter_renames_and_editor_backspace_and_escape_do_not_delete(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    click_row(cx, "collection-row-2", MouseButton::Left, 2);
    assert!(cx.debug_bounds("sidebar-rename-editor").is_none());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.simulate_keystrokes("backspace");
    assert!(fixture.0.join("API/Users/list.toml").exists());
    cx.simulate_input("All users");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(sidebar.read(cx).rename.is_none());
        assert_eq!(sidebar.read(cx).tree.items[2].label, "All users");
    });
    assert!(
        fs::read_to_string(fixture.0.join("API/Users/list.toml"))
            .unwrap()
            .contains("All users")
    );

    click_row(cx, "collection-row-2", MouseButton::Left, 2);
    assert!(cx.debug_bounds("sidebar-rename-editor").is_none());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("Discard this");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(sidebar.read(cx).tree.items[2].label, "All users"));
    cx.update(|window, cx| assert!(sidebar.focus_handle(cx).is_focused(window)));

    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(fixture.0.join("API/Users/list.toml").exists());
    assert!(cx.debug_bounds("sidebar-delete-prompt").is_some());
    cx.update(|window, cx| assert!(sidebar.read(cx).delete_focus.is_focused(window)));
    cx.simulate_keystrokes("backspace backspace space");
    assert!(fixture.0.join("API/Users/list.toml").exists());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!fixture.0.join("API/Users/list.toml").exists());
    cx.read(|cx| {
        assert_eq!(sidebar.read(cx).tree.items[0].request_count, 0);
        assert_eq!(sidebar.read(cx).selected, Some(2));
    });
}

#[gpui_kit::test]
fn context_menu_targets_clicked_collection_and_can_rename_then_delete(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    click_row(cx, "collection-row-2", MouseButton::Left, 1);
    click_row(cx, "collection-row-3", MouseButton::Right, 1);
    cx.read(|cx| assert_eq!(sidebar.read(cx).selected, Some(3)));
    cx.simulate_keystrokes("down down down enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.simulate_input("Renamed");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(fixture.0.join("Renamed").is_dir());
    assert!(!fixture.0.join("Other").exists());
    click_row(cx, "collection-row-3", MouseButton::Right, 1);
    cx.simulate_keystrokes("down down down down down down enter");
    cx.run_until_parked();
    assert!(fixture.0.join("Renamed").exists());
    cx.update(|window, cx| assert!(sidebar.read(cx).delete_focus.is_focused(window)));
    click_row(cx, "cancel-sidebar-delete", MouseButton::Left, 1);
    cx.run_until_parked();
    assert!(fixture.0.join("Renamed").exists());
    assert!(cx.debug_bounds("sidebar-delete-prompt").is_none());
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    click_row(cx, "confirm-sidebar-delete", MouseButton::Left, 1);
    cx.run_until_parked();
    assert!(!fixture.0.join("Renamed").exists());
    assert!(fixture.0.join("API/Users/list.toml").exists());
    cx.read(|cx| assert_eq!(sidebar.read(cx).tree.roots.len(), 1));
}

#[gpui_kit::test]
fn rename_errors_allow_correction_and_search_backspace_keeps_files(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    click_row(cx, "collection-row-0", MouseButton::Left, 2);
    assert!(cx.debug_bounds("sidebar-rename-editor").is_none());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("Other");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(sidebar.read(cx).error.is_some());
        assert!(sidebar.read(cx).rename.is_some());
    });
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("Renamed API");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(fixture.0.join("Renamed API/Users/list.toml").exists());

    cx.update(|window, cx| sidebar.read(cx).search.focus_handle(cx).focus(window, cx));
    cx.simulate_input("List users");
    cx.run_until_parked();
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(fixture.0.join("Renamed API/Users/list.toml").exists());
    cx.simulate_keystrokes("down down down backspace");
    cx.run_until_parked();
    assert!(fixture.0.join("Renamed API/Users/list.toml").exists());
    cx.update(|window, cx| assert!(sidebar.read(cx).delete_focus.is_focused(window)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| assert!(sidebar.focus_handle(cx).is_focused(window)));
    assert!(cx.debug_bounds("sidebar-delete-prompt").is_none());
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    click_row(cx, "confirm-sidebar-delete", MouseButton::Left, 1);
    cx.run_until_parked();
    assert!(!fixture.0.join("Renamed API/Users/list.toml").exists());
    cx.read(|cx| {
        assert!(sidebar.read(cx).visible.is_empty());
        assert!(sidebar.read(cx).selected.is_none());
    });
}

#[gpui_kit::test]
fn moving_selection_or_filtering_cancels_pending_deletion(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    click_row(cx, "collection-row-2", MouseButton::Left, 1);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-delete-prompt").is_some());
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    cx.read(|cx| assert!(sidebar.read(cx).pending_delete.is_none()));
    assert!(fixture.0.join("API/Users/list.toml").exists());
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-delete-prompt").is_some());
    cx.update(|window, cx| sidebar.read(cx).search.focus_handle(cx).focus(window, cx));
    cx.run_until_parked();
    cx.read(|cx| assert!(sidebar.read(cx).pending_delete.is_none()));
    assert!(fixture.0.join("Other").exists());
}

#[gpui_kit::test]
fn creates_a_collection_folder_and_request_from_an_empty_sidebar(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fs::remove_dir_all(&fixture.0).unwrap();
    let (sidebar, cx) = sidebar(&fixture, cx);
    cx.update(|window, cx| sidebar.read(cx).search.focus_handle(cx).focus(window, cx));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let keystroke = Keystroke::parse("enter").unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.update(|window, cx| {
        let editor = &sidebar.read(cx).rename.as_ref().unwrap().input;
        assert!(editor.focus_handle(cx).is_focused(window));
    });
    cx.simulate_input("My API");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(fixture.0.join("My API").is_dir());

    click_row(cx, "collection-row-0", MouseButton::Right, 1);
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.simulate_input("Nested");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(fixture.0.join("My API/Nested").is_dir());

    click_row(cx, "collection-row-1", MouseButton::Right, 1);
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.simulate_input("My request");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let path = fixture.0.join("My API/Nested/New Request.toml");
    assert!(fs::read_to_string(path).unwrap().contains("My request"));
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(sidebar.tree.items[0].request_count, 1);
        assert_eq!(sidebar.tree.items[2].label, "My request");
    });
}

#[gpui_kit::test]
fn creation_clears_filter_expands_parent_and_keeps_default_name_on_escape(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    click_row(cx, "collection-row-0", MouseButton::Left, 1);
    cx.update(|window, cx| sidebar.read(cx).search.focus_handle(cx).focus(window, cx));
    cx.simulate_input("API");
    cx.run_until_parked();
    click_row(cx, "collection-row-0", MouseButton::Right, 1);
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("sidebar-rename-editor").is_some());
    cx.update(|window, cx| {
        let sidebar = sidebar.read(cx);
        assert!(sidebar.search.read(cx).value().is_empty());
        assert!(!sidebar.collapsed.contains(&0));
        let editor = &sidebar.rename.as_ref().unwrap().input;
        assert!(editor.focus_handle(cx).is_focused(window));
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(fixture.0.join("API/New Request.toml").exists());
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert_eq!(
            sidebar.tree.items[sidebar.selected.unwrap()].label,
            "New Request"
        );
    });
}
