use gpui_kit::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext};
use request::HttpRequest;

use super::RequestDraft;

fn open(
    request: HttpRequest,
    cx: &mut TestAppContext,
) -> (Entity<RequestDraft>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut draft = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut draft = RequestDraft::new(request, None, Default::default(), None, cx);
            draft.prepare(window, cx);
            draft
        });
        draft = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let draft = draft.unwrap();
    click(cx, "request-section-Params");

    (draft, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
}

/// Replace the text of the field at `selector`, as typing over a selection.
fn retype(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(text);
}

fn path(draft: &Entity<RequestDraft>, cx: &mut VisualTestContext) -> String {
    cx.read(|cx| draft.read(cx).request.path.clone())
}

fn params(draft: &Entity<RequestDraft>, cx: &mut VisualTestContext) -> Vec<(bool, String, String)> {
    cx.read(|cx| {
        let params = draft.read(cx).params.clone().unwrap();
        params.read(cx).rows_for_test(cx)
    })
}

fn row(enabled: bool, key: &str, value: &str) -> (bool, String, String) {
    (enabled, key.into(), value.into())
}

fn shows(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.update(|window, _| window.refresh());
    cx.debug_bounds(selector).is_some()
}

#[gpui_kit::test]
fn query_params_and_the_url_follow_each_other(cx: &mut TestAppContext) {
    let (draft, cx) = open(
        HttpRequest {
            path: "https://example.com/pets?limit=10".into(),
            ..Default::default()
        },
        cx,
    );
    assert_eq!(
        params(&draft, cx),
        [row(true, "limit", "10"), row(true, "", "")]
    );

    retype(
        cx,
        "request-url",
        "https://example.com/pets?limit=10&sort=name",
    );
    assert_eq!(
        params(&draft, cx),
        [
            row(true, "limit", "10"),
            row(true, "sort", "name"),
            row(true, "", "")
        ]
    );
    assert!(shows(cx, "request-section-Params-count-2"));

    retype(cx, "params-value-0", "20");
    assert_eq!(
        path(&draft, cx),
        "https://example.com/pets?limit=20&sort=name"
    );
    cx.read(|cx| {
        let url = draft.read(cx).url_input().unwrap().read(cx).value();
        assert_eq!(url, "https://example.com/pets?limit=20&sort=name");
    });

    // A disabled row leaves the URL but stays in the table.
    click(cx, "params-enabled-0");
    assert_eq!(path(&draft, cx), "https://example.com/pets?sort=name");

    retype(
        cx,
        "request-url",
        "https://example.com/pets?sort=name&page=2",
    );
    assert_eq!(
        params(&draft, cx),
        [
            row(false, "limit", "20"),
            row(true, "sort", "name"),
            row(true, "page", "2"),
            row(true, "", "")
        ]
    );

    click(cx, "params-remove-1");
    assert_eq!(path(&draft, cx), "https://example.com/pets?page=2");

    click(cx, "params-enabled-0");
    assert_eq!(path(&draft, cx), "https://example.com/pets?limit=20&page=2");

    // A new key is written without `=` until it has a value. Characters that
    // would end it are encoded.
    retype(cx, "params-key-2", "q");
    assert_eq!(
        path(&draft, cx),
        "https://example.com/pets?limit=20&page=2&q"
    );
    retype(cx, "params-value-2", "a&b c");
    assert_eq!(
        path(&draft, cx),
        "https://example.com/pets?limit=20&page=2&q=a%26b c"
    );

    click(cx, "params-enabled-1");
    retype(cx, "request-url", "https://example.com/pets");
    assert_eq!(
        params(&draft, cx),
        [row(false, "page", "2"), row(true, "", "")]
    );
}

#[gpui_kit::test]
fn path_variables_follow_the_url(cx: &mut TestAppContext) {
    let (draft, cx) = open(
        HttpRequest {
            path: "https://example.com/users/:id/posts/:post".into(),
            path_variables: vec![("id".into(), "7".into())],
            ..Default::default()
        },
        cx,
    );
    let rows = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let table = draft.read(cx).path_variables.clone().unwrap();
            table.read(cx).rows_for_test(cx)
        })
    };
    let chips = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let url = draft.read(cx).url_completion.clone().unwrap();
            url.update(cx, |url, cx| url.path_chips(cx))
        })
    };

    assert!(shows(cx, "path-variables-table"));
    assert!(shows(cx, "request-section-Params-count-2"));
    assert_eq!(
        rows(cx),
        [("id".into(), "7".into()), ("post".into(), String::new())]
    );
    assert_eq!(chips(cx), [(26..29, true), (36..41, false)]);

    retype(cx, "path-variables-value-1", "{{post_id}}");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.path_variables,
            [
                ("id".to_owned(), "7".to_owned()),
                ("post".to_owned(), "{{post_id}}".to_owned())
            ]
        );
        assert!(draft.read(cx).is_dirty());
    });
    assert_eq!(chips(cx), [(26..29, true), (36..41, true)]);

    // Editing the URL elsewhere keeps the values; removing a variable drops its value.
    retype(
        cx,
        "request-url",
        "https://api.example.com/users/:id?expand=1",
    );
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.path_variables,
            [("id".to_owned(), "7".to_owned())]
        )
    });
    assert_eq!(rows(cx), [("id".into(), "7".into())]);
    assert!(shows(cx, "request-section-Params-count-2"));

    retype(cx, "request-url", "https://api.example.com/users/:user");
    assert_eq!(rows(cx), [("user".into(), String::new())]);
    assert_eq!(chips(cx), [(30..35, false)]);

    retype(cx, "request-url", "https://api.example.com/users");
    assert!(!shows(cx, "path-variables-table"));
    cx.read(|cx| assert!(draft.read(cx).request.path_variables.is_empty()));

    // Values return with their variables while the tab is open.
    retype(
        cx,
        "request-url",
        "https://api.example.com/users/:id/posts/:post",
    );
    assert_eq!(
        rows(cx),
        [
            ("id".into(), "7".into()),
            ("post".into(), "{{post_id}}".into())
        ]
    );
    cx.read(|cx| assert_eq!(draft.read(cx).request.path_variables.len(), 2));
}

#[gpui_kit::test]
fn saved_query_params_move_into_the_url_without_changing_the_draft(cx: &mut TestAppContext) {
    let (draft, cx) = open(
        HttpRequest {
            path: "https://example.com/?a=1#top".into(),
            query: vec![("q".into(), "a b".into())],
            ..Default::default()
        },
        cx,
    );

    cx.read(|cx| {
        let draft = draft.read(cx);
        assert_eq!(draft.request.path, "https://example.com/?a=1&q=a+b#top");
        assert!(draft.request.query.is_empty());
        assert!(!draft.is_dirty());
    });
    assert_eq!(
        params(&draft, cx),
        [
            row(true, "a", "1"),
            row(true, "q", "a+b"),
            row(true, "", "")
        ]
    );
}

#[gpui_kit::test]
fn params_without_a_key_stay_in_the_url(cx: &mut TestAppContext) {
    let (draft, cx) = open(
        HttpRequest {
            path: "https://example.com/?=keep&x=1".into(),
            ..Default::default()
        },
        cx,
    );
    assert_eq!(
        params(&draft, cx),
        [
            row(true, "", "keep"),
            row(true, "x", "1"),
            row(true, "", "")
        ]
    );

    retype(cx, "params-value-1", "2");
    assert_eq!(path(&draft, cx), "https://example.com/?=keep&x=2");

    // A header without a name is not sent.
    click(cx, "request-section-Headers");
    retype(cx, "headers-value-0", "orphan");
    cx.read(|cx| assert!(draft.read(cx).request.headers.is_empty()));
}
