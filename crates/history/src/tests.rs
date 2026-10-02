use std::{
    fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use request::{
    Body, Execution, Field, GrpcRequest, HeaderMap, HttpMetrics, HttpRequest, HttpResponse, Method,
    StatusCode, Version, WebSocketRequest,
};

use crate::{BODY_LIMIT, History, HistoryError, LIMIT, Record, Response};

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn get(path: &str) -> Record {
    Record::sent(HttpRequest {
        path: path.into(),
        ..Default::default()
    })
}

fn execution(body: Vec<u8>) -> Execution {
    let mut headers = HeaderMap::new();
    headers.append("content-type", "application/json".parse().unwrap());
    headers.append("set-cookie", "a=1".parse().unwrap());
    headers.append("set-cookie", "b=2".parse().unwrap());

    Execution {
        response: request::Response::Http(HttpResponse {
            status: StatusCode::CREATED,
            version: Version::HTTP_2,
            headers,
            body,
            metrics: HttpMetrics {
                waiting: Duration::from_millis(40),
                response_header_bytes: 120,
                encoded_response_body_bytes: Some(9),
                ..Default::default()
            },
        }),
        elapsed: Duration::from_millis(52),
        scripts: Vec::new(),
    }
}

fn addresses(history: &History) -> Vec<&str> {
    history
        .entries()
        .iter()
        .map(|entry| entry.address.as_str())
        .collect()
}

#[test]
fn a_missing_history_is_empty() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path().join("history"));

    history.load().unwrap();

    assert!(history.entries().is_empty());
}

#[test]
fn keeps_requests_newest_first_across_launches() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());

    history.add(get("https://a.test"), at(10)).save().unwrap();
    history.add(get("{{host}}/b"), at(30)).save().unwrap();
    // A slow request completes after one that was sent later.
    history.add(get("https://c.test"), at(20)).save().unwrap();

    let mut reopened = History::new(directory.path());
    reopened.load().unwrap();

    assert_eq!(
        addresses(&reopened),
        ["{{host}}/b", "https://c.test", "https://a.test"]
    );
    assert_eq!(reopened.entries()[0].label, "GET");
    assert_eq!(reopened.entries()[0].sent_at(), at(30));
}

#[test]
fn reads_a_request_with_its_response() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());
    let request = HttpRequest {
        method: Method::Post,
        path: "{{host}}/users".into(),
        headers: vec![Field::new("Authorization", "Bearer {{token}}")],
        body: Some(Body::json(r#"{"name":"Ada"}"#)),
        ..Default::default()
    };
    let record = Record {
        response: Some(Response::new(&execution(br#"{"id":1}"#.to_vec()))),
        ..Record::sent(request.clone())
    };

    history.add(record, at(1)).save().unwrap();
    let id = history.entries()[0].id.clone();
    let read = history.files(&id).unwrap().read().unwrap();

    let request::Request::Http(read_request) = read.request else {
        panic!("expected an HTTP request");
    };
    assert_eq!(read_request, request);

    let execution = read.response.unwrap().into_execution();
    let request::Response::Http(response) = execution.response;
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.version, Version::HTTP_2);
    assert_eq!(response.body, br#"{"id":1}"#);
    assert_eq!(
        response
            .headers
            .get_all("set-cookie")
            .iter()
            .collect::<Vec<_>>(),
        ["a=1", "b=2"]
    );
    assert_eq!(response.metrics.waiting, Duration::from_millis(40));
    assert_eq!(response.metrics.encoded_response_body_bytes, Some(9));
    assert_eq!(execution.elapsed, Duration::from_millis(52));
}

#[test]
fn leaves_out_bodies_over_the_limit() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());
    let response = Response::new(&execution(vec![b'a'; BODY_LIMIT + 1]));
    let record = Record {
        response: Some(response),
        ..get("https://large.test")
    };

    history.add(record, at(1)).save().unwrap();
    let read = history
        .files(&history.entries()[0].id)
        .unwrap()
        .read()
        .unwrap();
    let response = read.response.unwrap();

    assert_eq!(response.body, None);
    assert_eq!(response.body_size, BODY_LIMIT + 1);
}

#[test]
fn keeps_why_a_sent_request_failed() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());
    let record = Record {
        error: Some("connection refused".into()),
        ..get("http://localhost:1")
    };

    history.add(record, at(1)).save().unwrap();
    let read = history
        .files(&history.entries()[0].id)
        .unwrap()
        .read()
        .unwrap();

    assert!(read.response.is_none());
    assert_eq!(read.error.as_deref(), Some("connection refused"));
}

#[test]
fn names_grpc_calls_by_server_and_method() {
    let grpc = Record::sent(GrpcRequest {
        url: "grpcb.in:9000".into(),
        method: "hello.HelloService/SayHello".into(),
        ..Default::default()
    });
    let websocket = Record::sent(WebSocketRequest {
        url: " wss://echo.test ".into(),
        ..Default::default()
    });

    assert_eq!(grpc.label(), "gRPC");
    assert_eq!(grpc.address(), "grpcb.in:9000/hello.HelloService/SayHello");
    assert_eq!(websocket.label(), "WS");
    assert_eq!(websocket.address(), "wss://echo.test");
}

#[test]
fn deletes_the_oldest_beyond_the_limit() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());

    for second in 0..LIMIT as u64 {
        history
            .add(get(&format!("/{second}")), at(second))
            .save()
            .unwrap();
    }
    let oldest = history.entries().last().unwrap().id.clone();
    history
        .add(get("/newest"), at(LIMIT as u64))
        .save()
        .unwrap();

    assert_eq!(history.entries().len(), LIMIT);
    assert_eq!(history.entries()[0].address, "/newest");
    assert_eq!(history.entries().last().unwrap().address, "/1");
    assert!(
        !directory
            .path()
            .join(format!("entries/{oldest}.json"))
            .exists()
    );
}

#[test]
fn deletes_one_request_or_all_of_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());
    let record = Record {
        response: Some(Response::new(&execution(b"{}".to_vec()))),
        ..get("/a")
    };

    history.add(record, at(1)).save().unwrap();
    history.add(get("/b"), at(2)).save().unwrap();
    history.add(get("/c"), at(3)).save().unwrap();
    let deleted = history.entries()[2].id.clone();

    history.delete(&deleted).save().unwrap();

    assert_eq!(addresses(&history), ["/c", "/b"]);
    assert!(history.files(&deleted).is_none());
    assert!(
        !directory
            .path()
            .join(format!("entries/{deleted}.body"))
            .exists()
    );

    history.clear().save().unwrap();
    let mut reopened = History::new(directory.path());
    reopened.load().unwrap();

    assert!(history.entries().is_empty());
    assert!(reopened.entries().is_empty());
    assert!(!directory.path().join("entries").exists());
}

#[test]
fn reads_only_listed_entries() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());
    history.add(get("/a"), at(1)).save().unwrap();
    fs::write(directory.path().join("secret.json"), "{}").unwrap();

    assert!(history.files("../secret").is_none());

    // A listed request whose files are gone can no longer be read.
    let id = history.entries()[0].id.clone();
    fs::remove_dir_all(directory.path().join("entries")).unwrap();

    assert!(matches!(
        history.files(&id).unwrap().read(),
        Err(HistoryError::Missing)
    ));
}

#[test]
fn lists_a_request_at_once_and_writes_it_when_saved() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = History::new(directory.path());

    let change = history.add(get("/a"), at(1));
    let mut reopened = History::new(directory.path());
    reopened.load().unwrap();

    assert_eq!(addresses(&history), ["/a"]);
    assert!(reopened.entries().is_empty());

    change.save().unwrap();
    reopened.load().unwrap();

    assert_eq!(addresses(&reopened), ["/a"]);
}

#[test]
fn an_unreadable_list_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("index.json"), "<<<<<<< HEAD").unwrap();
    let mut history = History::new(directory.path());

    assert!(matches!(history.load(), Err(HistoryError::Json(_))));
    assert!(history.entries().is_empty());
}
