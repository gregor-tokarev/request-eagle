use crate::workspace::Layout;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    path: &std::path::Path,
) -> (Entity<Layout>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    cx.add_window_view(|window, cx| {
        Layout::new(
            collection::CollectionRegistry::from_path(path).unwrap(),
            updater::init("1.2.3", cx),
            window,
            cx,
        )
    })
}

fn call(
    layout: &Entity<Layout>,
    cx: &mut VisualTestContext,
    command: Value,
) -> Result<Value, String> {
    let command = serde_json::from_value(command).unwrap();
    cx.update(|window, cx| {
        layout.update(cx, |layout, cx| {
            layout.automation_command(command, window, cx)
        })
    })
}

#[gpui_kit::test]
fn automation_edits_live_draft_saves_and_relocates_without_losing_changes(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let (layout, cx) = setup(cx, temp.path());
    let collection =
        call(&layout, cx, json!({"command":"collections.create"})).unwrap()["path"].clone();
    let tab = call(&layout, cx, json!({"command":"tabs.new"})).unwrap()["tab"].clone();
    let request = json!({"method":"POST","url":"https://example.test/first","body":"{\"a\":1}","headers":[["X-Test","1"]]});
    call(
        &layout,
        cx,
        json!({"command":"drafts.set","tab":tab,"request":request}),
    )
    .unwrap();
    assert!(call(&layout, cx, json!({"command":"tabs.close","tab":tab})).is_err());
    let saved = call(
        &layout,
        cx,
        json!({"command":"drafts.save","tab":tab,"parent":collection,"name":"First"}),
    )
    .unwrap();
    let path = saved["path"].clone();
    assert!(std::path::Path::new(path.as_str().unwrap()).exists());
    assert_eq!(
        call(&layout, cx, json!({"command":"drafts.get","tab":tab})).unwrap()["dirty"],
        false
    );
    let edited = json!({"method":"GET","url":"https://example.test/unsaved"});
    call(
        &layout,
        cx,
        json!({"command":"drafts.set","tab":tab,"request":edited}),
    )
    .unwrap();
    call(&layout, cx, json!({"command":"requests.open","path":path})).unwrap();
    assert_eq!(
        call(&layout, cx, json!({"command":"drafts.get","tab":tab})).unwrap()["request"]["url"],
        edited["url"]
    );
    let renamed = call(
        &layout,
        cx,
        json!({"command":"entries.rename","path":path,"name":"Renamed"}),
    )
    .unwrap()["path"]
        .clone();
    cx.run_until_parked();
    let tabs = call(&layout, cx, json!({"command":"tabs.list"})).unwrap();
    let open = tabs
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == tab)
        .unwrap();
    assert_eq!(open["path"], renamed);
    assert_eq!(open["dirty"], true);
    assert!(
        call(
            &layout,
            cx,
            json!({"command":"entries.delete","path":renamed,"confirm":false})
        )
        .is_err()
    );
    call(&layout, cx, json!({"command":"drafts.save","tab":tab})).unwrap();
    assert_eq!(
        call(
            &layout,
            cx,
            json!({"command":"requests.get","path":renamed})
        )
        .unwrap()["request"]["url"],
        edited["url"]
    );
    call(&layout, cx, json!({"command":"tabs.close","tab":tab})).unwrap();
}

#[gpui_kit::test]
fn automation_script_trust_and_invalid_settings_leave_state_intact(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let (layout, cx) = setup(cx, temp.path());
    call(&layout, cx, json!({"command":"drafts.set","tab":1,"request":{"method":"GET","url":"http://127.0.0.1:1","pre_request":"console.log('test')"}})).unwrap();
    let error = call(&layout, cx, json!({"command":"requests.send","tab":1})).unwrap_err();
    assert!(error.contains("trust_scripts"));
    assert_eq!(
        call(&layout, cx, json!({"command":"drafts.get","tab":1})).unwrap()["sending"],
        false
    );
    let before = call(&layout, cx, json!({"command":"settings.get"})).unwrap();
    assert!(
        call(
            &layout,
            cx,
            json!({"command":"settings.appearance","interface_font_size":100})
        )
        .is_err()
    );
    assert_eq!(
        call(&layout, cx, json!({"command":"settings.get"})).unwrap(),
        before
    );
    assert!(
        call(
            &layout,
            cx,
            json!({"command":"responses.get","tab":1,"limit":9999999})
        )
        .is_err()
    );
    assert!(call(&layout, cx, json!({"command":"drafts.get","tab":9999})).is_err());
}
