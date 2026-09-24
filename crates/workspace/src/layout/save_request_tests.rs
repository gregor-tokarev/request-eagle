use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::main_view::MainView;
use gpui_kit::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext, component::Root};
use tab_ui::RequestDraft;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
                "request-eagle-save-modal-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<MainView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    let registry = collection::CollectionRegistry::from_path(&fixture.0).unwrap();
    let mut main = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let layout = cx.new(|cx| {
            crate::workspace::Layout::new(registry, updater::init("1.2.3", cx), window, cx)
        });
        main = Some(layout.read(cx).main_view.clone());
        Root::new(layout, window, cx)
    });
    click(cx, "request-url");
    cx.simulate_input("https://example.test/draft");
    (main.unwrap(), cx)
}
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    // Mount the dialog, then paint its reduced-motion position before hit testing.
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
}

#[gpui_kit::test]
fn save_modal_cancels_without_changes_then_saves_same_tab_to_nested_folder(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("API/Users")).unwrap();
    let (main, cx) = setup(&fixture, cx);
    let draft = cx.read(|cx| {
        main.read(cx).tabs[0]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap()
    });
    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("save-request-dialog").is_some());
    click(cx, "cancel-save-request");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
    cx.read(|cx| assert!(draft.read(cx).is_dirty()));
    assert_eq!(
        fs::read_dir(fixture.0.join("API/Users")).unwrap().count(),
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
        collection::FileEntry::from_path(fixture.0.join("API/Users/Create user.toml")).unwrap();
    let collection::Request::Http(saved) = file.request;
    cx.read(|cx| {
        assert_eq!(main.read(cx).tabs.len(), 1);
        assert_eq!(
            main.read(cx).tabs[0].page.view().entity_id(),
            draft.entity_id()
        );
        assert_eq!(draft.read(cx).request, saved);
        assert!(!draft.read(cx).is_dirty());
        assert_eq!(draft.read(cx).name, "Create user");
        assert_eq!(
            draft.read(cx).folders,
            vec![gpui_kit::SharedString::from("Users")]
        );
    });
    cx.simulate_keystrokes("secondary-s");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
}

#[gpui_kit::test]
fn save_and_close_can_create_a_collection_and_does_not_close_on_failure(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (main, cx) = setup(&fixture, cx);
    cx.simulate_keystrokes("secondary-w");
    click(cx, "save-and-close-request");
    assert!(cx.debug_bounds("save-request-dialog").is_some());
    click(cx, "save-new-collection");
    let parent = fixture.0.join("New Collection");
    fs::remove_dir(&parent).unwrap();
    click(cx, "confirm-save-request");
    assert!(cx.debug_bounds("save-request-error").is_some());
    cx.read(|cx| assert_eq!(main.read(cx).tabs.len(), 1));
    fs::create_dir(&parent).unwrap();
    click(cx, "confirm-save-request");
    assert!(cx.debug_bounds("save-request-dialog").is_none());
    cx.read(|cx| assert!(main.read(cx).tabs.is_empty()));
    assert_eq!(
        collection::CollectionRegistry::from_path(&fixture.0)
            .unwrap()
            .collections()[0]
            .entries
            .len(),
        1
    );
}

#[gpui_kit::test]
fn picker_navigates_multiple_folder_levels_and_filters_each_level(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("API/Users/Active")).unwrap();
    let (_, cx) = setup(&fixture, cx);
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
        fixture
            .0
            .join("API/Users/Active/Nested request.toml")
            .exists()
    );
    assert!(!fixture.0.join("API/Users/Nested request.toml").exists());
}
