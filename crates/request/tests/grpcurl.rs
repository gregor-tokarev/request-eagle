//! grpcurl snippets run by grpcurl itself. A local server must receive from
//! each command the metadata and messages it receives when Request Eagle
//! makes the same call, and answer both alike. Run with
//! `cargo test -p request --test grpcurl -- --ignored` with grpcurl on the
//! `PATH`.

#![cfg(unix)]

use std::{
    collections::HashMap,
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::PathBuf,
    pin::Pin,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use futures::{Stream, StreamExt as _, stream};
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use request::{
    ApiKeyAuth, Auth, AuthLocation, BearerAuth, Field, GrpcClient, GrpcDefinition, GrpcEvent,
    GrpcRequest, GrpcSettings, JwtAuth, OAuth2Auth, PasswordAuth, RequestPreferences,
    RequestVariables,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::{TlsAcceptor, rustls, server::TlsStream};
use tonic::{
    Status, Streaming,
    codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder},
    server::NamedService,
    transport::server::Connected,
};

const RECORDER_PROTO: &str = r#"
syntax = "proto3";

package eagle.v1;

import "common/kinds.proto";
import "google/protobuf/timestamp.proto";

service Recorder {
  rpc Unary(Everything) returns (Everything);
  rpc ServerStream(Everything) returns (stream Everything);
  rpc ClientStream(stream Everything) returns (Everything);
  rpc Bidi(stream Everything) returns (stream Everything);
}

message Everything {
  string text = 1;
  int32 small = 2;
  int64 big = 3;
  uint64 unsigned = 4;
  double ratio = 5;
  bool flag = 6;
  bytes data = 7;
  common.Kind kind = 8;
  repeated string labels = 9;
  map<string, int64> counts = 10;
  Nested nested = 11;
  google.protobuf.Timestamp at = 12;
  oneof choice {
    string name = 13;
    int32 number = 14;
  }
  optional string maybe = 15;
}

message Nested {
  string value = 1;
}
"#;

const KINDS_PROTO: &str = r#"
syntax = "proto3";

package common;

enum Kind {
  KIND_UNSPECIFIED = 0;
  KIND_EAGLE = 1;
}
"#;

/// A message with a value in each field.
const FULL_MESSAGE: &str = r#"{
  "text": "{{name}}'s \"nest\"",
  "small": -7,
  "big": "9007199254740993",
  "unsigned": 18446744073709551615,
  "ratio": 0.25,
  "flag": true,
  "data": "aGVsbG8=",
  "kind": "KIND_EAGLE",
  "labels": ["a", "b c"],
  "counts": {"x": "1"},
  "nested": {"value": "ünï"},
  "at": "2026-10-10T12:00:00Z",
  "number": 3,
  "maybe": ""
}"#;

/// `protos/eagle/v1/recorder.proto` imports `common/kinds.proto` from `shared`.
struct Protos {
    _directory: tempfile::TempDir,
    recorder: PathBuf,
    shared: PathBuf,
}

fn protos() -> Protos {
    let directory = tempfile::tempdir().unwrap();
    let recorder = directory.path().join("protos/eagle/v1/recorder.proto");
    let shared = directory.path().join("shared");

    fs::create_dir_all(recorder.parent().unwrap()).unwrap();
    fs::create_dir_all(shared.join("common")).unwrap();
    fs::write(&recorder, RECORDER_PROTO).unwrap();
    fs::write(shared.join("common/kinds.proto"), KINDS_PROTO).unwrap();

    Protos {
        _directory: directory,
        recorder,
        shared,
    }
}

fn descriptor_set(protos: &Protos) -> Vec<u8> {
    protox::Compiler::new([protos.shared.as_path(), protos.recorder.parent().unwrap()])
        .unwrap()
        .include_imports(true)
        .open_file("recorder.proto")
        .unwrap()
        .file_descriptor_set()
        .encode_to_vec()
}

struct DynamicCodec(MessageDescriptor);

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicEncoder
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicDecoder(self.0.clone())
    }
}

struct DynamicEncoder;

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        item.encode(dst)
            .map_err(|error| Status::internal(error.to_string()))
    }
}

struct DynamicDecoder(MessageDescriptor);

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Status> {
        DynamicMessage::decode(self.0.clone(), src)
            .map(Some)
            .map_err(|error| Status::internal(error.to_string()))
    }
}

/// A call as the server received it.
#[derive(Debug, PartialEq)]
struct Call {
    method: String,
    /// The metadata the call's caller chose, in lowercase and by name, as
    /// grpcurl sends it in no particular order. Binary values are decoded.
    metadata: Vec<(String, String)>,
    messages: Vec<serde_json::Value>,
}

type Calls = Arc<Mutex<Vec<Call>>>;

type Replies = Pin<Box<dyn Stream<Item = Result<DynamicMessage, Status>> + Send>>;

/// Records each call, then answers with the first message it received, twice
/// on a stream.
#[derive(Clone)]
struct Recorder {
    pool: DescriptorPool,
    calls: Calls,
}

impl NamedService for Recorder {
    const NAME: &'static str = "eagle.v1.Recorder";
}

impl tower::Service<tonic::codegen::http::Request<tonic::body::Body>> for Recorder {
    type Response = tonic::codegen::http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: tonic::codegen::http::Request<tonic::body::Body>) -> Self::Future {
        let name = request.uri().path().rsplit('/').next().unwrap().to_owned();
        let input = self
            .pool
            .get_message_by_name("eagle.v1.Everything")
            .unwrap();
        let calls = self.calls.clone();
        let handler =
            tower::service_fn(move |request: tonic::Request<Streaming<DynamicMessage>>| {
                record(name.clone(), calls.clone(), request)
            });

        Box::pin(async move {
            let mut grpc = tonic::server::Grpc::new(DynamicCodec(input));

            Ok(grpc.streaming(handler, request).await)
        })
    }
}

/// Metadata that the transport adds itself.
const TRANSPORT_METADATA: [&str; 5] = [
    "content-type",
    "te",
    "user-agent",
    "grpc-accept-encoding",
    "grpc-timeout",
];

async fn record(
    method: String,
    calls: Calls,
    request: tonic::Request<Streaming<DynamicMessage>>,
) -> Result<tonic::Response<Replies>, Status> {
    let mut metadata: Vec<_> = request
        .metadata()
        .clone()
        .into_headers()
        .iter()
        .filter(|(name, _)| !TRANSPORT_METADATA.contains(&name.as_str()))
        .map(|(name, value)| {
            let value = if name.as_str().ends_with("-bin") {
                // Either client may leave out the padding.
                let encoded = value.to_str().unwrap().trim_end_matches('=');
                String::from_utf8_lossy(&STANDARD_NO_PAD.decode(encoded).unwrap()).into_owned()
            } else {
                value.to_str().unwrap().to_owned()
            };
            (name.as_str().to_owned(), value)
        })
        .collect();
    // Values of the same name keep their order.
    metadata.sort_by(|(first, _), (second, _)| first.cmp(second));

    let mut requests = request.into_inner();
    let mut messages = Vec::new();
    while let Some(message) = requests.message().await? {
        messages.push(message);
    }

    let reply = messages.first().cloned();
    calls.lock().unwrap().push(Call {
        method: method.clone(),
        metadata,
        messages: messages
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .collect(),
    });

    let reply = reply.ok_or_else(|| Status::invalid_argument("no message"))?;
    let replies = match method.as_str() {
        "ServerStream" | "Bidi" => vec![Ok(reply.clone()), Ok(reply)],
        _ => vec![Ok(reply)],
    };

    Ok(tonic::Response::new(Box::pin(stream::iter(replies))))
}

/// A TLS connection the server accepted.
struct Tls(TlsStream<tokio::net::TcpStream>);

impl Connected for Tls {
    type ConnectInfo = ();

    fn connect_info(&self) {}
}

impl AsyncRead for Tls {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Tls {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

struct Server {
    address: SocketAddr,
    calls: Calls,
}

impl Server {
    /// Serves the recorder and, unless `reflection` is None, the reflection
    /// service of that version. With `tls`, over TLS with a certificate that
    /// no one trusts.
    async fn start(protos: &Protos, reflection: Option<&str>, tls: bool) -> Self {
        let bytes: &'static [u8] = Box::leak(descriptor_set(protos).into_boxed_slice());
        let pool = DescriptorPool::decode(bytes).unwrap();
        let calls = Calls::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let mut server = tonic::transport::Server::builder();
        let mut router = server.add_service(Recorder {
            pool,
            calls: calls.clone(),
        });
        let builder = || {
            tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(bytes)
        };
        match reflection {
            Some("v1") => router = router.add_service(builder().build_v1().unwrap()),
            Some("v1alpha") => router = router.add_service(builder().build_v1alpha().unwrap()),
            _ => {}
        }

        let connections = tokio_stream::wrappers::TcpListenerStream::new(listener);
        if tls {
            let acceptor = acceptor();
            let connections = connections.filter_map(move |stream| {
                let acceptor = acceptor.clone();
                async move {
                    let stream = acceptor.accept(stream.ok()?).await.ok()?;
                    Some(Ok::<_, std::io::Error>(Tls(stream)))
                }
            });
            tokio::spawn(router.serve_with_incoming(connections));
        } else {
            tokio::spawn(router.serve_with_incoming(connections));
        }

        Self { address, calls }
    }

    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut self.calls.lock().unwrap())
    }
}

fn acceptor() -> TlsAcceptor {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert.der().clone()], signing_key.into())
    .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];

    TlsAcceptor::from(Arc::new(config))
}

/// What the server received and answered when Request Eagle made the call,
/// and when its grpcurl command ran.
struct Made {
    command: String,
    by_request_eagle: Vec<Call>,
    by_grpcurl: Vec<Call>,
    replies: Vec<serde_json::Value>,
    printed: Vec<serde_json::Value>,
    grpcurl: Output,
}

async fn make(
    server: &Server,
    request: &GrpcRequest,
    values: &[(&str, &str)],
    preferences: &RequestPreferences,
) -> Made {
    let values: HashMap<String, String> = values
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let variables = || RequestVariables::new(values.clone(), None);
    let command = request.grpcurl_command(&values, None, preferences);

    let client = GrpcClient::new(preferences);
    let definition = client
        .load_definition(request, &variables(), None)
        .await
        .unwrap();
    let (mut call, mut events) = client
        .invoke(request, variables(), &definition)
        .await
        .unwrap();
    // Invoking opens a stream of messages, which the editor's message starts.
    if call.kind.streams_requests() {
        call.send(&request.message).unwrap();
        call.end();
    }

    let mut replies = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .unwrap()
            .unwrap();
        match event {
            GrpcEvent::Received(message) => {
                replies.push(serde_json::from_str(&message.json).unwrap());
            }
            GrpcEvent::Finished { status, .. } => {
                assert_eq!(status.code, 0, "{status:?}");
                break;
            }
            GrpcEvent::Failed(error) => panic!("{error}"),
            _ => {}
        }
    }
    let by_request_eagle = server.take();

    let grpcurl = run(&command).await;
    let by_grpcurl = server.take();
    let printed = serde_json::Deserializer::from_slice(&grpcurl.stdout)
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap_or_default();

    Made {
        command,
        by_request_eagle,
        by_grpcurl,
        replies,
        printed,
        grpcurl,
    }
}

/// Runs a command in a shell without proxies.
async fn run(command: &str) -> Output {
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command).stdin(Stdio::null());
    for proxy in [
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
        "grpc_proxy",
    ] {
        shell.env_remove(proxy);
    }

    tokio::task::spawn_blocking(move || shell.output().unwrap())
        .await
        .unwrap()
}

fn assert_same(made: &Made) {
    assert!(
        made.grpcurl.status.success(),
        "{}\n{}",
        made.command,
        String::from_utf8_lossy(&made.grpcurl.stderr)
    );
    assert!(!made.by_request_eagle.is_empty(), "{}", made.command);
    assert_eq!(made.by_request_eagle, made.by_grpcurl, "{}", made.command);
    assert_eq!(
        made.replies.iter().map(shown).collect::<Vec<_>>(),
        made.printed.iter().map(shown).collect::<Vec<_>>(),
        "{}",
        made.command
    );
}

/// A reply as it reads. grpcurl shows the messages a reply leaves unset as
/// `null` with its default fields, and writes whole numbers without a
/// fraction.
fn shown(reply: &serde_json::Value) -> serde_json::Value {
    match reply {
        serde_json::Value::Object(fields) => fields
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(name, value)| (name.clone(), shown(value)))
            .collect(),
        serde_json::Value::Array(values) => values.iter().map(shown).collect(),
        serde_json::Value::Number(number) => number.as_f64().unwrap().into(),
        value => value.clone(),
    }
}

fn request(server: &Server, method: &str, message: &str) -> GrpcRequest {
    GrpcRequest {
        url: server.address.to_string(),
        method: format!("eagle.v1.Recorder/{method}"),
        message: message.into(),
        ..GrpcRequest::default()
    }
}

#[tokio::test]
#[ignore = "needs grpcurl"]
async fn every_method_kind_sends_the_same_messages() {
    let protos = protos();
    let server = Server::start(&protos, Some("v1"), false).await;

    for method in ["Unary", "ServerStream", "ClientStream", "Bidi"] {
        for message in [FULL_MESSAGE, r#"{"text": "only"}"#] {
            let made = make(
                &server,
                &request(&server, method, message),
                &[("name", "Rex")],
                &RequestPreferences::default(),
            )
            .await;
            assert_same(&made);
        }
    }

    // Without a message, an empty one is sent.
    let made = make(
        &server,
        &request(&server, "Unary", ""),
        &[],
        &RequestPreferences::default(),
    )
    .await;
    assert_same(&made);
}

#[tokio::test]
#[ignore = "needs grpcurl"]
async fn addresses_and_methods_are_written_as_invoking_reads_them() {
    let protos = protos();
    let server = Server::start(&protos, Some("v1"), false).await;
    let address = server.address;

    for (url, method) in [
        (format!("grpc://{address}"), "eagle.v1.Recorder/Unary"),
        (format!("http://{address}/"), "/eagle.v1.Recorder/Unary"),
        ("{{host}}".to_owned(), "eagle.v1.Recorder/Unary"),
    ] {
        let request = GrpcRequest {
            url,
            method: method.into(),
            ..request(&server, "Unary", r#"{"text": "x"}"#)
        };
        let made = make(
            &server,
            &request,
            &[("host", &address.to_string())],
            &RequestPreferences::default(),
        )
        .await;
        assert_same(&made);
    }
}

#[tokio::test]
#[ignore = "needs grpcurl"]
async fn metadata_and_authorization_arrive_the_same() {
    let protos = protos();
    let server = Server::start(&protos, Some("v1"), false).await;
    let metadata = vec![
        Field::new("X-Custom", "{{name}}'s value"),
        Field::new("x-empty", ""),
        Field::new("x-twice", "1"),
        Field::new("x-twice", "2"),
        Field::new("x-data-bin", "aGVsbG8="),
        Field {
            enabled: false,
            ..Field::new("x-off", "1")
        },
    ];
    let auths = [
        Auth::None,
        Auth::Basic(PasswordAuth {
            username: "{{name}}".into(),
            password: "p@ss:wörd".into(),
        }),
        Auth::Bearer(BearerAuth {
            token: "{{token}}".into(),
        }),
        Auth::ApiKey(ApiKeyAuth {
            key: "X-Api-Key".into(),
            value: "{{token}}".into(),
            add_to: AuthLocation::Header,
        }),
        // A call has no query, so the key is metadata too.
        Auth::ApiKey(ApiKeyAuth {
            key: "api-key".into(),
            value: "{{token}}".into(),
            add_to: AuthLocation::Query,
        }),
        Auth::OAuth2(Box::new(OAuth2Auth {
            access_token: "{{token}}".into(),
            ..OAuth2Auth::default()
        })),
        Auth::Jwt(Box::new(JwtAuth {
            secret: "{{token}}".into(),
            payload: r#"{"sub": "{{name}}"}"#.into(),
            ..JwtAuth::default()
        })),
    ];

    for auth in auths {
        let request = GrpcRequest {
            metadata: metadata.clone(),
            auth,
            ..request(&server, "Unary", r#"{"text": "x"}"#)
        };
        let made = make(
            &server,
            &request,
            &[("name", "Rex"), ("token", "s3cr3t+/=")],
            &RequestPreferences::default(),
        )
        .await;
        assert_same(&made);
    }
}

#[tokio::test]
#[ignore = "needs grpcurl"]
async fn proto_files_and_reflection_versions_find_the_same_method() {
    let protos = protos();
    let server = Server::start(&protos, None, false).await;
    let from_file = GrpcRequest {
        definition: GrpcDefinition::ProtoFile {
            path: protos.recorder.clone(),
            import_paths: vec![protos.shared.clone()],
        },
        ..request(&server, "Unary", FULL_MESSAGE)
    };
    let made = make(
        &server,
        &from_file,
        &[("name", "Rex")],
        &RequestPreferences::default(),
    )
    .await;
    assert_same(&made);

    let server = Server::start(&protos, Some("v1alpha"), false).await;
    let made = make(
        &server,
        &request(&server, "Unary", FULL_MESSAGE),
        &[("name", "Rex")],
        &RequestPreferences::default(),
    )
    .await;
    assert_same(&made);
}

#[tokio::test]
#[ignore = "needs grpcurl"]
async fn responses_show_default_fields_as_the_settings_say() {
    let protos = protos();
    let server = Server::start(&protos, Some("v1"), false).await;

    for include_default_fields in [true, false] {
        let request = GrpcRequest {
            settings: GrpcSettings {
                include_default_fields,
                ..GrpcSettings::default()
            },
            ..request(&server, "Unary", r#"{"text": "", "small": 0, "maybe": ""}"#)
        };
        let made = make(&server, &request, &[], &RequestPreferences::default()).await;
        assert_same(&made);
    }
}

#[tokio::test]
#[ignore = "needs grpcurl"]
async fn certificate_checks_follow_the_settings_and_preferences() {
    let protos = protos();
    let server = Server::start(&protos, Some("v1"), true).await;
    let request = GrpcRequest {
        tls: true,
        ..request(&server, "Unary", r#"{"text": "x"}"#)
    };

    let unchecked = RequestPreferences {
        ssl_certificate_verification: false,
        ..RequestPreferences::default()
    };
    let made = make(&server, &request, &[], &unchecked).await;
    assert_same(&made);

    let request = GrpcRequest {
        url: format!("grpcs://{}", server.address),
        settings: GrpcSettings {
            verify_certificates: Some(false),
            ..GrpcSettings::default()
        },
        ..request
    };
    let made = make(&server, &request, &[], &RequestPreferences::default()).await;
    assert_same(&made);

    // Nobody trusts the server's certificate.
    let request = GrpcRequest {
        settings: GrpcSettings::default(),
        ..request
    };
    let command = request.grpcurl_command(&HashMap::new(), None, &RequestPreferences::default());
    assert!(!run(&command).await.status.success(), "{command}");
}
