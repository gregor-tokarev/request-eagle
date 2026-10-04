use std::path::PathBuf;

use gpui_kit::{Bounds, point, px, size};

use crate::session::{SavedFile, SavedSidebar, SavedTab, Session, SessionWriter, fit_bounds};

fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<gpui_kit::Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

#[test]
fn window_that_fits_its_display_keeps_its_bounds() {
    let display = bounds(0., 0., 1920., 1080.);
    let window = bounds(100., 50., 1200., 800.);

    assert_eq!(fit_bounds(window, display), window);
}

#[test]
fn window_moves_back_onto_a_display_that_became_smaller() {
    let display = bounds(0., 0., 1280., 800.);

    assert_eq!(
        fit_bounds(bounds(900., 600., 1200., 700.), display),
        bounds(80., 100., 1200., 700.),
    );
    assert_eq!(
        fit_bounds(bounds(-300., -40., 2560., 1440.), display),
        display,
    );
}

#[test]
fn missing_or_damaged_session_opens_like_the_first_launch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");

    let missing = Session::load(path.clone());
    assert!(missing.window.is_none());
    assert!(missing.tabs.is_empty());
    assert!(missing.sidebar.visible && missing.sidebar.collections);

    std::fs::write(&path, "{ not json").unwrap();
    let damaged = Session::load(path);
    assert!(damaged.tabs.is_empty());
    assert!(damaged.sidebar.environments && damaged.sidebar.history);
}

#[test]
fn sidebar_parts_left_out_of_the_file_are_shown() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    std::fs::write(&path, r#"{ "sidebar": { "history": false } }"#).unwrap();

    let sidebar = Session::load(path).sidebar;

    assert!(sidebar.visible && sidebar.collections && sidebar.environments);
    assert!(!sidebar.history);
}

#[test]
fn flow_tabs_of_flows_in_collections_keep_the_other_tabs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    std::fs::write(
        &path,
        r#"{ "tabs": [
            { "kind": "flow", "file": { "path": "/collections/api/Checkout.toml", "id": "f1", "collection": "/collections/api" } },
            { "kind": "cookies" }
        ], "selected_tab": 1 }"#,
    )
    .unwrap();

    let session = Session::load(path);

    assert!(matches!(
        &session.tabs[..],
        [SavedTab::Flow { path, id, draft: None }, SavedTab::Cookies]
            if path.as_os_str().is_empty() && id.is_empty()
    ));
    assert_eq!(session.selected_tab, Some(1));
}

#[test]
fn saved_session_loads_back_the_same() {
    let directory = tempfile::tempdir().unwrap();
    // The directory is created on the first save.
    let path = directory.path().join(".request-eagle/session.json");
    let session = Session {
        path: path.clone(),
        window: serde_json::from_value(serde_json::json!({
            "display": "6f1c2a1e-0d3b-4c39-9a51-0c5e8d5c2b7f",
            "bounds": {
                "fullscreen": {
                    "origin": { "x": 20.0, "y": 40.0 },
                    "size": { "width": 1200.0, "height": 800.0 },
                },
            },
        }))
        .unwrap(),
        sidebar: SavedSidebar {
            visible: false,
            collections: true,
            environments: false,
            flows: false,
            history: true,
            collapsed: vec![
                PathBuf::from("/collections/api"),
                PathBuf::from("/collections/billing/Invoices"),
            ],
        },
        tabs: vec![
            SavedTab::Request {
                title: "Get user".into(),
                file: Some(SavedFile {
                    path: PathBuf::from("/collections/api/get-user.toml"),
                    id: "2d0c5b8e-7a8f-4a52-9d1b-3c4e5f6a7b8c".into(),
                    collection: PathBuf::from("/collections/api"),
                }),
                name: None,
                draft: None,
            },
            SavedTab::Request {
                title: "Login".into(),
                file: None,
                name: Some("Login".into()),
                draft: Some(
                    request::HttpRequest {
                        path: "https://example.com/login".into(),
                        ..Default::default()
                    }
                    .into(),
                ),
            },
            SavedTab::Collection {
                path: PathBuf::from("/collections/api"),
            },
            SavedTab::Flow {
                path: PathBuf::from("/flows/Checkout.toml"),
                id: "6c1d2e3f-4a5b-4c6d-8e7f-9a0b1c2d3e4f".into(),
                draft: Some(Box::new(flow::Flow::starter())),
            },
            SavedTab::Environment {
                name: "Staging".into(),
            },
            SavedTab::Cookies,
        ],
        selected_tab: Some(1),
    };

    SessionWriter::default().save(1, &session);
    let loaded = Session::load(path);

    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&session).unwrap(),
    );
    assert!(loaded.window.is_some());
    assert_eq!(loaded.sidebar.collapsed.len(), 2);
    assert_eq!(loaded.tabs.len(), 6);
}

#[test]
fn session_captured_earlier_does_not_replace_a_later_one() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let session = |tabs| Session {
        path: path.clone(),
        tabs,
        ..Session::default()
    };
    let writer = SessionWriter::default();

    // Saves in the background can finish after the one that followed them.
    writer.save(2, &session(vec![SavedTab::Cookies]));
    writer.save(1, &session(Vec::new()));

    assert!(matches!(
        &Session::load(path.clone()).tabs[..],
        [SavedTab::Cookies]
    ));

    writer.save(3, &session(Vec::new()));

    assert!(Session::load(path).tabs.is_empty());
}

#[test]
fn unchanged_session_is_not_written_again() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let session = |tabs| Session {
        path: path.clone(),
        tabs,
        ..Session::default()
    };
    let writer = SessionWriter::default();
    writer.save(1, &session(vec![SavedTab::Cookies]));
    std::fs::remove_file(&path).unwrap();

    writer.save(2, &session(vec![SavedTab::Cookies]));
    assert!(!path.exists());

    writer.save(3, &session(Vec::new()));
    assert!(path.exists());
}

#[test]
fn session_that_could_not_be_written_is_tried_again() {
    let directory = tempfile::tempdir().unwrap();
    // A file where the session's directory should be fails the save.
    let blocked = directory.path().join("blocked");
    std::fs::write(&blocked, "").unwrap();
    let path = blocked.join("session.json");
    let session = Session {
        path: path.clone(),
        tabs: vec![SavedTab::Cookies],
        ..Session::default()
    };
    let writer = SessionWriter::default();
    writer.save(1, &session);

    std::fs::remove_file(&blocked).unwrap();
    writer.save(2, &session);

    assert!(matches!(&Session::load(path).tabs[..], [SavedTab::Cookies]));
}
