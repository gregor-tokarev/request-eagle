use environment::GlobalEnvironments;
use gpui_kit::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext};

use crate::{EnvironmentEditor, Environments};

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<EnvironmentEditor>,
    Entity<Environments>,
    &mut VisualTestContext,
    tempfile::TempDir,
) {
    let directory = tempfile::tempdir().unwrap();
    let catalog = GlobalEnvironments::new(directory.path());
    catalog.create("Staging").unwrap();
    std::fs::write(catalog.path("Staging"), "host = 'staging.example.com'\n").unwrap();

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });

    let environments = cx.update(|cx| cx.new(|_| Environments::new(catalog, None)));
    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut editor = EnvironmentEditor::new("Staging".into(), environments.clone(), cx);
            editor.prepare(window, cx);
            editor
        });
        editor = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });

    (editor.unwrap(), environments, cx, directory)
}

fn type_into(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
    cx.update(|window, _| window.refresh());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(text);
}

#[gpui_kit::test]
fn saves_added_variables_and_rejects_duplicates(cx: &mut TestAppContext) {
    let (editor, environments, cx, directory) = setup(cx);
    let path = directory.path().join("Staging.toml");

    type_into(cx, "environment-key-1", "token");
    type_into(cx, "environment-value-1", "secret");
    cx.read(|cx| assert!(editor.read(cx).is_dirty()));

    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.save(cx)))
        .unwrap();
    let saved = environment::Environment::from_file(&path).unwrap();
    assert_eq!(saved.resolve("host"), Some("staging.example.com"));
    assert_eq!(saved.resolve("token"), Some("secret"));
    cx.read(|cx| assert!(!editor.read(cx).is_dirty()));

    type_into(cx, "environment-key-2", "token");
    let error = cx
        .update(|_, cx| editor.update(cx, |editor, cx| editor.save(cx)))
        .unwrap_err();
    assert!(error.contains("more than once"), "{error}");
    assert_eq!(
        environment::Environment::from_file(&path)
            .unwrap()
            .resolve("token"),
        Some("secret")
    );

    cx.read(|cx| assert_eq!(environments.read(cx).names(), ["Staging"]));
}

#[gpui_kit::test]
fn renames_the_environment_from_its_name_field(cx: &mut TestAppContext) {
    let (editor, environments, cx, directory) = setup(cx);
    cx.update(|_, cx| {
        environments.update(cx, |environments, cx| {
            environments.set_active(Some("Staging".into()), cx)
        })
    });

    type_into(cx, "environment-name", "QA");
    cx.simulate_keystrokes("enter");

    cx.read(|cx| {
        assert_eq!(editor.read(cx).name, "QA");
        assert_eq!(environments.read(cx).names(), ["QA"]);
        assert_eq!(environments.read(cx).active().unwrap(), "QA");
    });
    assert!(directory.path().join("QA.toml").exists());
    assert!(!directory.path().join("Staging.toml").exists());
}

#[gpui_kit::test]
fn does_not_overwrite_an_environment_file_it_could_not_read(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let catalog = GlobalEnvironments::new(directory.path());
    catalog.create("Broken").unwrap();
    std::fs::write(catalog.path("Broken"), "host = [").unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
    });

    let error = cx.update(|cx| {
        let environments = cx.new(|_| Environments::new(catalog.clone(), None));
        let editor = cx.new(|cx| EnvironmentEditor::new("Broken".into(), environments, cx));
        editor.update(cx, |editor, cx| editor.save(cx)).unwrap_err()
    });

    assert!(error.contains("Fix the environment file"), "{error}");
    assert_eq!(
        std::fs::read_to_string(catalog.path("Broken")).unwrap(),
        "host = ["
    );
}
