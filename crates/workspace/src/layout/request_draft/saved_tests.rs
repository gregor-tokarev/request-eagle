use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use collection::{CollectionRegistry, Method};
use gpui_kit::{Modifiers, TestAppContext};
use smol::io::{AsyncReadExt, AsyncWriteExt};

use super::RequestDraft;

#[gpui_kit::test]
async fn saved_request_opens_with_all_fields_sends_and_keeps_its_tab_state(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/items?from=url", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        assert!(
            head.starts_with("POST /items?from=url&tag=edited&tag=two HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(head.contains("x-saved: updated\r\n"), "{head}");
        assert!(head.contains("x-saved: second\r\n"), "{head}");
        let mut body = [0; 14];
        stream.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"{\"hello\":true}");
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").await.unwrap();
    });

    let directory = std::env::temp_dir().join(format!(
        "request-eagle-open-saved-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let file = directory.join("Saved API/Items/create.toml");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let original = format!(
        "id = \"create-item\"\nname = \"Create item\"\nschema_version = 1\n[request]\ntype = \"http\"\nmethod = \"POST\"\npath = \"{url}\"\nheaders = [[\"X-Saved\", \"first\"], [\"X-Saved\", \"second\"]]\nquery = [[\"tag\", \"one\"], [\"tag\", \"two\"]]\nbody = {:?}\n",
        b"{\"hello\":true}".as_slice(),
    );
    fs::write(&file, &original).unwrap();
    let collections = CollectionRegistry::from_path(&directory).unwrap();

    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
    });
    let (layout, cx) = cx.add_window_view(|window, cx| {
        crate::workspace::Layout::new(collections, updater::init("1.2.3", cx), window, cx)
    });
    let tabs = cx.read(|cx| layout.read(cx).main_view.clone());
    let row = cx.debug_bounds("collection-row-2").unwrap();
    cx.simulate_click(row.center(), Modifiers::default());
    let draft = cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.selected, Some(1));
        assert_eq!(tabs.tabs[1].title, "Create item");
        assert_eq!(tabs.tabs[1].method, Some("POST"));
        assert_eq!(tabs.tabs[1].request_path.as_ref(), Some(&file));
        let draft = tabs.tabs[1]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();
        let data = draft.read(cx);
        assert_eq!(data.name, "Create item");
        assert_eq!(data.collection.as_deref(), Some("Saved API"));
        assert_eq!(data.request.path, url);
        assert_eq!(data.url.as_ref().unwrap().read(cx).value(), url);
        assert_eq!(data.request.headers.len(), 2);
        assert_eq!(data.request.query.as_ref().unwrap().len(), 2);
        assert_eq!(
            data.request.body.as_deref(),
            Some(b"{\"hello\":true}".as_slice())
        );
        draft
    });

    // Editing one preloaded row must keep the other saved duplicate intact.
    cx.update(|window, _| window.refresh());
    assert!(
        cx.debug_bounds("headers-key-2").is_some(),
        "keep a trailing blank row"
    );
    let header = cx.debug_bounds("headers-value-0").unwrap();
    cx.simulate_click(header.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("updated");
    let params = cx.debug_bounds("request-section-Params").unwrap();
    cx.simulate_click(params.center(), Modifiers::default());
    let param = cx.debug_bounds("params-value-0").unwrap();
    cx.simulate_click(param.center(), Modifiers::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("edited");
    let body = cx.debug_bounds("request-section-Body").unwrap();
    cx.simulate_click(body.center(), Modifiers::default());
    cx.read(|cx| {
        let data = draft.read(cx);
        assert_eq!(
            data.body.as_ref().unwrap().read(cx).value(),
            "{\"hello\":true}"
        );
        assert_eq!(
            data.request.headers,
            [
                ("X-Saved".into(), "updated".into()),
                ("X-Saved".into(), "second".into())
            ]
        );
        assert_eq!(
            data.request.query.as_ref().unwrap(),
            &[
                ("tag".into(), "edited".into()),
                ("tag".into(), "two".into())
            ]
        );
    });

    cx.simulate_keystrokes("secondary-enter");
    let started = Instant::now();
    while cx.read(|cx| draft.read(cx).task.is_some()) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "saved request did not finish"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    server.await;
    assert!(cx.debug_bounds("response-status").is_some());

    cx.update(|_, cx| draft.update(cx, |draft, cx| draft.set_method(Method::Put, cx)));
    cx.simulate_keystrokes("secondary-t");
    cx.simulate_click(row.center(), Modifiers::default());
    cx.simulate_click(row.center(), Modifiers::default());
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        assert_eq!(tabs.selected, Some(1));
        assert_eq!(tabs.tabs[1].page.entity_id(), draft.entity_id());
        assert_eq!(tabs.tabs[1].method, Some("PUT"));
        assert_eq!(draft.read(cx).request.headers[0].1, "updated");
    });
    assert!(
        cx.debug_bounds("response-status").is_some(),
        "keep the completed response"
    );

    // Closing and reopening loads a fresh saved snapshot, without persisting tab edits.
    cx.simulate_keystrokes("secondary-w");
    cx.simulate_click(row.center(), Modifiers::default());
    cx.read(|cx| {
        let tabs = tabs.read(cx);
        assert_eq!(tabs.tabs.len(), 3);
        let reopened = tabs.tabs[2]
            .page
            .clone()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();
        assert_ne!(reopened, draft);
        assert_eq!(reopened.read(cx).request.method, Method::Post);
        assert_eq!(reopened.read(cx).request.headers[0].1, "first");
        assert_eq!(
            reopened.read(cx).request.query.as_ref().unwrap()[0].1,
            "one"
        );
    });
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
    fs::remove_dir_all(directory).unwrap();
}
