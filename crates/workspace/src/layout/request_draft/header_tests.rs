use gpui_kit::{AppContext as _, Modifiers, MouseButton, TestAppContext, point, px};
use request::Method;

use super::{RequestDraft, draft::RequestSection};

#[gpui_kit::test]
fn generated_headers_update_count_respect_overrides_and_are_selectable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut draft = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        // The initial untitled tab can render before prepare/focus is called.
        let view = cx.new(|_| RequestDraft::new());
        draft = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let draft = draft.unwrap();
    let refresh = |cx: &mut gpui_kit::VisualTestContext| cx.update(|window, _| window.refresh());

    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-1").is_some());
    assert!(cx.debug_bounds("headers-generated-value-0").is_some());
    let url = cx.debug_bounds("request-url").unwrap();
    cx.simulate_click(url.center(), Modifiers::default());
    cx.simulate_input("example.com:8443/path");
    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-2").is_some());
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).generated_headers[0],
            ("Host".into(), "example.com:8443".into())
        );
        assert!(draft.read(cx).request.headers.is_empty());
    });

    let key = cx.debug_bounds("headers-key-0").unwrap();
    cx.simulate_click(key.center(), Modifiers::default());
    cx.simulate_input("HOST");
    refresh(cx);
    let value = cx.debug_bounds("headers-value-0").unwrap();
    cx.simulate_click(value.center(), Modifiers::default());
    cx.simulate_input("virtual.example");
    cx.read(|cx| {
        assert!(
            draft
                .read(cx)
                .generated_headers
                .iter()
                .all(|(name, _)| name != "Host")
        )
    });

    refresh(cx);
    let enabled = cx.debug_bounds("headers-enabled-0").unwrap();
    cx.simulate_click(enabled.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(draft.read(cx).generated_headers[0].1, "example.com:8443");
        assert!(draft.read(cx).request.headers.is_empty());
    });

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Post, cx);
            draft.section = RequestSection::Body;
            draft.prepare(window, cx);
            draft.body.as_ref().unwrap().update(cx, |body, cx| {
                body.replace_all("{\"bird\":\"🦅\"}", window, cx)
            });
        })
    });
    cx.read(|cx| {
        let headers = &draft.read(cx).generated_headers;
        assert_eq!(headers[2], ("Content-Length".into(), "15".into()));
        assert_eq!(
            headers[3],
            ("Content-Type".into(), "application/json".into())
        );
    });
    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-4").is_some());
    let tab = cx.debug_bounds("request-section-Headers").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    refresh(cx);
    let cell = cx.debug_bounds("headers-generated-value-0").unwrap();
    let start = point(cell.left() + px(8.), cell.center().y);
    let end = point(cell.right() - px(8.), cell.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.simulate_keystrokes("secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().unwrap(),
        "example.com:8443"
    );

    cx.update(|_, cx| draft.update(cx, |draft, cx| draft.set_method(Method::Get, cx)));
    cx.read(|cx| {
        assert_eq!(draft.read(cx).generated_headers.len(), 2);
        assert!(
            draft.read(cx).request.body.is_some(),
            "preserve the draft body while GET excludes it"
        );
    });
}
