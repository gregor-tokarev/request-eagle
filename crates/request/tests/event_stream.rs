use std::{
    collections::HashMap,
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use flate2::{
    Compression,
    write::{GzEncoder, ZlibEncoder},
};
use futures::StreamExt as _;
use request::{
    EventStream, EventStreamUpdate, EventStreamUpdates, ExecutionError, HttpRequest, HttpVersion,
    RequestExecutor, RequestPreferences, RequestVariables, Response, ServerSentEvent, StatusCode,
};
use smol::{
    channel::{Receiver, Sender},
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const STREAM_HEAD: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nConnection: close\r\n";

fn executor(preferences: RequestPreferences) -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences {
        http_version: HttpVersion::Http1_1,
        ..preferences
    })
    .unwrap()
}

fn get(url: &str) -> HttpRequest {
    HttpRequest {
        path: url.to_owned(),
        ..HttpRequest::default()
    }
}

fn no_variables() -> RequestVariables {
    RequestVariables::new(HashMap::new(), None)
}

/// Accept one request and pass its connection to `respond`.
async fn serve<F>(respond: impl FnOnce(TcpStream) -> F + Send + 'static) -> (String, smol::Task<()>)
where
    F: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();

        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }

        respond(stream).await;
    });

    (url, server)
}

/// Whether the client closed the connection within two seconds.
async fn closes(mut stream: TcpStream) -> bool {
    smol::future::or(
        async {
            let mut byte = [0];
            stream.read(&mut byte).await.unwrap() == 0
        },
        async {
            smol::Timer::after(Duration::from_secs(2)).await;
            false
        },
    )
    .await
}

async fn next_event(updates: &mut EventStreamUpdates) -> ServerSentEvent {
    match updates.next().await {
        Some(EventStreamUpdate::Event(event)) => event,
        update => panic!("expected an event, got {update:?}"),
    }
}

async fn opened(updates: &mut EventStreamUpdates) {
    match updates.next().await {
        Some(EventStreamUpdate::Opened {
            status, headers, ..
        }) => {
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers["content-type"], "text/event-stream; charset=utf-8");
        }
        update => panic!("expected the stream to open, got {update:?}"),
    }
}

/// The server writes the rest of its stream once the test received the first event.
fn next_part() -> (Sender<()>, Receiver<()>) {
    smol::channel::bounded(1)
}

#[test]
fn reports_events_as_they_arrive_and_completes_with_the_whole_body() {
    smol::block_on(async {
        let (received, receiving) = next_part();
        let (url, server) = serve(|mut stream| async move {
            stream
                .write_all(format!("{STREAM_HEAD}\r\n: hello\n\ndata: one\n\n").as_bytes())
                .await
                .unwrap();
            receiving.recv().await.unwrap();
            stream
                .write_all(b"event: update\nid: 2\ndata: {\"n\":2}\n\n")
                .await
                .unwrap();
        })
        .await;
        let (events, mut updates, _stop) = EventStream::new();
        let run = smol::spawn(executor(RequestPreferences::default()).execute_streaming(
            get(&url),
            no_variables(),
            events,
        ));

        opened(&mut updates).await;
        let first = next_event(&mut updates).await;
        assert_eq!(
            (first.event.as_str(), first.data.as_str()),
            ("message", "one")
        );
        received.send(()).await.unwrap();

        let second = next_event(&mut updates).await;
        assert_eq!(second.event, "update");
        assert_eq!(second.data, "{\"n\":2}");
        assert_eq!(second.id, "2");
        assert!(second.time >= first.time);
        assert!(updates.next().await.is_none());

        let execution = run.await.unwrap();
        let Response::Http(response) = execution.response;
        assert_eq!(
            response.body,
            b": hello\n\ndata: one\n\nevent: update\nid: 2\ndata: {\"n\":2}\n\n"
        );
        assert_eq!(response.metrics.encoded_response_body_bytes, None);
        server.await;
    });
}

#[test]
fn stopping_completes_the_response_with_what_arrived() {
    smol::block_on(async {
        let (closed, closing) = smol::channel::bounded(1);
        let (url, server) = serve(|mut stream| async move {
            stream
                .write_all(format!("{STREAM_HEAD}\r\ndata: one\n\n").as_bytes())
                .await
                .unwrap();
            closed.send(closes(stream).await).await.unwrap();
        })
        .await;
        let mut request = get(&url);
        request.scripts.post_response =
            "pm.test('body', () => pm.expect(pm.response.text()).to.equal('data: one\\n\\n'));"
                .into();
        let (events, mut updates, stop) = EventStream::new();
        let run = smol::spawn(executor(RequestPreferences::default()).execute_streaming(
            request,
            no_variables(),
            events,
        ));

        opened(&mut updates).await;
        assert_eq!(next_event(&mut updates).await.data, "one");
        stop.stop();
        assert!(updates.next().await.is_none());

        let execution = run.await.unwrap();
        let Response::Http(response) = &execution.response;
        assert_eq!(response.body, b"data: one\n\n");
        assert_eq!(execution.scripts.len(), 1);
        assert!(execution.scripts[0].error.is_none());
        assert!(execution.scripts[0].tests[0].error.is_none());
        assert!(
            closing.recv().await.unwrap(),
            "stopping must close the connection"
        );
        server.await;
    });
}

#[test]
fn the_request_timeout_lasts_until_the_stream_opens() {
    smol::block_on(async {
        let preferences = RequestPreferences {
            timeout_ms: 150,
            ..RequestPreferences::default()
        };

        let (url, server) = serve(|mut stream| async move {
            stream
                .write_all(format!("{STREAM_HEAD}\r\n").as_bytes())
                .await
                .unwrap();
            smol::Timer::after(Duration::from_millis(400)).await;
            stream.write_all(b"data: late\n\n").await.unwrap();
        })
        .await;
        let (events, mut updates, _stop) = EventStream::new();
        let run = smol::spawn(executor(preferences.clone()).execute_streaming(
            get(&url),
            no_variables(),
            events,
        ));
        opened(&mut updates).await;
        assert_eq!(next_event(&mut updates).await.data, "late");
        run.await.unwrap();
        server.await;

        let (url, server) = serve(|mut stream| async move {
            smol::Timer::after(Duration::from_millis(400)).await;
            let _ = stream
                .write_all(format!("{STREAM_HEAD}\r\ndata: late\n\n").as_bytes())
                .await;
        })
        .await;
        let (events, mut updates, _stop) = EventStream::new();
        let error = executor(preferences)
            .execute_streaming(get(&url), no_variables(), events)
            .await
            .unwrap_err()
            .error;
        assert!(matches!(error, ExecutionError::Timeout { .. }), "{error}");
        assert!(updates.next().await.is_none());
        server.await;
    });
}

#[test]
fn gzip_streams_decode_as_they_arrive() {
    smol::block_on(async {
        let (received, receiving) = next_part();
        let (url, server) = serve(|mut stream| async move {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            stream
                .write_all(format!("{STREAM_HEAD}Content-Encoding: gzip\r\n\r\n").as_bytes())
                .await
                .unwrap();

            encoder.write_all(b"data: one\n\n").unwrap();
            encoder.flush().unwrap();
            stream
                .write_all(&std::mem::take(encoder.get_mut()))
                .await
                .unwrap();
            receiving.recv().await.unwrap();

            encoder.write_all(b"data: two\n\n").unwrap();
            stream.write_all(&encoder.finish().unwrap()).await.unwrap();
        })
        .await;
        let (events, mut updates, _stop) = EventStream::new();
        let run = smol::spawn(executor(RequestPreferences::default()).execute_streaming(
            get(&url),
            no_variables(),
            events,
        ));

        opened(&mut updates).await;
        assert_eq!(next_event(&mut updates).await.data, "one");
        received.send(()).await.unwrap();
        assert_eq!(next_event(&mut updates).await.data, "two");

        let Response::Http(response) = run.await.unwrap().response;
        assert_eq!(response.body, b"data: one\n\ndata: two\n\n");
        assert!(response.metrics.encoded_response_body_bytes.is_some());
        server.await;
    });
}

/// Collects what an encoder writes, for the server to send after each flush.
#[derive(Clone, Default)]
struct Encoded(Arc<Mutex<Vec<u8>>>);

impl Encoded {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

impl Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn deflate_brotli_and_zstd_streams_decode_as_they_arrive() {
    for coding in ["deflate", "br", "zstd"] {
        smol::block_on(async {
            let (received, receiving) = next_part();
            let (url, server) = serve(move |mut stream| async move {
                let encoded = Encoded::default();
                // Dropping each encoder ends its stream.
                let mut encoder: Box<dyn Write + Send> = match coding {
                    "deflate" => {
                        Box::new(ZlibEncoder::new(encoded.clone(), Compression::default()))
                    }
                    "br" => Box::new(brotli::CompressorWriter::new(encoded.clone(), 4096, 5, 22)),
                    _ => Box::new(
                        zstd::stream::write::Encoder::new(encoded.clone(), 3)
                            .unwrap()
                            .auto_finish(),
                    ),
                };
                stream
                    .write_all(
                        format!("{STREAM_HEAD}Content-Encoding: {coding}\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();

                encoder.write_all(b"data: one\n\n").unwrap();
                encoder.flush().unwrap();
                stream.write_all(&encoded.take()).await.unwrap();
                receiving.recv().await.unwrap();

                encoder.write_all(b"data: two\n\n").unwrap();
                drop(encoder);
                stream.write_all(&encoded.take()).await.unwrap();
            })
            .await;
            let (events, mut updates, _stop) = EventStream::new();
            let run = smol::spawn(executor(RequestPreferences::default()).execute_streaming(
                get(&url),
                no_variables(),
                events,
            ));

            opened(&mut updates).await;
            assert_eq!(next_event(&mut updates).await.data, "one", "{coding}");
            received.send(()).await.unwrap();
            assert_eq!(next_event(&mut updates).await.data, "two", "{coding}");

            let Response::Http(response) = run.await.unwrap().response;
            assert_eq!(response.body, b"data: one\n\ndata: two\n\n");
            assert!(response.metrics.encoded_response_body_bytes.is_some());
            server.await;
        });
    }
}

#[test]
fn other_responses_and_plain_execution_read_the_whole_body() {
    smol::block_on(async {
        let (url, server) = serve(|mut stream| async move {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}")
                .await
                .unwrap();
        })
        .await;
        let (events, mut updates, _stop) = EventStream::new();
        let execution = executor(RequestPreferences::default())
            .execute_streaming(get(&url), no_variables(), events)
            .await
            .unwrap();
        let Response::Http(response) = execution.response;
        assert_eq!(response.body, b"{}");
        assert!(updates.next().await.is_none());
        server.await;

        let (url, server) = serve(|mut stream| async move {
            stream
                .write_all(format!("{STREAM_HEAD}\r\ndata: one\n\n").as_bytes())
                .await
                .unwrap();
        })
        .await;
        let execution = executor(RequestPreferences::default())
            .execute(get(&url), no_variables())
            .await
            .unwrap();
        let Response::Http(response) = execution.response;
        assert_eq!(response.body, b"data: one\n\n");
        server.await;
    });
}

#[test]
fn streams_stay_within_the_response_size_limit() {
    smol::block_on(async {
        let (url, server) = serve(|mut stream| async move {
            let _ = stream
                .write_all(format!("{STREAM_HEAD}\r\n").as_bytes())
                .await;
            let event = format!("data: {}\n\n", "x".repeat(64 * 1024));
            for _ in 0..20 {
                if stream.write_all(event.as_bytes()).await.is_err() {
                    break;
                }
            }
        })
        .await;
        let (events, updates, _stop) = EventStream::new();
        // Read the updates, so only the limit ends the stream.
        let reader = smol::spawn(updates.count());
        let error = executor(RequestPreferences {
            max_response_size_mb: 1,
            ..RequestPreferences::default()
        })
        .execute_streaming(get(&url), no_variables(), events)
        .await
        .unwrap_err()
        .error;

        assert!(
            matches!(error, ExecutionError::ResponseTooLarge { limit_bytes } if limit_bytes == 1024 * 1024),
            "{error}"
        );
        assert!(reader.await > 1);
        server.await;
    });
}
