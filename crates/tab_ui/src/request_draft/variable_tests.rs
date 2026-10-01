use request::Method;
use std::collections::HashMap;

#[test]
fn environment_errors_are_reported_only_for_environment_references() {
    use request::RequestVariables;
    let values = HashMap::from([("base_url".into(), "https://cached.example".into())]);
    let mut request = request::HttpRequest {
        path: "{{ base_url }}".into(),
        ..Default::default()
    };
    assert_eq!(
        RequestVariables::new(values.clone(), Some("Invalid environment file".into()))
            .resolve(&request)
            .unwrap_err(),
        "Invalid environment file"
    );
    request.path = "http://example.com/{{$unsupported}}".into();
    assert!(
        RequestVariables::new(values, Some("Invalid environment file".into()))
            .resolve(&request)
            .unwrap_err()
            .contains("Unknown variable")
    );
}

#[test]
fn environment_errors_are_reported_in_every_request_field() {
    use request::HttpRequest;
    use request::RequestVariables;

    let values = HashMap::from([("message".into(), "cached value".into())]);
    for field in 0..6 {
        let mut request = HttpRequest {
            method: Method::Post,
            path: "http://example.com".into(),
            ..Default::default()
        };
        let token = "{{ message }}".to_owned();
        match field {
            0 => request.path.push_str(&format!("/{token}")),
            1 => request.headers.push((token, "value".into())),
            2 => request.headers.push(("X-Message".into(), token)),
            3 => request.query = vec![(token, "value".into())],
            4 => request.query = vec![("message".into(), token)],
            _ => request.body = Some(token.into_bytes()),
        }
        assert!(
            RequestVariables::new(values.clone(), None)
                .resolve(&request)
                .is_ok()
        );
        assert_eq!(
            RequestVariables::new(values.clone(), Some("Invalid environment file".into()))
                .resolve(&request)
                .unwrap_err(),
            "Invalid environment file"
        );
    }

    let request = HttpRequest {
        path: "http://example.com".into(),
        body: Some(b"{{ message }}".to_vec()),
        ..Default::default()
    };
    assert!(
        RequestVariables::new(values, Some("Invalid environment file".into()))
            .resolve(&request.prepare_for_send())
            .is_ok(),
        "GET excludes the body before resolution"
    );
}
