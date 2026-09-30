use std::path::PathBuf;

use collection::RequestScripts;
use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::CollectionPage;
use crate::{TabPage as _, request_draft::tests::element_bounds};

fn page(cx: &mut TestAppContext) -> (Entity<CollectionPage>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });

    cx.add_window_view(|window, cx| {
        let mut page = CollectionPage::new(
            PathBuf::from("/collections/API"),
            "API".into(),
            [("base_url".into(), "https://api.test".into())].into(),
            RequestScripts::default(),
        );
        page.prepare(window, cx);
        page
    })
}

fn type_into(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
    let bounds = element_bounds(cx, selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.simulate_input(text);
}

#[gpui_kit::test]
fn edits_name_and_variables_until_saved(cx: &mut TestAppContext) {
    let (page, cx) = page(cx);
    assert!(cx.read(|cx| !page.read(cx).tab_state().dirty));

    type_into(cx, "collection-name", " v2");
    type_into(cx, "collection-variable-name-1", "token");
    type_into(cx, "collection-variable-value-1", "secret");

    let settings = cx.read(|cx| {
        let page = page.read(cx);
        assert!(page.tab_state().dirty);
        // The saved name remains the tab title until the edits are saved.
        assert_eq!(page.name(), "API");
        page.settings().unwrap()
    });
    assert_eq!(settings.name, "API v2");
    assert_eq!(
        settings.variables,
        [
            ("base_url".into(), "https://api.test".into()),
            ("token".into(), "secret".into())
        ]
    );
    // Typing in the last row adds another empty row.
    assert!(element_bounds(cx, "collection-variable-name-2").is_some());

    cx.update(|_, cx| {
        page.update(cx, |page, cx| {
            page.mark_saved(PathBuf::from("/collections/API v2"), settings, cx)
        })
    });
    cx.read(|cx| {
        let page = page.read(cx);
        assert!(!page.is_dirty());
        assert_eq!(page.name(), "API v2");
        assert_eq!(page.path, PathBuf::from("/collections/API v2"));
    });

    let remove = element_bounds(cx, "collection-variable-remove-0").unwrap();
    cx.simulate_click(remove.center(), Modifiers::default());
    cx.read(|cx| {
        let page = page.read(cx);
        assert!(page.is_dirty());
        assert_eq!(page.draft.variables, [("token".into(), "secret".into())]);
    });
}

#[gpui_kit::test]
fn invalid_or_repeated_variable_names_cannot_be_saved(cx: &mut TestAppContext) {
    let (page, cx) = page(cx);

    type_into(cx, "collection-variable-name-1", "has space");
    assert!(element_bounds(cx, "collection-variable-error").is_some());
    assert!(cx.read(|cx| page.read(cx).settings().is_err()));

    let remove = element_bounds(cx, "collection-variable-remove-1").unwrap();
    cx.simulate_click(remove.center(), Modifiers::default());
    type_into(cx, "collection-variable-name-1", "base_url");
    let error = cx.read(|cx| page.read(cx).settings().unwrap_err());
    assert!(error.contains("more than once"), "{error}");

    let remove = element_bounds(cx, "collection-variable-remove-1").unwrap();
    cx.simulate_click(remove.center(), Modifiers::default());
    type_into(cx, "collection-variable-name-1", "token");
    assert!(element_bounds(cx, "collection-variable-error").is_none());
    assert!(cx.read(|cx| page.read(cx).settings().is_ok()));
}

#[gpui_kit::test]
fn scripts_section_edits_collection_scripts(cx: &mut TestAppContext) {
    // Script assistance uses the shared TypeScript worker outside GPUI's test executor.
    cx.executor().allow_parking();
    let (page, cx) = page(cx);

    let scripts = element_bounds(cx, "collection-section-Scripts").unwrap();
    cx.simulate_click(scripts.center(), Modifiers::default());
    assert!(element_bounds(cx, "collection-scripts").is_some());
    assert!(element_bounds(cx, "collection-variable-table").is_none());

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            let editor = page
                .script_editor(cx)
                .update(cx, |scripts, cx| scripts.editor(window, cx));
            editor.update(cx, |editor, cx| {
                editor.replace_all("pm.variables.set('from', 'collection');", window, cx)
            });
        });
    });

    cx.read(|cx| {
        let page = page.read(cx);
        assert!(page.is_dirty());
        assert_eq!(
            page.settings().unwrap().scripts.pre_request,
            "pm.variables.set('from', 'collection');"
        );
    });
}

#[gpui_kit::test]
fn sidebar_renames_keep_an_unsaved_name(cx: &mut TestAppContext) {
    let (page, cx) = page(cx);

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.relocate("/collections/Renamed".into(), "Renamed".into(), window, cx)
        })
    });
    cx.read(|cx| {
        let page = page.read(cx);
        assert!(!page.is_dirty());
        assert_eq!(page.settings().unwrap().name, "Renamed");
    });

    type_into(cx, "collection-name", " draft");
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.relocate("/collections/Again".into(), "Again".into(), window, cx)
        })
    });
    cx.read(|cx| {
        let page = page.read(cx);
        assert_eq!(page.name(), "Again");
        assert_eq!(page.path, PathBuf::from("/collections/Again"));
        assert_eq!(page.settings().unwrap().name, "Renamed draft");
    });
}
