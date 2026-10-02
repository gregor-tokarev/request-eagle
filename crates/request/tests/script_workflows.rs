use std::collections::HashMap;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use environment::{EnvironmentSession, EnvironmentSessions, VariableScopes};
use request::{
    ExecutionError, Field, HttpRequest, ProxyMode, RequestExecutor, RequestPreferences,
    RequestVariables,
};

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let received = requests.clone();
        let shutdown = stop.clone();
        let worker = thread::spawn(move || {
            let mut connections = Vec::new();
            while !shutdown.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                let received = received.clone();
                connections.push(thread::spawn(move || {
                    // macOS accepts sockets in the listener's non-blocking mode,
                    // which would cut large responses short.
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 4096];
                    loop {
                        let Ok(count) = stream.read(&mut buffer) else { return };
                        if count == 0 { return; }
                        request.extend_from_slice(&buffer[..count]);
                        if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&request[..end]);
                            let length = headers.lines().find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                            }).unwrap_or(0);
                            if request.len() >= end + 4 + length { break; }
                        }
                    }
                    let request = String::from_utf8_lossy(&request).into_owned();
                    received.lock().unwrap().push(request.clone());
                    if request.starts_with("GET /slow") { thread::sleep(Duration::from_millis(250)); }
                    let body = if request.starts_with("GET /large") {
                        "x".repeat(1024 * 1024 + 1)
                    } else {
                        serde_json::json!({"token": "session-token", "id": 42, "request": request}).to_string()
                    };
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                }));
            }
            for connection in connections {
                connection.join().unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn executor(&self) -> RequestExecutor {
        let mut preferences = RequestPreferences::default();
        preferences.proxy.mode = ProxyMode::Disabled;
        RequestExecutor::new(&preferences).unwrap()
    }

    fn request(&self, pre: &str, post: &str) -> HttpRequest {
        let mut request = HttpRequest {
            path: format!("{}/main", self.url),
            ..Default::default()
        };
        request.scripts.pre_request = pre.replace("BASE", &self.url);
        request.scripts.post_response = post.replace("BASE", &self.url);
        request
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn variables(session: &EnvironmentSession) -> RequestVariables {
    RequestVariables::with_environment_session(
        HashMap::new(),
        HashMap::new(),
        None,
        session.clone(),
    )
}

fn no_variables() -> RequestVariables {
    RequestVariables::new(HashMap::new(), None)
}

#[test]
fn response_tokens_survive_execution_and_locals_do_not() {
    let server = Server::new();
    let executor = server.executor();
    let session = EnvironmentSession::default();
    let login = server.request(
        "pm.variables.set('local', 'only this request');",
        "pm.environment.set('token', pm.response.json().token);",
    );
    smol::block_on(executor.execute(login, variables(&session))).unwrap();
    let mut next = server.request("pm.expect(pm.variables.has('local')).to.be.false;", "");
    next.headers
        .push(Field::new("Authorization", "Bearer {{token}}"));
    smol::block_on(executor.execute(next.clone(), variables(&session))).unwrap();
    assert!(
        server.requests.lock().unwrap()[1]
            .to_lowercase()
            .contains("authorization: bearer session-token")
    );

    let isolated = EnvironmentSession::default();
    assert!(smol::block_on(executor.execute(next, variables(&isolated))).is_err());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[test]
fn local_values_fill_variables_over_every_scope_for_one_send() {
    let server = Server::new();
    let executor = server.executor();
    let session = EnvironmentSession::default();
    session
        .apply(&VariableScopes {
            environment: [("token".to_owned(), Some("environment".to_owned()))].into(),
            ..Default::default()
        })
        .unwrap();
    let mut request = server.request(
        "pm.expect(pm.variables.get('token')).to.equal('from a flow');",
        "",
    );
    request.headers.push(("X-Token", "{{token}}").into());

    let execution = smol::block_on(executor.execute(
        request.clone(),
        variables(&session).with_local_values([("token".to_owned(), "from a flow".to_owned())]),
    ))
    .unwrap();
    assert!(
        execution.scripts[0].error.is_none(),
        "{:?}",
        execution.scripts
    );
    assert!(
        server.requests.lock().unwrap()[0]
            .to_lowercase()
            .contains("x-token: from a flow")
    );

    // The value lasts for that send only.
    request.scripts.pre_request.clear();
    smol::block_on(executor.execute(request, variables(&session))).unwrap();
    assert!(
        server.requests.lock().unwrap()[1]
            .to_lowercase()
            .contains("x-token: environment")
    );
}

#[test]
fn postman_scopes_carry_values_to_later_requests_and_other_collections() {
    let server = Server::new();
    let executor = server.executor();
    let workspace = EnvironmentSessions::default();
    let session = |path: &str| {
        RequestVariables::with_environment_session(
            HashMap::from([("api".into(), "collection".into())]),
            HashMap::new(),
            None,
            workspace.for_path(Some(path.as_ref())),
        )
    };
    let login = server.request(
        "",
        r#"
        const token = pm.response.json().token;
        pm.globals.set("token", token);
        pm.collectionVariables.set("user", require("lodash").get(pm.response.json(), "id"));
        pm.test("signed", () => pm.expect(require("crypto-js").SHA256(token).toString()).to.have.lengthOf(64));
        "#,
    );
    let result = smol::block_on(executor.execute(login, session("/one/environment.toml"))).unwrap();
    assert!(
        result.scripts[0].error.is_none(),
        "{:?}",
        result.scripts[0].error
    );
    assert!(result.scripts[0].tests[0].error.is_none());

    let mut next = server.request("", "");
    next.path = format!("{}/{{{{api}}}}/{{{{user}}}}", server.url);
    next.headers
        .push(Field::new("Authorization", "Bearer {{token}}"));
    smol::block_on(executor.execute(next.clone(), session("/one/environment.toml"))).unwrap();
    let sent = server.requests.lock().unwrap()[1].to_lowercase();
    assert!(sent.starts_with("get /collection/42 "), "{sent}");
    assert!(sent.contains("authorization: bearer session-token"));

    // Another collection sees the global token, but not the collection variable.
    next.path = format!("{}/{{{{api}}}}", server.url);
    smol::block_on(executor.execute(next.clone(), session("/two/environment.toml"))).unwrap();
    assert!(
        server.requests.lock().unwrap()[2]
            .to_lowercase()
            .contains("authorization: bearer session-token")
    );
    next.path = format!("{}/{{{{user}}}}", server.url);
    assert!(smol::block_on(executor.execute(next, session("/two/environment.toml"))).is_err());
}

#[test]
fn async_auth_calls_resolve_variables_and_modify_only_the_outgoing_request() {
    let server = Server::new();
    let request = server.request(r#"
        pm.environment.set('base', 'BASE');
        pm.variables.set('username', 'eagle');
        const response = await pm.sendRequest({
            url: '{{ base }}/token', method: 'post',
            headers: {'X-User': '{{username}}', 'Content-Type': 'application/json'},
            body: {mode: 'raw', raw: '{"name":"{{username}}"}'},
        });
        pm.request.headers.upsert({key: 'Authorization', value: 'Bearer ' + response.json().token});
        pm.test('token returned', () => response.to.have.status(200));
    "#, "pm.test('authorized', () => pm.expect(pm.response.json().request).to.include('session-token'));");
    let result = smol::block_on(server.executor().execute(request, no_variables())).unwrap();
    assert_eq!(result.scripts.len(), 2);
    assert!(result.scripts.iter().all(
        |report| report.error.is_none() && report.tests.iter().all(|test| test.error.is_none())
    ));
    let received = server.requests.lock().unwrap();
    assert_eq!(received.len(), 2);
    assert!(received[0].starts_with("POST /token "));
    assert!(received[0].ends_with("{\"name\":\"eagle\"}"));
    assert!(
        received[1]
            .to_lowercase()
            .contains("authorization: bearer session-token")
    );
}

#[test]
fn malformed_subrequest_templates_fail_before_sending() {
    let server = Server::new();
    let executor = server.executor();

    for (template, expected_error) in [
        ("{{token", "Unclosed variable"),
        ("{{", "Unclosed variable"),
        ("{{!token", "Unclosed variable"),
        ("{{outer{{token}}", "Unknown variable {{outer{{token}}"),
        ("{{{token}}}", "Unknown variable {{{token}}"),
        ("{{}}", "Unknown variable {{}}"),
    ] {
        for field in ["url", "header name", "header value", "body", "raw body"] {
            let mut config = serde_json::json!({"url": "BASE/template", "method": "POST"});
            match field {
                "url" => config["url"] = format!("BASE/{template}").into(),
                "header name" => config["headers"] = serde_json::json!([[template, "value"]]),
                "header value" => config["headers"] = serde_json::json!([["X-Test", template]]),
                "body" => config["body"] = template.into(),
                "raw body" => {
                    config["body"] = serde_json::json!({"mode": "raw", "raw": template});
                }
                _ => unreachable!(),
            }
            let source =
                format!("pm.variables.set('token', 'resolved'); await pm.sendRequest({config});");
            let error =
                smol::block_on(executor.execute(server.request(&source, ""), no_variables()))
                    .unwrap_err();
            let ExecutionError::Script { message, .. } = error else {
                panic!("Expected a script error for {field}: {template}, got {error}");
            };
            assert!(
                message.contains(expected_error),
                "{field}: {template}: {message}"
            );
        }
    }

    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn subrequests_preserve_escaped_literals_and_resolve_nested_names_once() {
    let server = Server::new();
    let request = server.request(
        r#"
        pm.variables.set('token', 'resolved');
        pm.variables.set('literal', '{{unclosed');
        pm.variables.set('header', 'X-Literal');
        pm.variables.set('outer{{token', 'nested-name-value');
        const literal = '{{!token}}/{{! token }}/{{!!token}}/{{!{token}}}/{{!#if token}}yes{{!/if}}/}}/{token}}/{{literal}}';
        await pm.sendRequest({
            url: 'BASE/{{!token}}', method: 'POST',
            headers: {'{{header}}': literal},
            body: {mode: 'raw', raw: literal + '/{{outer{{token}}'},
        });
    "#,
        "",
    );

    smol::block_on(server.executor().execute(request, no_variables())).unwrap();

    let received = server.requests.lock().unwrap();
    assert_eq!(received.len(), 2);
    let literal = "{{token}}/{{ token }}/{{!token}}/{{{token}}}/{{#if token}}yes{{/if}}/}}/{token}}/{{unclosed";
    assert!(received[0].starts_with("POST /%7B%7Btoken%7D%7D "));
    assert!(received[0].contains(&format!("x-literal: {literal}\r\n")));
    assert!(received[0].ends_with(&format!("{literal}/nested-name-value")));
}

#[test]
fn subrequests_ignore_templates_in_literal_and_expanded_url_fragments() {
    let server = Server::new();
    let request = server.request(
        r#"
        await pm.sendRequest('BASE/literal#{{unclosed');
        pm.variables.set('url', 'BASE/expanded#fragment');
        await pm.sendRequest('{{url}}/{{unclosed');
    "#,
        "",
    );

    smol::block_on(server.executor().execute(request, no_variables())).unwrap();

    let received = server.requests.lock().unwrap();
    assert_eq!(received.len(), 3);
    assert!(received[0].starts_with("GET /literal "));
    assert!(received[1].starts_with("GET /expanded "));
}

#[test]
fn get_and_head_calls_omit_bodies_before_parsing_resolving_or_serializing() {
    let server = Server::new();
    let request = server.request(
        r#"
        for (const method of ['GET', 'head', undefined]) {
            for (const body of ['{{missing}}', '{{unclosed', '{{outer{{inner}}', {mode: 'formdata'}, 'x'.repeat(1048577)]) {
                const response = await pm.sendRequest({url: 'BASE/ignored-body', method, body});
                response.to.have.status(200);
            }
            await pm.sendRequest({
                url: 'BASE/ignored-body', method,
                get body() { throw Error('ignored body was read'); },
            });
        }
    "#,
        "",
    );

    smol::block_on(server.executor().execute(request, no_variables())).unwrap();

    let received = server.requests.lock().unwrap();
    assert_eq!(received.len(), 19);
    assert_eq!(
        received
            .iter()
            .filter(|request| request.starts_with("HEAD "))
            .count(),
        6
    );
    assert!(
        received
            .iter()
            .all(|request| { request.starts_with("GET ") || request.starts_with("HEAD ") })
    );
    assert!(received.iter().all(|request| request.ends_with("\r\n\r\n")));
}

#[test]
fn async_tests_and_unawaited_callbacks_finish_before_reporting() {
    let server = Server::new();
    let request = server.request("", r#"
        pm.test('async test', async () => {
            const responses = await Promise.all([pm.sendRequest('BASE/first'), pm.sendRequest('BASE/second')]);
            pm.expect(responses.length).to.equal(2);
        });
        pm.sendRequest('BASE/callback', (error, response) => {
            pm.test('callback test', () => { pm.expect(error).to.be.null; response.to.have.status(200); });
        });
        pm.test('async failure', async () => { await Promise.resolve(); pm.expect(1).to.equal(2); });
    "#);
    let result = smol::block_on(server.executor().execute(request, no_variables())).unwrap();
    let report = &result.scripts[0];
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.tests.len(), 3);
    assert_eq!(
        report
            .tests
            .iter()
            .filter(|test| test.error.is_some())
            .count(),
        1
    );
    assert_eq!(server.requests.lock().unwrap().len(), 4);
}

#[test]
fn callback_errors_are_not_called_twice_or_silently_swallowed() {
    let server = Server::new();
    let request = server.request(
        r#"
        let calls = 0;
        try {
            await pm.sendRequest('BASE/token', () => { calls++; throw Error('callback failed'); });
            throw Error('callback failure was swallowed');
        } catch (error) {
            pm.expect(error.message).to.equal('callback failed');
        }
        pm.expect(calls).to.equal(1);
    "#,
        "",
    );
    smol::block_on(server.executor().execute(request, no_variables())).unwrap();
}

#[test]
fn promise_errors_and_unresolved_async_tests_fail_without_sending() {
    let server = Server::new();
    for source in [
        "Promise.reject('lost'); await Promise.reject('handled').catch(() => {});",
        "await new Promise(() => {});",
        "pm.test('never completes', async () => await new Promise(() => {}));",
        "await pm.sendRequest('file:///tmp/local-file');",
        "await pm.sendRequest('BASE/{{constructor}}');",
    ] {
        let request = server.request(source, "");
        assert!(
            matches!(
                smol::block_on(server.executor().execute(request, no_variables())),
                Err(ExecutionError::Script { .. })
            ),
            "{source}"
        );
    }
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn local_overrides_reveal_environment_when_unset_and_failed_phases_do_not_commit() {
    let server = Server::new();
    let session = EnvironmentSession::default();
    session
        .apply(&VariableScopes {
            environment: [("token".into(), Some("initial".into()))].into(),
            ..Default::default()
        })
        .unwrap();
    let executor = server.executor();
    let request = server.request(
        r#"
        pm.variables.set('token', 'local');
        pm.expect(pm.variables.get('token')).to.equal('local');
        pm.variables.unset('token');
        pm.expect(pm.variables.get('token')).to.equal('initial');
        pm.environment.set('token', 'pre');
    "#,
        "pm.environment.set('token', 'post'); throw Error('discard changes');",
    );
    let result = smol::block_on(executor.execute(request, variables(&session))).unwrap();
    assert!(result.scripts[1].error.is_some());
    assert_eq!(
        session.values(HashMap::new(), HashMap::new())["token"],
        "pre"
    );
    let request = server.request(
        "pm.environment.set('token', 'failed'); throw Error('discard');",
        "",
    );
    assert!(smol::block_on(executor.execute(request, variables(&session))).is_err());
    assert_eq!(
        session.values(HashMap::new(), HashMap::new())["token"],
        "pre"
    );
}

#[test]
fn skip_preserves_reason_and_does_not_send_queued_or_main_requests() {
    let server = Server::new();
    let request = server.request(
        r#"
        console.log('checking setup');
        pm.sendRequest('BASE/queued');
        pm.execution.skipRequest('No access token configured');
        throw Error('must not run');
    "#,
        "throw Error('must not run post-response');",
    );
    let error = smol::block_on(server.executor().execute(request, no_variables())).unwrap_err();
    let ExecutionError::Skipped { reason, report } = error else {
        panic!("{error:?}")
    };
    assert_eq!(reason, "No access token configured");
    assert!(report.error.is_none());
    assert_eq!(report.logs.len(), 1);
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn crypto_and_schema_helpers_are_available_in_real_request_scripts() {
    let server = Server::new();
    let request = server.request(r#"
        pm.request.headers.upsert({key: 'X-Signature', value: pm.crypto.hmacSha256('key', 'message')});
        pm.test('encoding', () => pm.expect(pm.encoding.base64Decode(pm.encoding.base64Encode('🦅'))).to.equal('🦅'));
    "#, r#"
        pm.test('schema passes', () => pm.response.to.have.jsonSchema({type: 'object', required: ['id'], properties: {id: {type: 'integer'}}}));
        pm.test('schema fails usefully', () => pm.response.to.have.jsonSchema({properties: {id: {type: 'string'}}}));
        const result = pm.schema.validate(pm.response.json(), {properties: {id: {minimum: 100}}});
        pm.test('error location', () => pm.expect(result.errors[0].instancePath).to.equal('/id'));
    "#);
    let result = smol::block_on(server.executor().execute(request, no_variables())).unwrap();
    assert!(result.scripts.iter().all(|report| report.error.is_none()));
    assert!(result.scripts[1].tests[0].error.is_none());
    assert!(
        result.scripts[1].tests[1]
            .error
            .as_ref()
            .unwrap()
            .contains("/id")
    );
    assert!(result.scripts[1].tests[2].error.is_none());
}

#[test]
fn script_calls_inherit_response_limits_and_timeout() {
    let server = Server::new();
    let mut preferences = RequestPreferences::default();
    preferences.proxy.mode = ProxyMode::Disabled;
    preferences.max_response_size_mb = 1;
    let executor = RequestExecutor::new(&preferences).unwrap();
    let request = server.request("await pm.sendRequest('BASE/large');", "");
    let error = smol::block_on(executor.execute(request, no_variables())).unwrap_err();
    assert!(error.to_string().contains("limit"), "{error}");
    preferences.timeout_ms = 30;
    let executor = RequestExecutor::new(&preferences).unwrap();
    let request = server.request("await pm.sendRequest('BASE/slow');", "");
    let start = Instant::now();
    assert!(smol::block_on(executor.execute(request, no_variables())).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(
        !server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.starts_with("GET /main"))
    );
}

#[test]
fn cancellation_during_script_http_does_not_commit_environment_or_send_main() {
    let server = Server::new();
    let session = EnvironmentSession::default();
    let request = server.request(
        "pm.environment.set('token', 'cancelled'); await pm.sendRequest('BASE/slow');",
        "",
    );
    let future = server.executor().execute(request, variables(&session));
    smol::block_on(smol::future::or(
        async {
            let _ = future.await;
        },
        async {
            smol::Timer::after(Duration::from_millis(40)).await;
        },
    ));
    thread::sleep(Duration::from_millis(100));
    assert!(session.values(HashMap::new(), HashMap::new()).is_empty());
    assert!(
        !server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.starts_with("GET /main"))
    );
}

fn collection_scripts(pre: &str, post: &str) -> Result<request::RequestScripts, String> {
    Ok(request::RequestScripts {
        pre_request: pre.into(),
        post_response: post.into(),
    })
}

#[test]
fn collection_scripts_run_before_the_request_scripts_in_each_phase() {
    let server = Server::new();
    let session = EnvironmentSession::default();
    let request = server.request(
        "pm.test('sees collection values', () => pm.expect(pm.variables.get('order')).to.equal('collection'));
         pm.variables.set('order', 'request');",
        "pm.test('sees collection token', () => pm.expect(pm.environment.get('token')).to.equal('session-token'));",
    );
    let variables = variables(&session).with_collection_scripts(collection_scripts(
        "pm.variables.set('order', 'collection');
         pm.request.headers.upsert({key: 'X-Collection', value: '{{order}}'});",
        "pm.environment.set('token', pm.response.json().token);",
    ));

    let execution = smol::block_on(server.executor().execute(request, variables)).unwrap();

    let labels: Vec<_> = execution
        .scripts
        .iter()
        .map(|report| report.label())
        .collect();
    assert_eq!(
        labels,
        [
            "Collection pre-request",
            "Pre-request",
            "Collection post-response",
            "Post-response"
        ]
    );
    assert!(execution.scripts.iter().all(
        |report| report.error.is_none() && report.tests.iter().all(|test| test.error.is_none())
    ));
    // The request script's value replaces the collection's before variables resolve.
    assert!(
        server.requests.lock().unwrap()[0]
            .to_lowercase()
            .contains("x-collection: request")
    );
}

#[test]
fn a_failing_request_script_keeps_the_collection_report_and_sends_nothing() {
    let server = Server::new();
    let request = server.request("throw new Error('request failed');", "");
    let variables = RequestVariables::new(HashMap::new(), None)
        .with_collection_scripts(collection_scripts("console.log('collection ran');", ""));

    let error = smol::block_on(server.executor().execute(request, variables)).unwrap_err();

    let ExecutionError::ScriptedRequest { source, reports } = error else {
        panic!("expected both script reports, got {error}");
    };
    assert!(matches!(*source, ExecutionError::Script { .. }));
    assert_eq!(reports.len(), 2);
    assert!(reports[0].collection);
    assert_eq!(reports[0].logs[0].message, "collection ran");
    assert!(!reports[1].collection);
    assert!(
        reports[1]
            .error
            .as_deref()
            .unwrap()
            .contains("request failed")
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn unreadable_collection_scripts_stop_the_send() {
    let server = Server::new();
    let variables = RequestVariables::new(HashMap::new(), None)
        .with_collection_scripts(Err("Could not read the collection scripts".into()));

    let error =
        smol::block_on(server.executor().execute(server.request("", ""), variables)).unwrap_err();

    let ExecutionError::Script { message, report } = error else {
        panic!("expected a script error, got {error}");
    };
    assert_eq!(message, "Could not read the collection scripts");
    assert_eq!(report.label(), "Collection pre-request");
    assert!(server.requests.lock().unwrap().is_empty());
}
