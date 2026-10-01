use std::{fs, path::Path};

use collection::CollectionRegistry;
use gpui_kit::component::Root;
use gpui_kit::{
    AppContext, Context, Entity, Focusable, IntoElement, Modifiers, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, px, size,
};

use super::CollectionPanel;

const POSTMAN: &str = r#"{
    "info": {"name": "Pet Store"},
    "item": [
        {"name": "Pets", "item": [
            {"name": "List pets", "request": {"method": "GET", "url": "https://pets.test/pets"}}
        ]},
        {"name": "Lock pet", "request": {"method": "LOCK", "url": "https://pets.test/pets/1"}}
    ]
}"#;

/// Shows the sidebar and its dialogs, as the workspace does.
struct Host(Entity<CollectionPanel>);

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.0.clone())
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn sidebar<'a>(
    directory: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<CollectionPanel>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::init(cx);
        // The dialog then opens without animating, so it can be clicked at once.
        cx.set_reduce_motion(true);
    });

    let collections = CollectionRegistry::from_path(directory);
    let mut sidebar = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| CollectionPanel::new(collections, window, cx));
        sidebar = Some(view.clone());
        let host = cx.new(|_| Host(view));

        Root::new(host, window, cx)
    });
    // Wide enough to show the whole dialog.
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.update(|window, _| window.activate_window());

    (sidebar.unwrap(), cx)
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

    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

fn choose(sidebar: &Entity<CollectionPanel>, cx: &mut VisualTestContext, file: &Path) {
    cx.update(|window, cx| {
        sidebar.update(cx, |sidebar, cx| sidebar.open_import_dialog(window, cx))
    });
    click(cx, "choose-import-file");

    let file = file.to_path_buf();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && options.directories && !options.multiple);
        Some(vec![file])
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui_kit::test]
fn importing_a_file_adds_and_selects_a_collection(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("export.json");
    fs::write(
        &file,
        r#"{"info": {"name": "Pet Store"}, "item": [
            {"name": "Pets", "item": [
                {"name": "List pets", "request": {"method": "GET", "url": "https://pets.test/pets"}}
            ]}
        ]}"#,
    )
    .unwrap();
    let collections = directory.path().join("collections");
    let (sidebar, cx) = sidebar(&collections, cx);

    choose(&sidebar, cx, &file);

    assert!(cx.debug_bounds("import-dialog").is_none());
    assert!(collections.join("Pet Store/Pets/List pets.toml").is_file());
    cx.update(|window, cx| {
        let sidebar = sidebar.read(cx);
        let selected = sidebar
            .selected
            .expect("the imported collection is selected");

        assert_eq!(sidebar.tree.items[selected].label, "Pet Store");
        // Its folder is collapsed, so the request row is hidden.
        assert_eq!(sidebar.visible.len(), 2);
        assert!(sidebar.focus_handle(cx).is_focused(window));
    });
}

#[gpui_kit::test]
fn importing_a_postman_folder_keeps_its_grpc_requests(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("Shop");
    fs::create_dir_all(folder.join(".resources")).unwrap();
    fs::write(
        folder.join(".resources/definition.yaml"),
        "$kind: collection\n",
    )
    .unwrap();
    fs::write(
        folder.join("Get product.request.yaml"),
        "$kind: grpc-request\nurl: localhost:50051\nmethodPath: shop.ProductService.GetProduct\n",
    )
    .unwrap();
    let collections = directory.path().join("collections");
    let (sidebar, cx) = sidebar(&collections, cx);

    choose(&sidebar, cx, &folder);

    assert!(cx.debug_bounds("import-dialog").is_none());
    let saved = fs::read_to_string(collections.join("Shop/Get product.toml")).unwrap();
    assert!(saved.contains("method = \"shop.ProductService/GetProduct\""));
}

#[gpui_kit::test]
fn skipped_requests_are_listed_after_importing(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("export.json");
    fs::write(&file, POSTMAN).unwrap();
    let collections = directory.path().join("collections");
    let (sidebar, cx) = sidebar(&collections, cx);

    choose(&sidebar, cx, &file);

    assert!(cx.debug_bounds("import-summary").is_some());
    cx.read(|cx| assert_eq!(sidebar.read(cx).tree.roots.len(), 1));

    click(cx, "close-import");
    assert!(cx.debug_bounds("import-summary").is_none());
}

#[gpui_kit::test]
fn unsupported_files_keep_the_dialog_open_with_an_error(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("notes.txt");
    fs::write(&file, "just some notes").unwrap();
    let collections = directory.path().join("collections");
    let (sidebar, cx) = sidebar(&collections, cx);

    choose(&sidebar, cx, &file);

    assert!(cx.debug_bounds("import-dialog").is_some());
    assert!(cx.debug_bounds("import-error").is_some());
    cx.read(|cx| assert!(sidebar.read(cx).tree.roots.is_empty()));
    assert!(!collections.exists());
}
