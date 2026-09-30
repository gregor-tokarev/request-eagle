use std::{fs, time::Duration};

use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
use smol::io::{AsyncReadExt, AsyncWriteExt};
use tab_ui::CollectionPage;

use super::request_tab_tests::{SavedRequestFixture, click, edit_url};

#[gpui_kit::test]
async fn saved_collection_settings_rename_it_and_apply_to_its_requests(cx: &mut TestAppContext) {
    // Script assistance uses the shared TypeScript worker outside GPUI's test executor.
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/items", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8(head).unwrap()
    });

    let fixture = SavedRequestFixture::new();
    let (tabs, draft, cx) = fixture.open(cx);

    // A click only expands or collapses the collection.
    click(cx, "collection-row-0");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 2));

    double_click(cx, "collection-row-0");
    let page = cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        assert_eq!(tabs.selected, Some(2));
        tabs.tabs[2]
            .page
            .view()
            .downcast::<CollectionPage>()
            .ok()
            .unwrap()
    });
    // Opening it again selects the existing tab.
    double_click(cx, "collection-row-0");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 3));

    click(cx, "collection-name");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("Renamed API");
    click(cx, "collection-variable-name-0");
    cx.simulate_input("token");
    click(cx, "collection-variable-value-0");
    cx.simulate_input("collection-secret");
    click(cx, "collection-section-Scripts");
    click(cx, "script-editor");
    cx.simulate_input(
        "pm.request.headers.upsert({key: 'X-Collection', value: pm.environment.get('token')});",
    );
    assert!(cx.debug_bounds("tab-dirty-3").is_some());

    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    assert!(cx.debug_bounds("request-save-error").is_none());

    let collection = fixture.directory.join("Renamed API");
    assert!(!fixture.directory.join("API").exists());
    assert!(
        fs::read_to_string(collection.join("environment.toml"))
            .unwrap()
            .contains("token = \"collection-secret\"")
    );
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs[2].title, "Renamed API");
        assert!(!page.read(cx).is_dirty());
        assert_eq!(page.read(cx).path, collection);
        // The open request follows its collection's new directory.
        assert_eq!(
            tabs.tabs[1].request_path.as_ref(),
            Some(&collection.join("example.toml"))
        );
    });

    tabs.update(cx, |tabs, cx| tabs.select_tab(1, cx));
    edit_url(cx, &url);
    click(cx, "send-request");
    let head = smol::future::or(server, async {
        smol::Timer::after(Duration::from_secs(10)).await;
        panic!("the request was not sent");
    })
    .await;
    assert!(
        head.to_lowercase()
            .contains("x-collection: collection-secret"),
        "{head}"
    );
    cx.read(|cx| assert!(draft.read(cx).request.headers.is_empty()));
}

#[gpui_kit::test]
fn deleting_a_collection_closes_its_tab(cx: &mut TestAppContext) {
    let fixture = SavedRequestFixture::new();
    let (tabs, _, cx) = fixture.open(cx);

    double_click(cx, "collection-row-0");
    cx.read(|cx| assert_eq!(tabs.read(cx).tabs.len(), 3));
    cx.simulate_keystrokes("backspace");
    click(cx, "confirm-sidebar-delete");

    assert!(!fixture.directory.join("API").exists());
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 2);
        assert!(
            tabs.tabs
                .iter()
                .all(|tab| tab.page.view().downcast::<CollectionPage>().is_err())
        );
    });
}

fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let position = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
        .center();

    for click_count in 1..=2 {
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count,
            ..Default::default()
        });
        cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position,
            click_count,
            ..Default::default()
        });
    }
}
