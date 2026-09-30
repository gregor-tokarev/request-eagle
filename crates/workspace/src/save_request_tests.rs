use std::fs;

use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use crate::main_view::MainView;
use crate::tests::{init, no_environments, workspace};

fn setup<'a>(
    directory: &TempDir,
    cx: &'a mut TestAppContext,
) -> (Entity<MainView>, &'a mut VisualTestContext) {
    init(cx);
    let registry = collection::CollectionRegistry::from_path(directory.path()).unwrap();
    let (workspace, cx) = workspace(registry, no_environments(), cx);
    let main = cx.read(|cx| workspace.read(cx).main_view.clone());

    click(cx, "request-url");
    cx.simulate_input("https://example.test/draft");

    (main, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    // Mount the dialog, then paint its reduced-motion position before hit testing.
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    crate::tests::click(cx, selector);
}

#[gpui_kit::test]
fn save_modal_cancels_without_changes_then_saves_same_tab_to_nested_folder(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("API/Users")).unwrap();
    let (main, cx) = setup(&directory, cx);
    let draft = cx.read(|cx| main.read(cx).tabs[0].draft());
    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("save-request-dialog").is_some());
    click(cx, "cancel-save-request");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
    cx.read(|cx| assert!(draft.read(cx).is_dirty()));
    assert_eq!(
        fs::read_dir(directory.path().join("API/Users"))
            .unwrap()
            .count(),
        0
    );

    cx.simulate_keystrokes("secondary-s");
    click(cx, "save-request-name");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Create user");
    assert!(cx.debug_bounds("save-destination-1").is_none());
    click(cx, "save-destination-0");
    click(cx, "save-destination-1");
    click(cx, "save-location-0");
    assert!(cx.debug_bounds("save-destination-1").is_some());
    click(cx, "save-location-root");
    click(cx, "save-destination-0");
    click(cx, "save-destination-1");
    click(cx, "confirm-save-request");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
    let file =
        collection::FileEntry::from_path(directory.path().join("API/Users/Create user.toml"))
            .unwrap();
    let request::Request::Http(saved) = file.request else {
        panic!("expected an HTTP request");
    };
    cx.read(|cx| {
        assert_eq!(main.read(cx).tabs.len(), 1);
        assert_eq!(main.read(cx).tabs[0].draft(), draft);
        assert_eq!(draft.read(cx).request, saved);
        assert!(!draft.read(cx).is_dirty());
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().name,
            "Create user"
        );
        assert_eq!(
            draft.read(cx).location.as_ref().unwrap().folders,
            vec![gpui_kit::SharedString::from("Users")]
        );
    });
    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
}

#[gpui_kit::test]
fn save_and_close_can_create_a_collection_and_does_not_close_on_failure(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (main, cx) = setup(&directory, cx);
    cx.simulate_keystrokes("secondary-w");
    click(cx, "save-and-close-request");
    assert!(cx.debug_bounds("save-request-dialog").is_some());
    click(cx, "save-new-collection");
    let parent = directory.path().join("New Collection");
    fs::remove_dir(&parent).unwrap();
    click(cx, "confirm-save-request");
    assert!(cx.debug_bounds("save-request-error").is_some());
    cx.read(|cx| assert_eq!(main.read(cx).tabs.len(), 1));
    fs::create_dir(&parent).unwrap();
    click(cx, "confirm-save-request");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
    cx.read(|cx| assert!(main.read(cx).tabs.is_empty()));
    assert_eq!(
        collection::CollectionRegistry::from_path(directory.path())
            .unwrap()
            .collections()[0]
            .entries
            .len(),
        1
    );
}

#[gpui_kit::test]
fn picker_navigates_multiple_folder_levels_and_filters_each_level(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("API/Users/Active")).unwrap();
    let (_, cx) = setup(&directory, cx);
    cx.simulate_keystrokes("secondary-s");
    click(cx, "save-destination-0");
    assert!(cx.debug_bounds("save-destination-2").is_none());
    click(cx, "save-location-filter");
    cx.simulate_input("missing");
    assert!(cx.debug_bounds("save-destination-1").is_none());
    click(cx, "save-location-root");
    click(cx, "save-destination-0");
    click(cx, "save-destination-1");
    click(cx, "save-destination-2");
    assert!(cx.debug_bounds("save-location-2").is_some());
    click(cx, "save-request-name");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Nested request");
    click(cx, "confirm-save-request");
    assert!(
        directory
            .path()
            .join("API/Users/Active/Nested request.toml")
            .exists()
    );
    assert!(
        !directory
            .path()
            .join("API/Users/Nested request.toml")
            .exists()
    );
}
