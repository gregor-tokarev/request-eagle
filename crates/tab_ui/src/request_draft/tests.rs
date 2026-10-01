use request::{HttpRequest, Method};

#[test]
fn prepares_json_requests_without_mutating_the_draft() {
    let mut original = HttpRequest {
        method: Method::Post,
        path: "  ifconfig.me/ip  ".into(),
        body: Some(b"{\"hello\":true}".to_vec()),
        ..HttpRequest::default()
    };
    let templated = HttpRequest {
        path: "{{baseUrl}}/echo".into(),
        ..Default::default()
    };
    assert_eq!(templated.prepare_for_send().path, "{{baseUrl}}/echo");
    let outgoing = original.clone().prepare_for_send();
    assert_eq!(outgoing.path, "https://ifconfig.me/ip");
    assert_eq!(
        outgoing.headers,
        [("Content-Type".into(), "application/json".into())]
    );
    assert!(original.headers.is_empty());
    assert_eq!(original.path, "  ifconfig.me/ip  ");

    original
        .headers
        .push(("content-type".into(), "application/custom+json".into()));
    assert_eq!(
        original.clone().prepare_for_send().headers,
        original.headers
    );

    for method in [Method::Get, Method::Head] {
        original.method = method;
        assert!(original.clone().prepare_for_send().body.is_none());
        assert!(original.body.is_some());
    }
}
