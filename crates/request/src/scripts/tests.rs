use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use std::collections::HashMap;

use super::{
    RequestScripts, ScriptPhase, ScriptReport,
    runtime::{self, Cancellation, ScriptState},
};
use crate::{
    Execution, ExecutionError, HeaderMap, HttpMetrics, HttpRequest, HttpResponse, Method,
    RequestExecutor, RequestPreferences, RequestVariables, Response, StatusCode, Version,
};

fn scripted(source: &str) -> HttpRequest {
    HttpRequest {
        path: "http://localhost/".into(),
        scripts: RequestScripts {
            pre_request: source.into(),
            post_response: String::new(),
        },
        ..Default::default()
    }
}

fn cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn executor() -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences::default()).unwrap()
}

fn no_variables() -> RequestVariables {
    RequestVariables::new(HashMap::new(), None)
}

async fn pre_request(
    request: HttpRequest,
    variables: RequestVariables,
) -> Result<(HttpRequest, ScriptState, Vec<ScriptReport>), ExecutionError> {
    runtime::pre_request(request, variables, executor(), cancelled()).await
}

async fn post_response(
    request: HttpRequest,
    request_body: Option<bytes::Bytes>,
    state: ScriptState,
    execution: Execution,
) -> Execution {
    runtime::post_response(
        request,
        request_body,
        state,
        execution,
        executor(),
        cancelled(),
    )
    .await
}

#[test]
fn edits_only_the_outgoing_snapshot_and_resolves_variables_in_all_fields() {
    smol::block_on(async {
        let mut request = scripted(
            r#"
            pm.variables.set("path", "hello");
            pm.variables.set("value", "a & b");
            pm.request.method = "POST";
            pm.request.headers.upsert({key: "X-Test", value: "{{value}}"});
            pm.request.body.update('{"message":"{{value}}"}');
            console.log("Sending", pm.request.url);
        "#,
        );
        request.path = "http://localhost/{{path}}".into();
        request.headers = vec![("x-test".into(), "old".into())];
        request.query = Some(vec![("q".into(), "{{value}}".into())]);
        let original = request.clone();
        let (sent, state, reports) = pre_request(request, no_variables()).await.unwrap();

        assert_eq!(sent.path, "http://localhost/hello");
        assert_eq!(sent.method, Method::Post);
        assert_eq!(
            sent.headers,
            [
                ("X-Test".into(), "a & b".into()),
                ("Content-Type".into(), "application/json".into())
            ]
        );
        assert_eq!(sent.query.unwrap()[0].1, "a & b");
        assert_eq!(sent.body.unwrap(), br#"{"message":"a & b"}"#);
        assert_eq!(state.variables.values["path"], "hello");
        assert_eq!(reports[0].logs.len(), 1);
        assert_eq!(original.path, "http://localhost/{{path}}");
        assert_eq!(original.method, Method::Get);
    });
}

#[test]
fn request_body_reads_preserve_bytes_and_edits_are_exported() {
    smol::block_on(async {
        for (body, source, expected) in [
            (None, "pm.expect(pm.request.body.raw).to.be.null;", None),
            (
                Some(vec![]),
                "pm.expect(pm.request.body.raw).to.equal('');",
                Some(vec![]),
            ),
            (
                Some(vec![0, 255, 42]),
                "const body = pm.request.body.raw; pm.expect(body).to.equal('\\u0000\\ufffd*'); pm.request.body.raw = body;",
                Some(vec![0, 255, 42]),
            ),
            (
                Some(b"original".to_vec()),
                "pm.request.body.raw = 'edited'; pm.expect(pm.request.body.raw).to.equal('edited');",
                Some(b"edited".to_vec()),
            ),
            (
                Some(b"original".to_vec()),
                "const body = pm.request.body.raw; pm.request.body.update('edited'); pm.request.body.raw = body;",
                Some(b"original".to_vec()),
            ),
            (
                Some(b"original".to_vec()),
                "pm.request.body.raw = null; pm.expect(pm.request.body.raw).to.be.null;",
                None,
            ),
        ] {
            let mut request = scripted(source);
            request.method = Method::Post;
            request.body = body;
            let (sent, _, _) = pre_request(request, no_variables()).await.unwrap();
            assert_eq!(sent.body, expected, "{source}");
        }
    });
}

#[test]
fn unread_bodies_can_exceed_the_js_heap_and_keep_their_buffers() {
    smol::block_on(async {
        let mut request = scripted("pm.variables.set('path', 'large');");
        request.method = Method::Post;
        request.body = Some(vec![b'x'; 40 * 1024 * 1024]);
        let request_buffer = request.body.as_ref().unwrap().as_ptr();
        let (mut request, _, _) = pre_request(request, no_variables()).await.unwrap();
        assert_eq!(request.body.as_ref().unwrap().as_ptr(), request_buffer);

        request.scripts.post_response =
            "pm.response.to.have.status(200); throw new Error('after status');".into();
        request.scripts.pre_request.clear();
        let (mut request, state, _) = pre_request(request, no_variables()).await.unwrap();
        assert_eq!(request.body.as_ref().unwrap().as_ptr(), request_buffer);
        let request_body = request.body.take().map(bytes::Bytes::from);
        // Fits as bytes, but replacement characters would exceed the decode budget.
        let body = vec![255; 11 * 1024 * 1024];
        let response_buffer = body.as_ptr();
        let execution = Execution {
            elapsed: Duration::ZERO,
            scripts: Vec::new(),
            response: Response::Http(HttpResponse {
                status: StatusCode::OK,
                version: Version::HTTP_11,
                headers: HeaderMap::new(),
                body,
                metrics: HttpMetrics::default(),
            }),
        };
        let first_state = ScriptState {
            variables: state.variables.clone(),
            session: None,
            collection_post_response: String::new(),
        };
        let mut result = post_response(
            request.clone(),
            request_body.clone(),
            first_state,
            execution,
        )
        .await;
        assert!(
            result.scripts[0]
                .error
                .as_ref()
                .unwrap()
                .contains("after status")
        );
        let Response::Http(response) = &result.response;
        assert_eq!(response.body.as_ptr(), response_buffer);

        request.scripts.post_response = "pm.response.text();".into();
        result = post_response(request, request_body, state, result).await;
        assert!(
            result.scripts[1]
                .error
                .as_ref()
                .unwrap()
                .contains("decoding limit"),
            "lossy decoding must be bounded before allocating native text"
        );
        let Response::Http(response) = result.response;
        assert_eq!(response.body.as_ptr(), response_buffer);
        assert_eq!(response.body.len(), 11 * 1024 * 1024);
    });
}

#[test]
fn collection_variables_resolve_once_after_scripts_and_remain_bounded() {
    smol::block_on(async {
        let values = HashMap::from([("value".into(), "from file".into())]);
        let mut request = scripted(
            "pm.expect(pm.variables.get('value')).to.equal('from file'); pm.variables.set('path', 'created'); pm.variables.set('value', 'local');",
        );
        request.path = "http://localhost/{{path}}".into();
        request.headers = vec![("X-Value".into(), "{{value}}/{{!value}}/{{$guid}}".into())];
        request.query = Some(vec![("id".into(), "{{$guid}}".into())]);
        let (sent, state, _) = pre_request(request, RequestVariables::new(values, None))
            .await
            .unwrap();
        assert_eq!(sent.path, "http://localhost/created");
        assert_eq!(state.variables.values["value"], "local");
        assert_eq!(
            sent.headers[0].1,
            format!("local/{{{{value}}}}/{}", sent.query.unwrap()[0].1)
        );

        let mut request = scripted("pm.variables.set('path', 'fallback');");
        request.path = "http://localhost/{{path}}".into();
        request.body = Some(vec![0, 255, 42]);
        request.method = Method::Post;
        let variables = RequestVariables::new(HashMap::new(), Some("File unavailable".into()));
        let (sent, _, _) = pre_request(request, variables).await.unwrap();
        assert_eq!(sent.path, "http://localhost/fallback");
        assert_eq!(sent.body.unwrap(), [0, 255, 42]);

        let mut request = scripted("pm.variables.set('path', 'x'.repeat(1024 * 1024));");
        request.path = format!("http://localhost/{}", "{{path}}".repeat(33));
        let error = pre_request(request, no_variables()).await.unwrap_err();
        assert!(error.to_string().contains("output limit"), "{error}");

        let request =
            scripted("pm.request.url = 'https://example.com/{{' + 'x'.repeat(10000) + '}}';");
        let error = pre_request(request, no_variables()).await.unwrap_err();
        let ExecutionError::Script { message, report } = error else {
            panic!("expected script failure")
        };
        assert_eq!(message.chars().count(), 4096);
        assert_eq!(report.error.as_deref(), Some(message.as_str()));
    });
}

#[test]
fn script_method_changes_control_body_resolution_and_dynamic_overrides_win() {
    smol::block_on(async {
        for (before, after, body, expected) in [
            (Method::Post, "GET", "{{missing}}", None),
            (Method::Put, "HEAD", "{{unclosed", None),
            (Method::Get, "POST", "{{$guid}}", Some("fixed")),
            (Method::Head, "PUT", "{{$guid}}", Some("fixed")),
        ] {
            let mut request = scripted(&format!(
                "pm.request.method = '{after}'; pm.variables.set('$guid', 'fixed'); pm.expect(pm.variables.replaceIn('{{{{$guid}}}}')).to.equal('fixed');"
            ));
            request.method = before;
            request.path = "http://localhost/{{$guid}}".into();
            request.headers = vec![("X-Id".into(), "{{$guid}}".into())];
            request.query = Some(vec![("id".into(), "{{$guid}}".into())]);
            request.body = Some(body.as_bytes().to_vec());
            let values = HashMap::from([("$guid".into(), "from file".into())]);
            let (sent, _, _) = pre_request(request, RequestVariables::new(values, None))
                .await
                .unwrap();
            assert_eq!(sent.path, "http://localhost/fixed");
            assert_eq!(sent.headers[0].1, "fixed");
            assert_eq!(sent.query.unwrap()[0].1, "fixed");
            assert_eq!(sent.body.as_deref(), expected.map(str::as_bytes));
        }
    });
}

#[test]
fn response_tests_keep_failures_logs_and_response_data() {
    smol::block_on(async {
        let mut request = scripted("pm.variables.set('token', 'abc');");
        request.scripts.post_response = r#"
            pm.test("status", () => pm.response.to.have.status(201));
            pm.test("json", () => pm.expect(pm.response.json()).to.deep.equal({ok: true}));
            pm.test("variable", () => pm.expect(pm.variables.get('token')).to.equal('abc'));
            pm.test("header", () => pm.response.to.have.header('Content-Type', 'application/json'));
            pm.test("failure", () => pm.expect(pm.response.code).to.equal(200));
            pm.test("continues", () => pm.expect(pm.response.responseTime).to.be.below(100));
            console.log('response', pm.response.json());
            throw new Error('after tests');
        "#
        .into();
        let (request, state, scripts) = pre_request(request, no_variables()).await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        let execution = Execution {
            elapsed: Duration::from_millis(42),
            scripts,
            response: Response::Http(HttpResponse {
                status: StatusCode::CREATED,
                version: Version::HTTP_11,
                headers,
                body: br#"{"ok":true}"#.to_vec(),
                metrics: HttpMetrics::default(),
            }),
        };
        let result = post_response(request, None, state, execution).await;
        let report = &result.scripts[1];
        assert_eq!(report.phase, ScriptPhase::PostResponse);
        assert_eq!(report.tests.len(), 6);
        assert_eq!(
            report
                .tests
                .iter()
                .filter(|test| test.error.is_none())
                .count(),
            5
        );
        assert!(report.tests[4].error.as_ref().unwrap().contains("200"));
        assert!(report.error.as_ref().unwrap().contains("after tests"));
        assert!(report.logs[0].message.contains("true"));
        let Response::Http(response) = result.response;
        assert_eq!(response.body, br#"{"ok":true}"#);
    });
}

#[test]
fn exceptions_invalid_request_data_and_unhandled_rejections_fail_before_sending() {
    smol::block_on(async {
        for source in [
            "throw new Error('boom');",
            "const = ;",
            "pm.request.method = 'TYPO';",
            "Promise.reject('bad'); void 0;",
            "pm.test('async', async () => {}); throw new Error('stop');",
        ] {
            let error = pre_request(scripted(source), no_variables())
                .await
                .unwrap_err();
            assert!(
                matches!(error, ExecutionError::Script { .. }),
                "{source}: {error}"
            );
        }
    });
}

#[test]
fn uncaught_errors_are_bounded_without_splitting_unicode() {
    smol::block_on(async {
        for source in [
            "throw new Error('x'.repeat(8 * 1024 * 1024));",
            "throw new Error('🦅'.repeat(10000));",
        ] {
            let error = pre_request(scripted(source), no_variables())
                .await
                .unwrap_err();
            let ExecutionError::Script { message, report } = error else {
                panic!("expected a script error");
            };

            assert!(message.contains("Error:"));
            assert_eq!(message.chars().count(), 4096);
            assert_eq!(report.error.as_deref(), Some(message.as_str()));
        }
    });
}

#[test]
fn runtime_is_isolated_and_has_no_host_io() {
    smol::block_on(async {
        let source = "pm.test('sandbox', () => { for (const name of ['process', 'require', 'fetch', 'std', 'os']) pm.expect(typeof globalThis[name]).to.equal('undefined'); }); globalThis.leak = 42;";
        let (_, _, reports) = pre_request(scripted(source), no_variables()).await.unwrap();
        assert!(reports[0].tests[0].error.is_none());
        let (_, _, reports) = pre_request(
            scripted("pm.test('fresh', () => pm.expect(typeof leak).to.equal('undefined'));"),
            no_variables(),
        )
        .await
        .unwrap();
        assert!(reports[0].tests[0].error.is_none());
    });
}

#[test]
fn loops_memory_and_output_are_bounded() {
    smol::block_on(async {
        let started = Instant::now();
        let error = pre_request(scripted("while (true) {}"), no_variables())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("time limit"));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            pre_request(
                scripted(
                    "const values = []; while (true) values.push(new Array(100000).fill(123));"
                ),
                no_variables()
            )
            .await
            .is_err()
        );
        let (_, _, reports) = pre_request(
            scripted("for (let i = 0; i < 600; i++) console.log('entry', i);"),
            no_variables(),
        )
        .await
        .unwrap();
        assert_eq!(reports[0].logs.len(), 500);
        assert!(
            pre_request(
                scripted("for (let i = 0; i < 501; i++) pm.test('test', () => {});"),
                no_variables()
            )
            .await
            .is_err()
        );
    });
}

#[test]
fn cancellation_interrupts_the_blocking_worker() {
    smol::block_on(async {
        let cancellation = Cancellation::new();
        let flag = cancellation.0.clone();
        let started = Instant::now();
        let run = smol::spawn(runtime::pre_request(
            scripted("while (true) {}"),
            no_variables(),
            executor(),
            flag,
        ));
        smol::Timer::after(Duration::from_millis(30)).await;
        drop(cancellation);
        let error = run.await.unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
    });
}

#[test]
fn request_timeout_also_interrupts_scripts() {
    smol::block_on(async {
        let executor = RequestExecutor::new(&RequestPreferences {
            timeout_ms: 30,
            ..Default::default()
        })
        .unwrap();
        let started = Instant::now();
        assert!(matches!(
            executor
                .execute(scripted("while (true) {} "), no_variables())
                .await,
            Err(ExecutionError::Timeout { .. })
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    });
}

#[test]
fn variable_expansion_cannot_allocate_an_unbounded_request() {
    smol::block_on(async {
        let mut request = scripted("pm.variables.set('large', 'x'.repeat(1024 * 1024));");
        request.method = Method::Post;
        request.body = Some("{{large}}".repeat(40).into_bytes());
        let error = pre_request(request, no_variables()).await.unwrap_err();
        assert!(error.to_string().contains("output limit"));
    });
}
