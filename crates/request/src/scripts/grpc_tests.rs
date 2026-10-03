use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use environment::EnvironmentSession;
use futures::{StreamExt as _, channel::mpsc::unbounded};

use super::{CallScripts, GrpcScripts, ScriptPhase, ScriptReport};
use crate::{
    Field, GrpcError, GrpcEvent, GrpcFailure, GrpcMessage, GrpcRequest, GrpcStatus,
    RequestExecutor, RequestPreferences, RequestVariables,
};

fn variables() -> RequestVariables {
    RequestVariables::with_environment_session(
        HashMap::from([("name".into(), "eagle".into())]),
        HashMap::new(),
        None,
        EnvironmentSession::default(),
    )
}

fn call_scripts(scripts: GrpcScripts, variables: &RequestVariables) -> CallScripts {
    let executor = RequestExecutor::new(&RequestPreferences::default()).unwrap();

    CallScripts::new(scripts, variables, executor)
}

fn request() -> GrpcRequest {
    GrpcRequest {
        url: "localhost:50051".into(),
        method: "echo.v1.EchoService/Say".into(),
        message: r#"{"text": "hi"}"#.into(),
        metadata: vec![Field::new("authorization", "Bearer t")],
        ..GrpcRequest::default()
    }
}

fn message(json: &str) -> GrpcMessage {
    GrpcMessage {
        json: json.into(),
        at: SystemTime::now(),
    }
}

fn finished(code: i32, message: &str, trailers: &[(&str, &str)]) -> GrpcEvent {
    GrpcEvent::Finished {
        status: GrpcStatus {
            code,
            message: message.into(),
        },
        trailers: trailers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        elapsed: Duration::from_millis(12),
    }
}

/// Pass a call's events through its scripts and collect what comes out.
fn follow(scripts: GrpcScripts, events: Vec<GrpcEvent>) -> Vec<GrpcEvent> {
    let (sender, receiver) = unbounded();
    for event in events {
        sender.unbounded_send(event).unwrap();
    }
    drop(sender);

    let (output, forwarded) = unbounded();
    smol::block_on(call_scripts(scripts, &variables()).forward(request(), receiver, output));

    smol::block_on(forwarded.collect())
}

fn reports(events: &[GrpcEvent]) -> Vec<&ScriptReport> {
    events
        .iter()
        .filter_map(|event| match event {
            GrpcEvent::Script(report) => Some(report),
            _ => None,
        })
        .collect()
}

fn failures(report: &ScriptReport) -> Vec<(&str, &str)> {
    report
        .tests
        .iter()
        .filter_map(|test| Some((test.name.as_str(), test.error.as_deref()?)))
        .collect()
}

#[test]
fn before_invoke_changes_the_call_and_the_variables_it_resolves_with() {
    let mut variables = variables();
    let session = variables.session.clone().unwrap();
    let mut scripts = call_scripts(
        GrpcScripts {
            before_invoke: r#"
                pm.request.url = "grpc://" + pm.request.url;
                pm.request.metadata.upsert({key: "x-id", value: pm.variables.replaceIn("{{$guid}}")});
                pm.request.metadata.remove("Authorization");
                pm.request.message = {text: "{{greeting}}", id: "{{$guid}}"};
                pm.variables.set("greeting", "hi " + pm.variables.get("name"));
                pm.environment.set("token", "secret");
                console.log(pm.request.methodPath);
            "#
            .into(),
            ..GrpcScripts::default()
        },
        &variables,
    );
    let mut request = request();

    let report = smol::block_on(scripts.before_invoke(&mut request, &mut variables))
        .unwrap()
        .unwrap();

    assert_eq!(report.phase, ScriptPhase::BeforeInvoke);
    assert_eq!(report.logs[0].message, "echo.v1.EchoService/Say");
    assert_eq!(request.url, "grpc://localhost:50051");
    assert_eq!(request.metadata.len(), 1);

    // The generated value is the same in the script, metadata and message.
    let (resolved, _) = variables.resolve_grpc(&request, true).unwrap();
    let id = &resolved.metadata[0].value;
    let message: serde_json::Value = serde_json::from_str(&resolved.message).unwrap();
    assert_eq!(message, serde_json::json!({"text": "hi eagle", "id": id}));
    assert_eq!(id.len(), 36);
    assert_eq!(
        session
            .values(HashMap::new(), HashMap::new())
            .get("token")
            .map(String::as_str),
        Some("secret")
    );
}

#[test]
fn before_invoke_errors_and_skips_stop_the_call() {
    let run = |source: &str| {
        let mut variables = variables();
        let mut scripts = call_scripts(
            GrpcScripts {
                before_invoke: source.into(),
                ..GrpcScripts::default()
            },
            &variables,
        );

        smol::block_on(scripts.before_invoke(&mut request(), &mut variables))
    };

    match run("console.log('checking'); throw new Error('no token');") {
        Err(GrpcFailure {
            error: GrpcError::Script { message },
            scripts,
        }) => {
            assert!(message.contains("no token"), "{message}");
            assert_eq!(scripts[0].logs[0].message, "checking");
        }
        result => panic!("expected a script error, got {result:?}"),
    }

    match run("pm.execution.skipRequest('offline');") {
        Err(GrpcFailure {
            error: GrpcError::Skipped { reason },
            ..
        }) => assert_eq!(reason, "offline"),
        result => panic!("expected a skip, got {result:?}"),
    }

    // Only Before invoke can skip the call.
    let events = follow(
        GrpcScripts {
            on_message:
                "pm.test('cannot skip', () => pm.expect(pm.execution.skipRequest).to.be.undefined);"
                    .into(),
            ..GrpcScripts::default()
        },
        vec![GrpcEvent::Received(message("{}")), finished(0, "", &[])],
    );
    assert!(failures(reports(&events)[0]).is_empty());
}

#[test]
fn on_message_runs_after_each_received_message_and_shares_variables() {
    let events = follow(
        GrpcScripts {
            on_message: r#"
                const seen = Number(pm.variables.get("seen") ?? 0) + 1;
                pm.variables.set("seen", seen);
                pm.test("message " + seen + " is in order", () => {
                    pm.expect(pm.message.data.index).to.equal(seen - 1);
                    pm.expect(pm.message.timestamp).to.be.a("date");
                    pm.expect(pm.response).to.be.undefined;
                });
            "#
            .into(),
            after_response: r#"
                pm.test("every message ran", () => pm.expect(pm.variables.get("seen")).to.equal("3"));
            "#
            .into(),
            ..GrpcScripts::default()
        },
        vec![
            GrpcEvent::Metadata(Vec::new()),
            GrpcEvent::Received(message(r#"{"index": 0}"#)),
            GrpcEvent::Received(message(r#"{"index": 1}"#)),
            GrpcEvent::Received(message(r#"{"index": 2}"#)),
            finished(0, "", &[]),
        ],
    );

    let order = events
        .iter()
        .map(|event| match event {
            GrpcEvent::Metadata(_) => "metadata".to_owned(),
            GrpcEvent::Received(_) => "received".to_owned(),
            GrpcEvent::Script(report) => report.label(),
            GrpcEvent::Finished { .. } => "finished".to_owned(),
            event => panic!("unexpected {event:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            "metadata",
            "received",
            "On message 1",
            "received",
            "On message 2",
            "received",
            "On message 3",
            "After response",
            "finished",
        ]
    );

    for report in reports(&events) {
        assert_eq!(report.tests.len(), 1, "{report:?}");
        assert!(failures(report).is_empty(), "{report:?}");
    }
}

#[test]
fn after_response_tests_the_status_metadata_trailers_and_messages() {
    let events = follow(
        GrpcScripts {
            after_response: r#"
                pm.test("status", () => {
                    pm.response.to.have.statusCode(3);
                    pm.response.to.have.status("INVALID_ARGUMENT");
                    pm.response.to.have.status(3);
                    pm.response.to.be.error;
                    pm.expect(pm.response.statusMessage).to.equal("text is not allowed");
                    pm.expect(pm.response.responseTime).to.equal(12);
                });
                pm.test("metadata and trailers", () => {
                    pm.response.to.have.metadata("X-Echo", "eagle");
                    pm.response.to.have.trailer("x-reason");
                    pm.expect(pm.response.trailers.get("x-reason")).to.equal("test");
                });
                pm.test("messages", () => {
                    pm.response.to.have.message({text: "hello b", index: 1});
                    pm.response.messages.to.include({text: "hello a"});
                    pm.response.messages.to.not.include({text: "missing"});
                    pm.response.messages.to.have.property("text");
                    pm.response.messages.to.have.jsonSchema({type: "object", required: ["text", "index"]});
                    pm.expect(pm.response.messages.count()).to.equal(2);
                    pm.expect(pm.response.messages.idx(-1).data.index).to.equal(1);
                    pm.expect(pm.response.messages.filter({data: {index: 1}})).to.have.lengthOf(1);
                    pm.expect(pm.response.messages.map(message => message.data.text)).to.eql(["hello a", "hello b"]);
                });
                pm.test("request", () => {
                    pm.expect(pm.request.methodPath).to.equal("echo.v1.EchoService/Say");
                    pm.expect(pm.request.metadata.get("Authorization")).to.equal("Bearer t");
                    pm.expect(pm.request.messages.idx(0).data).to.eql({text: "a"});
                });
                pm.test("is ok", () => pm.response.to.be.ok);
                pm.test("equals a partial message", () => pm.response.to.have.message({text: "hello a"}));
                pm.test("all have a missing property", () => pm.response.messages.to.have.property("missing"));
                pm.test("sent messages match a schema", () => pm.request.messages.to.have.jsonSchema({required: ["id"]}));
                console.log(pm.response);
            "#
            .into(),
            ..GrpcScripts::default()
        },
        vec![
            GrpcEvent::Metadata(vec![("x-echo".into(), "eagle".into())]),
            GrpcEvent::Sent(message(r#"{"text": "a"}"#)),
            GrpcEvent::Received(message(r#"{"text": "hello a", "index": 0}"#)),
            GrpcEvent::Received(message(r#"{"text": "hello b", "index": 1}"#)),
            finished(3, "text is not allowed", &[("x-reason", "test")]),
        ],
    );

    let report = reports(&events)[0];
    assert_eq!(report.error, None);
    assert_eq!(report.tests.len(), 8);
    let failures = failures(report);
    assert_eq!(
        failures.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        [
            "is ok",
            "equals a partial message",
            "all have a missing property",
            "sent messages match a schema",
        ],
        "{failures:?}"
    );
    assert!(failures[0].1.contains("INVALID_ARGUMENT"), "{failures:?}");
    assert!(failures[1].1.contains("hello a"), "{failures:?}");
    assert!(failures[2].1.contains("message 1"), "{failures:?}");
    assert!(failures[3].1.contains("message 1"), "{failures:?}");

    // Logging the response shows its data without running its assertions.
    assert!(
        report.logs[0].message.contains(r#""code":3"#),
        "{:?}",
        report.logs
    );
    assert!(matches!(events.last(), Some(GrpcEvent::Finished { .. })));
}

#[test]
fn calls_without_a_status_skip_after_response() {
    let events = follow(
        GrpcScripts {
            after_response: "pm.test('ran', () => {});".into(),
            ..GrpcScripts::default()
        },
        vec![
            GrpcEvent::Received(message("{}")),
            GrpcEvent::Failed(GrpcError::Connect("refused".into())),
        ],
    );

    assert!(matches!(
        events.as_slice(),
        [GrpcEvent::Received(_), GrpcEvent::Failed(_)]
    ));
}

#[test]
fn on_message_results_are_bounded_per_call() {
    let events = follow(
        GrpcScripts {
            on_message: "for (let i = 0; i < 300; i++) pm.test('check ' + i, () => {});".into(),
            ..GrpcScripts::default()
        },
        vec![
            GrpcEvent::Received(message("{}")),
            GrpcEvent::Received(message("{}")),
            GrpcEvent::Received(message("{}")),
            finished(0, "", &[]),
        ],
    );

    // The third run still happens, but has nothing left to show.
    let reports = reports(&events);
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports
            .iter()
            .map(|report| report.tests.len())
            .sum::<usize>(),
        500
    );
    assert!(
        reports[1]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("500 tests")),
        "{:?}",
        reports[1].error
    );
}

#[test]
fn after_response_keeps_the_latest_messages_within_its_limit() {
    let large = |text: &str| {
        message(&format!(
            r#"{{"text": "{}"}}"#,
            text.repeat(3 * 1024 * 1024)
        ))
    };
    let events = follow(
        GrpcScripts {
            after_response: r#"
                const texts = pm.response.messages.map(message => message.data.text[0]);
                pm.test("latest messages", () => pm.expect(texts).to.eql(["c", "d"]));
            "#
            .into(),
            ..GrpcScripts::default()
        },
        vec![
            GrpcEvent::Received(large("a")),
            GrpcEvent::Received(large("b")),
            GrpcEvent::Received(large("c")),
            GrpcEvent::Received(large("d")),
            finished(0, "", &[]),
        ],
    );

    let report = reports(&events)[0];
    assert_eq!(report.error, None);
    assert!(failures(report).is_empty(), "{:?}", report.tests);
}
