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

use environment::{EnvironmentSession, VariableValues};

use crate::{
    ExecutionError, HttpRequest, ProxyMode, RequestExecutor, RequestPreferences, RequestVariables,
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
    RequestVariables::with_environment_session(VariableValues::default(), None, session.clone())
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
    let login_before = login.clone();
    smol::block_on(executor.execute_with_variables(&login, variables(&session))).unwrap();
    let mut next = server.request("pm.expect(pm.variables.has('local')).to.be.false;", "");
    next.headers
        .push(("Authorization".into(), "Bearer {{token}}".into()));
    smol::block_on(executor.execute_with_variables(&next, variables(&session))).unwrap();
    assert_eq!(login, login_before);
    assert_eq!(next.headers[0].1, "Bearer {{token}}");
    assert!(
        server.requests.lock().unwrap()[1]
            .to_lowercase()
            .contains("authorization: bearer session-token")
    );

    let isolated = EnvironmentSession::default();
    assert!(smol::block_on(executor.execute_with_variables(next, variables(&isolated))).is_err());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
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
    let result = smol::block_on(server.executor().execute(&request)).unwrap();
    assert_eq!(result.scripts.len(), 2);
    assert!(result.scripts.iter().all(
        |report| report.error.is_none() && report.tests.iter().all(|test| test.error.is_none())
    ));
    assert!(request.headers.is_empty());
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
fn get_and_head_calls_omit_bodies_before_parsing_resolving_or_serializing() {
    let server = Server::new();
    let request = server.request(
        r#"
        for (const method of ['GET', 'head', undefined]) {
            for (const body of ['{{missing}}', {mode: 'formdata'}, 'x'.repeat(1048577)]) {
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

    smol::block_on(server.executor().execute(request)).unwrap();

    let received = server.requests.lock().unwrap();
    assert_eq!(received.len(), 13);
    assert_eq!(
        received
            .iter()
            .filter(|request| request.starts_with("HEAD "))
            .count(),
        4
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
    let result = smol::block_on(server.executor().execute(request)).unwrap();
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
    smol::block_on(server.executor().execute(request)).unwrap();
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
                smol::block_on(server.executor().execute(request)),
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
        .apply(&[("token".into(), Some("initial".into()))].into())
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
    let result =
        smol::block_on(executor.execute_with_variables(request, variables(&session))).unwrap();
    assert!(result.scripts[1].error.is_some());
    assert_eq!(
        session.values(VariableValues::default()).environment["token"],
        "pre"
    );
    let request = server.request(
        "pm.environment.set('token', 'failed'); throw Error('discard');",
        "",
    );
    assert!(smol::block_on(executor.execute_with_variables(request, variables(&session))).is_err());
    assert_eq!(
        session.values(VariableValues::default()).environment["token"],
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
    let error = smol::block_on(server.executor().execute(request)).unwrap_err();
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
    let result = smol::block_on(server.executor().execute(request)).unwrap();
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
    let error = smol::block_on(executor.execute(request)).unwrap_err();
    assert!(error.to_string().contains("limit"), "{error}");
    preferences.timeout_ms = 30;
    let executor = RequestExecutor::new(&preferences).unwrap();
    let request = server.request("await pm.sendRequest('BASE/slow');", "");
    let start = Instant::now();
    assert!(smol::block_on(executor.execute(request)).is_err());
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
    let future = server
        .executor()
        .execute_with_variables(request, variables(&session));
    smol::block_on(smol::future::or(
        async {
            let _ = future.await;
        },
        async {
            smol::Timer::after(Duration::from_millis(40)).await;
        },
    ));
    thread::sleep(Duration::from_millis(100));
    assert!(
        session
            .values(VariableValues::default())
            .environment
            .is_empty()
    );
    assert!(
        !server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.starts_with("GET /main"))
    );
}
