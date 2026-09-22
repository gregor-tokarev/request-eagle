use crate::{AppearanceMode, Preferences, init, load, update};
use gpui_kit::TestAppContext;
use std::fs;

#[gpui_kit::test]
fn saves_and_reloads_the_shared_document(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");

    cx.update(|cx| {
        load(directory.path(), cx).unwrap();
        assert_eq!(
            cx.try_global::<Preferences>().cloned().unwrap_or_default(),
            Preferences::default()
        );
        assert!(!path.exists());

        update(cx, |preferences| {
            preferences.appearance.mode = AppearanceMode::System;
            preferences.appearance.dark_theme = "Catppuccin Mocha".into();
            preferences.appearance.editor_font = "Menlo".into();
            preferences.request.follow_all_redirects = false;
        })
        .unwrap();
        update(cx, |preferences| {
            preferences.appearance.interface_font_size = 20.
        })
        .unwrap();

        let expected = cx.try_global::<Preferences>().cloned().unwrap_or_default();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(document["appearance"]["editor_font"], "Menlo");
        assert_eq!(document["appearance"]["interface_font_size"], 20.);
        assert_eq!(document["request"]["follow_all_redirects"], false);
        cx.set_global(Preferences::default());
        load(directory.path(), cx).unwrap();
        assert_eq!(
            cx.try_global::<Preferences>().cloned().unwrap_or_default(),
            expected
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    });
}

#[gpui_kit::test]
fn partial_documents_default_missing_fields_and_preserve_supplied_values(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("preferences.json"),
        r#"{"appearance":{"interface_font_size":99,"editor_font":"  Menlo  "}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        load(directory.path(), cx).unwrap();
        let appearance = cx
            .try_global::<Preferences>()
            .cloned()
            .unwrap_or_default()
            .appearance;
        assert_eq!(appearance.interface_font_size, 99.);
        assert_eq!(appearance.editor_font, "  Menlo  ");
        assert_eq!(appearance.mode, AppearanceMode::Dark);
        assert_eq!(appearance.dark_theme, "Ayu Dark");

        update(cx, |preferences| {
            preferences.appearance.interface_font_size = 30.
        })
        .unwrap();
        load(directory.path(), cx).unwrap();

        assert_eq!(
            cx.try_global::<Preferences>()
                .cloned()
                .unwrap_or_default()
                .appearance
                .interface_font_size,
            30.
        );
    });
}

#[gpui_kit::test]
fn failed_save_does_not_publish_changes_or_leave_temporary_files(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");

    cx.update(|cx| {
        load(directory.path(), cx).unwrap();
        update(cx, |preferences| {
            preferences.appearance.editor_font = "Menlo".into()
        })
        .unwrap();
        let original = cx.try_global::<Preferences>().cloned().unwrap_or_default();

        // A directory at the target prevents the final atomic replacement.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(
            update(cx, |preferences| preferences.appearance.mode =
                AppearanceMode::Light)
            .is_err()
        );
        assert_eq!(
            cx.try_global::<Preferences>().cloned().unwrap_or_default(),
            original
        );
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    });
}

#[gpui_kit::test]
fn malformed_file_is_not_overwritten_and_can_be_reloaded_after_repair(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    fs::write(&path, "invalid json").unwrap();

    cx.update(|cx| {
        init(cx);
        update(cx, |preferences| {
            preferences.appearance.interface_font_size = 18.
        })
        .unwrap();
        let original = cx.try_global::<Preferences>().cloned().unwrap_or_default();
        assert!(load(directory.path(), cx).is_err());
        assert!(
            update(cx, |preferences| preferences.appearance.mode =
                AppearanceMode::Light)
            .is_err()
        );
        assert_eq!(
            cx.try_global::<Preferences>().cloned().unwrap_or_default(),
            original
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid json");

        fs::write(&path, "{}").unwrap();
        load(directory.path(), cx).unwrap();
        update(cx, |preferences| {
            preferences.appearance.interface_font_size = 19.
        })
        .unwrap();
        load(directory.path(), cx).unwrap();
        assert_eq!(
            cx.try_global::<Preferences>()
                .cloned()
                .unwrap_or_default()
                .appearance
                .interface_font_size,
            19.
        );
    });
}

#[gpui_kit::test]
fn updates_can_run_without_a_storage_directory(cx: &mut TestAppContext) {
    cx.update(|cx| {
        update(cx, |preferences| {
            preferences.appearance.editor_font = "Menlo".into()
        })
        .unwrap();
        init(cx);
        assert_eq!(
            cx.try_global::<Preferences>()
                .cloned()
                .unwrap_or_default()
                .appearance
                .editor_font,
            "Menlo"
        );
    });
}
