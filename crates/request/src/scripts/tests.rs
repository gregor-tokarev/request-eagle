use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use super::{
    RequestScripts, ScriptPhase,
    runtime::{Cancellation, post_response, pre_request},
};
use crate::{
    Execution, ExecutionError, HeaderMap, HttpMetrics, HttpRequest, HttpResponse, Method,
    RequestExecutor, RequestPreferences, Response, StatusCode, Version,
};

fn scripted(source: &str) -> HttpRequest {
    HttpRequest {
        path: "http://localhost/{{path}}".into(),
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
        request.headers = vec![("x-test".into(), "old".into())];
        request.query = Some(vec![("q".into(), "{{value}}".into())]);
        let original = request.clone();
        let (sent, vars, reports) = pre_request(request, cancelled()).await.unwrap();

        assert_eq!(sent.path, "http://localhost/hello");
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.headers, [("X-Test".into(), "a & b".into())]);
        assert_eq!(sent.query.unwrap()[0].1, "a & b");
        assert_eq!(sent.body.unwrap(), br#"{"message":"a & b"}"#);
        assert_eq!(vars["path"], "hello");
        assert_eq!(reports[0].logs.len(), 1);
        assert_eq!(original.path, "http://localhost/{{path}}");
        assert_eq!(original.method, Method::Get);
    });
}

#[test]
fn binary_bodies_and_unknown_variables_are_preserved() {
    smol::block_on(async {
        let mut request = scripted("pm.variables.set('other', '{{nested}}');");
        request.body = Some(vec![0, 255, 42]);
        let (sent, _, _) = pre_request(request, cancelled()).await.unwrap();
        assert_eq!(sent.body.unwrap(), [0, 255, 42]);
        assert_eq!(sent.path, "http://localhost/{{path}}");
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
        let (request, variables, scripts) = pre_request(request, cancelled()).await.unwrap();
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
        let result = post_response(request, variables, execution, cancelled()).await;
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
fn exceptions_invalid_request_data_and_async_scripts_fail_before_sending() {
    smol::block_on(async {
        for source in [
            "throw new Error('boom');",
            "const = ;",
            "pm.request.method = 'TYPO';",
            "Promise.resolve().then(() => {});",
            "Promise.reject('bad'); void 0;",
            "pm.test('async', async () => {}); throw new Error('stop');",
        ] {
            let error = pre_request(scripted(source), cancelled())
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
fn runtime_is_isolated_and_has_no_host_io() {
    smol::block_on(async {
        let source = "pm.test('sandbox', () => { for (const name of ['process', 'require', 'fetch', 'std', 'os']) pm.expect(typeof globalThis[name]).to.equal('undefined'); }); globalThis.leak = 42;";
        let (_, _, reports) = pre_request(scripted(source), cancelled()).await.unwrap();
        assert!(reports[0].tests[0].error.is_none());
        let (_, _, reports) = pre_request(
            scripted("pm.test('fresh', () => pm.expect(typeof leak).to.equal('undefined'));"),
            cancelled(),
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
        let error = pre_request(scripted("while (true) {}"), cancelled())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("time limit"));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            pre_request(
                scripted(
                    "const values = []; while (true) values.push(new Array(100000).fill(123));"
                ),
                cancelled()
            )
            .await
            .is_err()
        );
        let (_, _, reports) = pre_request(
            scripted("for (let i = 0; i < 600; i++) console.log('entry', i);"),
            cancelled(),
        )
        .await
        .unwrap();
        assert_eq!(reports[0].logs.len(), 500);
        assert!(
            pre_request(
                scripted("for (let i = 0; i < 501; i++) pm.test('test', () => {});"),
                cancelled()
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
        let run = smol::spawn(pre_request(scripted("while (true) {}"), flag));
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
            executor.execute(scripted("while (true) {} ")).await,
            Err(ExecutionError::Timeout { .. })
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    });
}

#[test]
fn variable_expansion_cannot_allocate_an_unbounded_request() {
    smol::block_on(async {
        let mut request = scripted("pm.variables.set('large', 'x'.repeat(1024 * 1024));");
        request.body = Some("{{large}}".repeat(40).into_bytes());
        let error = pre_request(request, cancelled()).await.unwrap_err();
        assert!(error.to_string().contains("output limit"));
    });
}
