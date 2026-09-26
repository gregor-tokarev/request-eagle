use gpui_kit::{AppContext as _, Modifiers, MouseButton, TestAppContext, point, px};
use request::Method;

use super::{RequestDraft, draft::RequestSection};

#[test]
fn templated_header_names_defer_potentially_overridden_defaults() {
    let request = request::HttpRequest {
        path: "http://example.com".into(),
        method: Method::Post,
        headers: vec![("{{header_name}}".into(), "virtual.example".into())],
        body: Some(b"{}".to_vec()),
        ..Default::default()
    };
    let preview = super::execution::generated_headers(&request);
    assert!(preview.iter().all(|(_, value)| value == "Resolved on Send"));

    for name in [
        "Host",
        "Accept",
        "Accept-Encoding",
        "Content-Length",
        "Content-Type",
    ] {
        let values = environment::VariableValues {
            environment: [("header_name".into(), name.into())].into(),
            ..Default::default()
        };
        let resolved = request.resolve_variables(&values).unwrap();
        assert!(
            super::execution::generated_headers(&resolved)
                .iter()
                .all(|(generated, _)| generated != name)
        );
    }
}

#[test]
fn templated_url_credentials_preview_authorization_as_unresolved() {
    for (path, expected) in [
        ("{{base_url}}/users", "Resolved on Send"),
        ("https://{{authority}}/users", "Resolved on Send"),
        (
            "https://{{user}}:{{vault:password}}@example.com",
            "Resolved on Send",
        ),
        (
            "https://user:{{vault:password}}@example.com",
            "Resolved on Send",
        ),
        (
            "https://{{user}}:pass@{{host}}:{{port}}",
            "Resolved on Send",
        ),
        (
            "https://user:pass@example.com/{{path}}",
            "Basic dXNlcjpwYXNz",
        ),
    ] {
        let mut request = request::HttpRequest {
            path: path.into(),
            ..Default::default()
        };
        let headers = super::execution::generated_headers(&request);
        assert_eq!(
            headers
                .iter()
                .find(|(name, _)| name == "Authorization")
                .map(|(_, value)| value.as_str()),
            Some(expected),
            "{path}"
        );

        request
            .headers
            .push(("AUTHORIZATION".into(), "Bearer explicit".into()));
        assert!(
            super::execution::generated_headers(&request)
                .iter()
                .all(|(name, _)| name != "Authorization")
        );
    }
}

#[test]
fn templated_urls_preview_generated_host_without_hiding_known_hosts() {
    for (path, expected) in [
        ("{{base_url}}/users", "Resolved on Send"),
        ("https://{{host}}/users", "Resolved on Send"),
        ("https://example.com:{{port}}/users", "Resolved on Send"),
        ("https://example.com/{{path}}", "example.com"),
        ("example.com:8443/?q={{query}}", "example.com:8443"),
    ] {
        let mut request = request::HttpRequest {
            path: path.into(),
            ..Default::default()
        };
        let headers = super::execution::generated_headers(&request);
        assert_eq!(headers[0], ("Host".into(), expected.into()), "{path}");

        request
            .headers
            .push(("hOsT".into(), "override.example".into()));
        assert!(
            super::execution::generated_headers(&request)
                .iter()
                .all(|(name, _)| name != "Host")
        );
    }
}

#[gpui_kit::test]
fn generated_headers_update_count_respect_overrides_and_are_selectable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
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
    assert!(cx.debug_bounds("request-section-Headers-count-2").is_some());
    assert!(cx.debug_bounds("headers-generated-value-0").is_some());
    let url = cx.debug_bounds("request-url").unwrap();
    cx.simulate_click(url.center(), Modifiers::default());
    cx.simulate_input("example.com:8443/path");
    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-3").is_some());
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
        assert_eq!(headers[3], ("Content-Length".into(), "15".into()));
        assert_eq!(
            headers[4],
            ("Content-Type".into(), "application/json".into())
        );
    });
    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-5").is_some());
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
        assert_eq!(draft.read(cx).generated_headers.len(), 3);
        assert!(
            draft.read(cx).request.body.is_some(),
            "preserve the draft body while GET excludes it"
        );
    });
}
