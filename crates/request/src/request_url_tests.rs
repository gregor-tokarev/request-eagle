use std::collections::HashMap;

use crate::request_url::{
    append_encoded_query, endpoint_label, endpoint_tooltip, fill_path_variables, path_variables,
    query_params, with_query_params,
};
use crate::{Field, GrpcRequest, HttpRequest, Request, RequestVariables, WebSocketRequest};

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn query_params_are_read_as_the_server_decodes_them() {
    assert_eq!(
        query_params("https://example.com/search?q=owner%20name+x&flag&&page={{page}}#top"),
        pairs(&[("q", "owner name x"), ("flag", ""), ("page", "{{page}}")])
    );
    assert_eq!(query_params("https://example.com/a=b"), []);
    assert_eq!(query_params("https://example.com/#?q=1"), []);
    assert_eq!(query_params("{{base}}?q=a=b"), pairs(&[("q", "a=b")]));
}

#[test]
fn query_params_replace_the_url_query_and_keep_its_fragment() {
    let url = "https://example.com/search?old=1#top";

    assert_eq!(
        with_query_params(url, &pairs(&[("q", "a b"), ("flag", ""), ("x", "{{x}}")])),
        "https://example.com/search?q=a b&flag&x={{x}}#top"
    );
    assert_eq!(
        with_query_params(url, &[]),
        "https://example.com/search#top"
    );
    assert_eq!(
        with_query_params("example.com/?", &pairs(&[("page", "2")])),
        "example.com/?page=2"
    );
    assert_eq!(
        with_query_params("example.com", &pairs(&[("page", "2")])),
        "example.com?page=2"
    );
}

#[test]
fn query_params_encode_what_would_change_their_meaning() {
    let params = pairs(&[("a=b&c#d", "C++ e=f&g#h%20{{x+y}}")]);
    let url = with_query_params("https://example.com/", &params);

    assert_eq!(
        url,
        "https://example.com/?a%3Db%26c%23d=C%2B%2B e%3Df%26g%23h%2520{{x+y}}"
    );
    assert_eq!(query_params(&url), params);
}

#[test]
fn path_variables_are_whole_path_segments_after_the_host() {
    let names = |url| {
        path_variables(url)
            .map(|(range, name)| (&url[range], name))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        names("https://user:secret@example.com:8443/users/:id/posts/:post_id?sort=:asc#:top"),
        [(":id", "id"), (":post_id", "post_id")]
    );
    assert_eq!(names("{{base_url}}/pets/:id"), [(":id", "id")]);
    assert_eq!(
        names("localhost:3000/:id.json/a:b/:/"),
        [(":id.json", "id.json")]
    );
    assert_eq!(
        names("/:first/:first"),
        [(":first", "first"), (":first", "first")]
    );
    assert_eq!(names("example.com:8080"), []);
    assert_eq!(names("https://example.com/users/:{{name}}"), []);
}

#[test]
fn path_variables_are_filled_when_they_have_a_value() {
    let values = pairs(&[("id", "a/b?c#d{e}"), ("empty", ""), ("unused", "x")]);
    let filled = fill_path_variables(
        "https://example.com:8080/:id/:empty/:id?id=:id",
        &values,
        |value| Ok::<_, ()>(value.to_owned()),
    );

    assert_eq!(
        filled.unwrap(),
        "https://example.com:8080/a/b%3Fc%23d%7Be%7D/:empty/a/b%3Fc%23d%7Be%7D?id=:id"
    );
}

fn resolve(request: HttpRequest, values: &[(&str, &str)]) -> String {
    let values = values
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>();

    RequestVariables::new(values, None)
        .resolve(&request)
        .unwrap()
        .path
}

#[test]
fn sending_resolves_variables_in_path_variable_values() {
    let request = HttpRequest {
        path: "{{base}}/users/:id/:missing".into(),
        path_variables: pairs(&[("id", "{{user}}")]),
        ..Default::default()
    };
    let values = [("base", "https://example.com"), ("user", "42")];
    assert_eq!(
        resolve(request.clone(), &values),
        "https://example.com/users/42/:missing"
    );

    // A resolved value stays in its path segment.
    let request = HttpRequest {
        path: "https://example.com/files/:id/end?x=1".into(),
        path_variables: pairs(&[("id", "{{file}}")]),
        ..Default::default()
    };
    assert_eq!(
        resolve(request, &[("file", "a#b?c{{d}}")]),
        "https://example.com/files/a%23b%3Fc%7B%7Bd%7D%7D/end?x=1"
    );

    // A value after a fragment is not sent, so it is not resolved.
    let request = HttpRequest {
        path: "{{endpoint}}/files/:id?q=1".into(),
        path_variables: pairs(&[("id", "{{missing}}")]),
        ..Default::default()
    };
    assert_eq!(
        resolve(request, &[("endpoint", "https://example.com/path#top")]),
        "https://example.com/path?q=1"
    );
}

#[test]
fn variables_in_the_query_stay_within_their_key_or_value() {
    let request = HttpRequest {
        path: "https://example.com/{{segment}}?q={{term}}&{{key}}=a b+{{!literal}}#{{ignored}}"
            .into(),
        ..Default::default()
    };

    assert_eq!(
        resolve(
            request,
            &[("segment", "a/b"), ("term", "x&y=z #1+2%"), ("key", "k=v")]
        ),
        "https://example.com/a/b?q=x%26y%3Dz+%231%2B2%25&k%3Dv=a b+{{literal}}"
    );
}

#[test]
fn a_whole_url_variable_keeps_the_query_written_after_it() {
    let request = HttpRequest {
        path: "{{endpoint}}/{{missing}}?q=2".into(),
        ..Default::default()
    };

    assert_eq!(
        resolve(
            request.clone(),
            &[("endpoint", "https://example.com/path?old=1#top")]
        ),
        "https://example.com/path?old=1&q=2"
    );

    let request = HttpRequest {
        path: "{{endpoint}}?q=2".into(),
        ..request
    };
    assert_eq!(
        resolve(request, &[("endpoint", "https://example.com/path?old=1")]),
        "https://example.com/path?old=1&q=2"
    );
}

#[test]
fn appended_query_params_are_encoded_around_variables() {
    let params = pairs(&[("q", "a & b+{{term}}"), ("{{key}}", "")]);

    assert_eq!(
        append_encoded_query("https://example.com/?page=1#top", &params),
        "https://example.com/?page=1&q=a+%26+b%2B{{term}}&{{key}}=#top"
    );
    assert_eq!(
        append_encoded_query("https://example.com/?", &params[..1]),
        "https://example.com/?q=a+%26+b%2B{{term}}"
    );
    assert_eq!(
        append_encoded_query(" https://example.com/pets ", &params[..1]),
        "https://example.com/pets?q=a+%26+b%2B{{term}}"
    );
}

#[test]
fn query_params_moved_into_the_url_resolve_as_they_were_sent() {
    let mut request = HttpRequest {
        path: "https://example.com/pets ".into(),
        query: pairs(&[("q", "{{term}} &"), ("{{key}}", "1")])
            .into_iter()
            .map(Field::from)
            .collect(),
        ..Default::default()
    };
    let values = [("term", "a&b+c d%"), ("key", "k=v")];
    let mut sent = request.clone().prepare_for_send();
    sent.path = resolve(sent.clone(), &values);
    let mut separate = url::Url::parse(&sent.path).unwrap();
    let pairs: Vec<_> = sent
        .query
        .iter()
        .map(|Field { key, value, .. }| (resolve_text(key, &values), resolve_text(value, &values)))
        .collect();
    separate.query_pairs_mut().extend_pairs(&pairs);

    request.inline_query();
    let inline = resolve(request.prepare_for_send(), &values);

    assert_eq!(inline, separate.as_str());
    assert_eq!(
        inline,
        "https://example.com/pets?q=a%26b%2Bc+d%25+%26&k%3Dv=1"
    );
}

fn resolve_text(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(&format!("{{{{{name}}}}}"), value)
    })
}

#[test]
fn endpoint_labels_strip_schemes_queries_fragments_and_trailing_slashes() {
    assert_eq!(
        endpoint_label("https://example.com").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        endpoint_label("https://example.com/").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        endpoint_label("HTTPS://httpbin.org/post/").as_deref(),
        Some("httpbin.org/post")
    );
    assert_eq!(
        endpoint_label("http://localhost:3000/api/v1/users?page=2#top").as_deref(),
        Some("localhost:3000/api/v1/users")
    );
    assert_eq!(
        endpoint_label("wss://echo.websocket.org/chat?room=general").as_deref(),
        Some("echo.websocket.org/chat")
    );
    assert_eq!(
        endpoint_label("example.com?next=https://a.test").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        endpoint_label("  https://example.com/users/:id/  ").as_deref(),
        Some("example.com/users/:id")
    );
    assert_eq!(
        endpoint_label("http://[::1]:8080/health/").as_deref(),
        Some("[::1]:8080/health")
    );
    assert_eq!(
        endpoint_label("localhost:3000/api/").as_deref(),
        Some("localhost:3000/api")
    );
    assert_eq!(
        endpoint_label("http:8080/api/").as_deref(),
        Some("http:8080/api")
    );
    assert_eq!(
        endpoint_tooltip("http:8080/api?page=1").as_deref(),
        Some("http:8080/api?page=1")
    );
    assert_eq!(
        endpoint_label("ws:9000/chat/").as_deref(),
        Some("ws:9000/chat")
    );
    assert_eq!(
        endpoint_tooltip("ws:9000/chat?room=general").as_deref(),
        Some("ws:9000/chat?room=general")
    );
    assert_eq!(endpoint_label("http:99999/api"), None);
    assert_eq!(endpoint_tooltip("http:99999/api"), None);
    assert_eq!(endpoint_label("http:8080:9090/api"), None);
    assert_eq!(endpoint_tooltip("http:8080:9090/api"), None);
    assert_eq!(
        endpoint_label("[::1]:8080/health/").as_deref(),
        Some("[::1]:8080/health")
    );
    assert_eq!(
        endpoint_label("localhost:/api").as_deref(),
        Some("localhost/api")
    );
    assert_eq!(
        endpoint_tooltip("localhost:/api").as_deref(),
        Some("localhost:/api")
    );
    assert_eq!(endpoint_label("localhost:").as_deref(), Some("localhost"));
    assert_eq!(
        endpoint_tooltip("localhost:").as_deref(),
        Some("localhost:")
    );
    assert_eq!(
        endpoint_label("[::1]:/health/").as_deref(),
        Some("[::1]/health")
    );
    assert_eq!(
        endpoint_tooltip("[::1]:/health/").as_deref(),
        Some("[::1]:/health/")
    );
    assert_eq!(endpoint_label("http:/api").as_deref(), Some("http/api"));
    assert_eq!(endpoint_tooltip("http:/api").as_deref(), Some("http:/api"));
    assert_eq!(
        endpoint_label("http:/api?x={{y}}").as_deref(),
        Some("http/api")
    );
    assert_eq!(
        endpoint_tooltip("http:/api?x={{y}}").as_deref(),
        Some("http:/api?x={{y}}")
    );
    assert_eq!(endpoint_label("http:").as_deref(), Some("http"));
    assert_eq!(endpoint_tooltip("http:").as_deref(), Some("http:"));
    assert_eq!(endpoint_label("/health/").as_deref(), Some("/health"));
}

#[test]
fn endpoint_labels_keep_template_variables_and_reject_empty_targets() {
    assert_eq!(
        endpoint_label("{{baseUrl}}").as_deref(),
        Some("{{baseUrl}}")
    );
    assert_eq!(
        endpoint_label("{{baseUrl}}/users/?page={{page}}").as_deref(),
        Some("{{baseUrl}}/users")
    );
    assert_eq!(
        endpoint_label("{{base}}/:id").as_deref(),
        Some("{{base}}/:id")
    );
    assert_eq!(
        endpoint_label("{{baseUrl}}/@alice/posts").as_deref(),
        Some("{{baseUrl}}/@alice/posts")
    );
    assert_eq!(
        endpoint_label("https://{{host?name}}/api").as_deref(),
        Some("{{host?name}}/api")
    );
    assert_eq!(
        endpoint_tooltip("https://{{host?name}}/api?q={{unset}}").as_deref(),
        Some("https://{{host?name}}/api?q={{unset}}")
    );
    assert_eq!(
        endpoint_label("{{host?name}}/{{section}}#{{unset}}").as_deref(),
        Some("{{host?name}}/{{section}}")
    );
    assert_eq!(
        endpoint_tooltip("{{host?name}}/{{section}}#{{unset}}").as_deref(),
        Some("{{host?name}}/{{section}}#{{unset}}")
    );
    assert_eq!(
        endpoint_label("https://{{host/name}}/api").as_deref(),
        Some("{{host/name}}/api")
    );
    assert_eq!(
        endpoint_label("https://{{host@name}}/api").as_deref(),
        Some("{{host@name}}/api")
    );
    assert_eq!(
        endpoint_label("https://{{host#name}}/api").as_deref(),
        Some("{{host#name}}/api")
    );
    assert_eq!(
        endpoint_label("{{scheme}}://api.test/users/?q=1").as_deref(),
        Some("{{scheme}}://api.test/users")
    );
    assert_eq!(
        endpoint_label("{{ scheme }}://api.test/users").as_deref(),
        Some("{{ scheme }}://api.test/users")
    );
    assert_eq!(
        endpoint_label("http{{s}}://api.test/users/").as_deref(),
        Some("http{{s}}://api.test/users")
    );
    assert_eq!(
        endpoint_tooltip("http{{s}}://api.test/users/").as_deref(),
        Some("http{{s}}://api.test/users/")
    );
    assert_eq!(
        endpoint_label("https://{{host}}:{{port}}/v1/").as_deref(),
        Some("{{host}}:{{port}}/v1")
    );
    assert_eq!(
        endpoint_label("https://{{host}}:/api").as_deref(),
        Some("{{host}}/api")
    );
    assert_eq!(
        endpoint_tooltip("https://{{host}}:/api").as_deref(),
        Some("https://{{host}}:/api")
    );
    assert_eq!(
        endpoint_label("localhost:/api?x={{y}}").as_deref(),
        Some("localhost/api")
    );
    assert_eq!(
        endpoint_tooltip("localhost:/api?x={{y}}").as_deref(),
        Some("localhost:/api?x={{y}}")
    );
    assert_eq!(
        endpoint_label("https://{{ host }}/api").as_deref(),
        Some("{{ host }}/api")
    );
    assert_eq!(
        endpoint_tooltip("https://{{ host }}/api?x=1").as_deref(),
        Some("https://{{ host }}/api?x=1")
    );
    assert_eq!(
        endpoint_label("https://example.com:{{ port }}/api").as_deref(),
        Some("example.com:{{ port }}/api")
    );
    assert_eq!(
        endpoint_tooltip("https://example.com:{{ port }}/api").as_deref(),
        Some("https://example.com:{{ port }}/api")
    );
    assert_eq!(
        endpoint_label("http:{{ port }}/api").as_deref(),
        Some("http:{{ port }}/api")
    );
    assert_eq!(endpoint_label("https://{{   }}/api"), None);
    assert_eq!(endpoint_tooltip("https://{{   }}/api"), None);
    assert_eq!(
        endpoint_label("https://{{host}}\\").as_deref(),
        Some("{{host}}")
    );

    for empty in [
        "",
        "   ",
        "https://",
        "https:///",
        "https://   /api",
        "https://?a=1",
        "{{scheme}}://",
        "//",
        "/",
        "///",
        "?a=1",
        "#top",
    ] {
        assert_eq!(endpoint_label(empty), None, "expected None for {empty:?}");
        assert_eq!(endpoint_tooltip(empty), None, "expected None for {empty:?}");
    }
}

#[test]
fn endpoint_formatting_strips_authority_credentials_and_preserves_path_and_query_at_signs() {
    assert_eq!(
        endpoint_label("https://user:secret@example.com/api?page=1").as_deref(),
        Some("example.com/api")
    );
    assert_eq!(
        endpoint_tooltip("https://user:secret@example.com/api?page=1").as_deref(),
        Some("https://example.com/api?page=1")
    );
    assert_eq!(
        endpoint_label("https://user:p@ss@example.com/api").as_deref(),
        Some("example.com/api")
    );
    assert_eq!(
        endpoint_tooltip("https://user:p@ss@example.com/api").as_deref(),
        Some("https://example.com/api")
    );
    // Special-scheme forms accepted by WHATWG `Url::parse` (0, 1, 3+ slashes,
    // backslashes, control characters) must never expose credentials.
    for variant in [
        "https:/alice:secret@example.com/api",
        "https:alice:secret@example.com/api",
        "https:///alice:secret@example.com/api",
        "https:\\alice:secret@example.com/api",
        "https:\\\\alice:secret@example.com/api",
        "https:/\\alice:secret@example.com/api",
        "ht\ttps:/alice:secret@example.com/api",
        "https:/\n/alice:secret@example.com/api",
    ] {
        assert_eq!(
            endpoint_label(variant).as_deref(),
            Some("example.com/api"),
            "label leaked or misparsed for {variant:?}"
        );
        assert_eq!(
            endpoint_tooltip(variant).as_deref(),
            Some("https://example.com/api"),
            "tooltip leaked or misparsed for {variant:?}"
        );
    }
    for templated_variant in [
        "https:/alice:secret@{{host}}/api",
        "https:alice:secret@{{host}}/api",
        "https:///alice:secret@{{host}}/api",
        "https:\\\\alice:secret@{{host}}/api",
        "https:/{{user}}:{{pass}}@{{host}}/api",
    ] {
        assert_eq!(
            endpoint_label(templated_variant).as_deref(),
            Some("{{host}}/api"),
            "templated label leaked or misparsed for {templated_variant:?}"
        );
        assert_eq!(
            endpoint_tooltip(templated_variant).as_deref(),
            Some("https://{{host}}/api"),
            "templated tooltip leaked or misparsed for {templated_variant:?}"
        );
    }
    assert_eq!(
        endpoint_label("ws:/alice:secret@example.com/chat").as_deref(),
        Some("example.com/chat")
    );
    assert_eq!(
        endpoint_tooltip("ws:/alice:secret@example.com/chat").as_deref(),
        Some("ws://example.com/chat")
    );
    assert_eq!(
        endpoint_label("/alice:secret@example.com/api").as_deref(),
        Some("example.com/api")
    );
    assert_eq!(
        endpoint_tooltip("/alice:secret@example.com/api").as_deref(),
        Some("example.com/api")
    );
    // Leading Unicode whitespace is trimmed by `str::trim()` when sending, so
    // it must be trimmed before inspecting authority credentials too.
    for whitespace_variant in [
        "\u{a0}/alice:secret@example.com/api",
        "\u{2003}/alice:secret@example.com/api",
        "\u{a0}\\alice:secret@example.com/api",
    ] {
        assert_eq!(
            endpoint_label(whitespace_variant).as_deref(),
            Some("example.com/api"),
            "label leaked for {whitespace_variant:?}"
        );
        assert_eq!(
            endpoint_tooltip(whitespace_variant).as_deref(),
            Some("example.com/api"),
            "tooltip leaked for {whitespace_variant:?}"
        );
    }
    assert_eq!(
        endpoint_tooltip("\u{a0}https://alice:secret@example.com/api").as_deref(),
        Some("https://example.com/api")
    );
    assert_eq!(
        endpoint_tooltip("https://:secret@example.com/api").as_deref(),
        Some("https://example.com/api")
    );
    assert_eq!(
        endpoint_tooltip("https://token@example.com/").as_deref(),
        Some("https://example.com/")
    );
    assert_eq!(
        endpoint_tooltip("{{scheme}}://user:secret@api.test/items?x=1").as_deref(),
        Some("{{scheme}}://api.test/items?x=1")
    );
    assert_eq!(
        endpoint_tooltip("user:secret@example.com/api").as_deref(),
        Some("example.com/api")
    );
    assert_eq!(
        endpoint_label("user:secret@localhost:/api").as_deref(),
        Some("localhost/api")
    );
    assert_eq!(
        endpoint_tooltip("user:secret@localhost:/api").as_deref(),
        Some("localhost/api")
    );
    assert_eq!(
        endpoint_tooltip("//user:secret@example.com/api").as_deref(),
        Some("//example.com/api")
    );
    assert_eq!(
        endpoint_tooltip("http://user:secret@[::1]:8080/health").as_deref(),
        Some("http://[::1]:8080/health")
    );
    assert_eq!(
        endpoint_tooltip("https://{{user}}:{{pass}}@{{host}}/users").as_deref(),
        Some("https://{{host}}/users")
    );

    // Credentials without a host, invalid scheme prefixes, or malformed
    // authorities such as credentials typed before `@host`, must never leak
    // into labels or tooltips.
    for rejected in [
        "://example.com/api",
        "://user:secret@example.com/api",
        "alice:secret@host://example.com",
        "alice:secret@host://{{host}}/api",
        "alice:secret@{{host}}://example.com",
        "{{user}}:{{pass}}@host://example.com",
        "token@host://example.com",
        "token@host://{{host}}/api",
        "123://example.com",
        "123{{s}}://example.com",
        "bad scheme://example.com",
        "bad_scheme://example.com",
        "{{}}://example.com",
        "{{   }}://example.com",
        "{{scheme://example.com",
        "{{scheme}}}://example.com",
        "https://user:secret@",
        "https://user:secret@/",
        "https://user:secret@/api",
        "user:secret@",
        "user:secret@/api",
        "https://alice:secret",
        "https://alice:secret/api",
        "https://:secret",
        "https:/alice:secret",
        "https:alice:secret",
        "alice:secret",
        "/alice:secret",
        "\u{a0}/alice:secret",
        "https://alice:123456",
        "https://{{user}}:secret",
        "{{scheme}}://alice:secret",
        "https://[::1]:secret",
        "https://bad host/api",
        "https://example.com:+80/api",
        "https://{{host}}:{{port",
        "https://{{host}}:{{}}",
        "{{prefix}}/alice:secret@example.com/api",
        "{{prefix}}\\alice:secret@example.com/api",
        "{{prefix}}//alice:secret@example.com/api",
        "{{prefix}}:/alice:secret@example.com/api",
        "{{prefix}}/token@example.com/api",
        "{{prefix}}/:secret@example.com/api",
        "{{prefix}}/alice:secret",
        "{{prefix}}/{{user}}:{{pass}}@example.com/api",
        "http{{suffix}}/alice:secret@example.com/api",
        "{{a}}/{{b}}/alice:secret@example.com/api",
        "https://{{empty}}/alice:secret@example.com/api",
    ] {
        assert_eq!(
            endpoint_label(rejected),
            None,
            "expected None label for {rejected:?}"
        );
        assert_eq!(
            endpoint_tooltip(rejected),
            None,
            "expected None tooltip for {rejected:?}"
        );
    }

    // Valid templated ports remain accepted.
    assert_eq!(
        endpoint_label("https://{{host}}:8080/v1/").as_deref(),
        Some("{{host}}:8080/v1")
    );
    assert_eq!(
        endpoint_label("http://[::1]:{{port}}/health/").as_deref(),
        Some("[::1]:{{port}}/health")
    );

    // `@` inside the path or query is not userinfo.
    assert_eq!(
        endpoint_label("https://example.com/@alice/posts?email=a@b.com").as_deref(),
        Some("example.com/@alice/posts")
    );
    assert_eq!(
        endpoint_tooltip("https://example.com/@alice/posts?email=a@b.com").as_deref(),
        Some("https://example.com/@alice/posts?email=a@b.com")
    );
    assert_eq!(
        endpoint_label("/@alice/posts").as_deref(),
        Some("/@alice/posts")
    );
}

#[test]
fn request_variants_derive_endpoint_labels_and_tooltips() {
    let http = Request::Http(HttpRequest {
        path: "https://httpbin.org/post?verbose=1".into(),
        ..Default::default()
    });
    assert_eq!(http.endpoint_label().as_deref(), Some("httpbin.org/post"));
    assert_eq!(
        http.endpoint_tooltip().as_deref(),
        Some("https://httpbin.org/post?verbose=1")
    );

    let ws = Request::WebSocket(WebSocketRequest {
        url: "wss://echo.websocket.org/stream/".into(),
        ..Default::default()
    });
    assert_eq!(
        ws.endpoint_label().as_deref(),
        Some("echo.websocket.org/stream")
    );
    assert_eq!(
        ws.endpoint_tooltip().as_deref(),
        Some("wss://echo.websocket.org/stream/")
    );

    let grpc = Request::Grpc(GrpcRequest {
        url: "grpcs://user:secret@grpcb.in:9000/".into(),
        method: "/hello.HelloService/SayHello".into(),
        ..Default::default()
    });
    assert_eq!(
        grpc.endpoint_label().as_deref(),
        Some("grpcb.in:9000/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc.endpoint_tooltip().as_deref(),
        Some("grpcs://grpcb.in:9000/hello.HelloService/SayHello")
    );

    let grpc_server_only = GrpcRequest {
        url: "grpcb.in:9000".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_server_only.endpoint_label().as_deref(),
        Some("grpcb.in:9000")
    );
    assert_eq!(
        grpc_server_only.endpoint_tooltip().as_deref(),
        Some("grpcb.in:9000")
    );

    let grpc_method_only = GrpcRequest {
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_method_only.endpoint_label().as_deref(),
        Some("hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_method_only.endpoint_tooltip().as_deref(),
        Some("hello.HelloService/SayHello")
    );

    let grpc_empty = GrpcRequest::default();
    assert_eq!(grpc_empty.endpoint_label(), None);
    assert_eq!(grpc_empty.endpoint_tooltip(), None);

    // gRPC treats only `://` as a scheme, so `http:50051` is host `http` on
    // port `50051` rather than an HTTP URL with decimal IPv4 `0.0.195.131`.
    let grpc_http_host = GrpcRequest {
        url: "http:50051".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_http_host.endpoint_label().as_deref(),
        Some("http:50051/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_http_host.endpoint_tooltip().as_deref(),
        Some("http:50051/hello.HelloService/SayHello")
    );

    // gRPC defaults to port 443 even with `http://`, so an explicit `:80` is
    // meaningful and distinguishes the endpoint from an implicit port.
    let grpc_http_port_80 = GrpcRequest {
        url: "http://grpc.example.com:80".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    let grpc_http_default = GrpcRequest {
        url: "http://grpc.example.com".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_http_port_80.endpoint_label().as_deref(),
        Some("grpc.example.com:80/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_http_port_80.endpoint_tooltip().as_deref(),
        Some("http://grpc.example.com:80/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_http_default.endpoint_label().as_deref(),
        Some("grpc.example.com/hello.HelloService/SayHello")
    );
    assert_ne!(
        grpc_http_port_80.endpoint_label(),
        grpc_http_default.endpoint_label()
    );

    let grpc_creds_port_80 = GrpcRequest {
        url: "\u{a0}http://alice:secret@grpc.example.com:80".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_creds_port_80.endpoint_label().as_deref(),
        Some("grpc.example.com:80")
    );
    assert_eq!(
        grpc_creds_port_80.endpoint_tooltip().as_deref(),
        Some("http://grpc.example.com:80")
    );

    let http_bare_empty_port = HttpRequest {
        path: "localhost:/api".into(),
        ..Default::default()
    };
    assert_eq!(
        http_bare_empty_port.endpoint_label().as_deref(),
        Some("localhost/api")
    );
    assert_eq!(
        http_bare_empty_port.endpoint_tooltip().as_deref(),
        Some("localhost:/api")
    );

    let http_scheme_named_empty_port = HttpRequest {
        path: "http:/api".into(),
        ..Default::default()
    };
    assert_eq!(
        http_scheme_named_empty_port.endpoint_label().as_deref(),
        Some("http/api")
    );
    assert_eq!(
        http_scheme_named_empty_port.endpoint_tooltip().as_deref(),
        Some("http:/api")
    );

    let ws_bare_empty_port = WebSocketRequest {
        url: "localhost:/chat".into(),
        ..Default::default()
    };
    assert_eq!(
        ws_bare_empty_port.endpoint_label().as_deref(),
        Some("localhost/chat")
    );
    assert_eq!(
        ws_bare_empty_port.endpoint_tooltip().as_deref(),
        Some("localhost:/chat")
    );

    let ws_scheme_named_empty_port = WebSocketRequest {
        url: "ws:/chat".into(),
        ..Default::default()
    };
    assert_eq!(
        ws_scheme_named_empty_port.endpoint_label().as_deref(),
        Some("ws/chat")
    );
    assert_eq!(
        ws_scheme_named_empty_port.endpoint_tooltip().as_deref(),
        Some("ws:/chat")
    );

    let ws_bare_default_port = WebSocketRequest {
        url: "localhost:443/chat".into(),
        ..Default::default()
    };
    assert_eq!(
        ws_bare_default_port.endpoint_label().as_deref(),
        Some("localhost/chat")
    );
    assert_eq!(
        ws_bare_default_port.endpoint_tooltip().as_deref(),
        Some("localhost:443/chat")
    );

    let grpc_partial_creds = GrpcRequest {
        url: "http://alice:secret".into(),
        ..Default::default()
    };
    assert_eq!(grpc_partial_creds.endpoint_label(), None);
    assert_eq!(grpc_partial_creds.endpoint_tooltip(), None);

    let grpc_empty_port = GrpcRequest {
        url: "localhost:".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_empty_port.endpoint_label().as_deref(),
        Some("localhost/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_empty_port.endpoint_tooltip().as_deref(),
        Some("localhost:/hello.HelloService/SayHello")
    );

    let grpc_template_scheme = GrpcRequest {
        url: "{{scheme}}://user:secret@grpcb.in:9000".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_template_scheme.endpoint_label().as_deref(),
        Some("{{scheme}}://grpcb.in:9000/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_template_scheme.endpoint_tooltip().as_deref(),
        Some("{{scheme}}://grpcb.in:9000/hello.HelloService/SayHello")
    );

    let grpc_partial_template_scheme = GrpcRequest {
        url: "grpc{{tls}}://grpcb.in:9000".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_partial_template_scheme.endpoint_label().as_deref(),
        Some("grpc{{tls}}://grpcb.in:9000")
    );
    assert_eq!(
        grpc_partial_template_scheme.endpoint_tooltip().as_deref(),
        Some("grpc{{tls}}://grpcb.in:9000")
    );

    let grpc_special_var_name = GrpcRequest {
        url: "grpcs://{{host?name}}".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    };
    assert_eq!(
        grpc_special_var_name.endpoint_label().as_deref(),
        Some("{{host?name}}/hello.HelloService/SayHello")
    );
    assert_eq!(
        grpc_special_var_name.endpoint_tooltip().as_deref(),
        Some("grpcs://{{host?name}}/hello.HelloService/SayHello")
    );

    for invalid_grpc in [
        "alice:secret@host://example.com",
        "alice:secret@{{host}}://example.com",
        "token@host://example.com",
        "://user:secret@example.com",
        "123://example.com",
        "bad scheme://example.com",
        "{{}}://example.com",
        "{{scheme://example.com",
        "{{prefix}}/alice:secret@example.com/api",
        "{{prefix}}/alice:secret",
    ] {
        let request = GrpcRequest {
            url: invalid_grpc.into(),
            ..Default::default()
        };
        assert_eq!(
            request.endpoint_label(),
            None,
            "expected None gRPC label for {invalid_grpc:?}"
        );
        assert_eq!(
            request.endpoint_tooltip(),
            None,
            "expected None gRPC tooltip for {invalid_grpc:?}"
        );
    }
}
