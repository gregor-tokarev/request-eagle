//! gRPC calls against a local server with dynamic messages and reflection.

use std::{
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{Stream, StreamExt as _, stream};
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, Value};
use request::{
    GrpcClient, GrpcDefinition, GrpcError, GrpcEvent, GrpcEvents, GrpcRequest, GrpcScripts,
    GrpcSettings, MethodKind, RequestPreferences, RequestVariables, ServiceDefinition,
};
use tonic::{
    Status, Streaming,
    codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder},
    metadata::MetadataValue,
    server::NamedService,
};

const ECHO_PROTO: &str = r#"
syntax = "proto3";

package echo.v1;

import "common/types.proto";
import "google/protobuf/any.proto";
import "google/protobuf/timestamp.proto";

service EchoService {
  rpc Say(EchoRequest) returns (EchoReply);
  rpc Count(EchoRequest) returns (stream EchoReply);
  rpc Collect(stream EchoRequest) returns (EchoReply);
  rpc Chat(stream EchoRequest) returns (stream EchoReply);
  rpc Fail(EchoRequest) returns (EchoReply);
}

message EchoRequest {
  string text = 1;
  int32 times = 2;
  common.Tag tag = 3;
  google.protobuf.Timestamp at = 4;
  repeated string labels = 5;
  map<string, int64> counts = 6;
  oneof target {
    string user = 7;
    int32 group = 8;
  }
  google.protobuf.Any detail = 9;
}

message EchoReply {
  string text = 1;
  int32 index = 2;
}
"#;

const TYPES_PROTO: &str = r#"
syntax = "proto3";

package common;

message Tag {
  string name = 1;
  Level level = 2;
}

enum Level {
  LOW = 0;
  HIGH = 1;
}
"#;

/// `protos/echo.proto` imports `common/types.proto` from `shared`.
struct Protos {
    _directory: tempfile::TempDir,
    echo: PathBuf,
    shared: PathBuf,
}

fn protos() -> Protos {
    let directory = tempfile::tempdir().unwrap();
    let echo = directory.path().join("protos/echo.proto");
    let shared = directory.path().join("shared");

    fs::create_dir_all(echo.parent().unwrap()).unwrap();
    fs::create_dir_all(shared.join("common")).unwrap();
    fs::write(&echo, ECHO_PROTO).unwrap();
    fs::write(shared.join("common/types.proto"), TYPES_PROTO).unwrap();

    Protos {
        _directory: directory,
        echo,
        shared,
    }
}

fn descriptor_set(protos: &Protos) -> Vec<u8> {
    protox::Compiler::new([protos.shared.as_path(), protos.echo.parent().unwrap()])
        .unwrap()
        .include_imports(true)
        .open_file("echo.proto")
        .unwrap()
        .file_descriptor_set()
        .encode_to_vec()
}

struct TestCodec(MessageDescriptor);

impl Codec for TestCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = TestEncoder;
    type Decoder = TestDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        TestEncoder
    }

    fn decoder(&mut self) -> Self::Decoder {
        TestDecoder(self.0.clone())
    }
}

struct TestEncoder;

impl Encoder for TestEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        item.encode(dst)
            .map_err(|error| Status::internal(error.to_string()))
    }
}

struct TestDecoder(MessageDescriptor);

impl Decoder for TestDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Status> {
        DynamicMessage::decode(self.0.clone(), src)
            .map(Some)
            .map_err(|error| Status::internal(error.to_string()))
    }
}

type Replies = Pin<Box<dyn Stream<Item = Result<DynamicMessage, Status>> + Send>>;

/// Implements every EchoService method with dynamic messages.
#[derive(Clone)]
struct Echo {
    pool: DescriptorPool,
}

impl NamedService for Echo {
    const NAME: &'static str = "echo.v1.EchoService";
}

impl tower::Service<tonic::codegen::http::Request<tonic::body::Body>> for Echo {
    type Response = tonic::codegen::http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: tonic::codegen::http::Request<tonic::body::Body>) -> Self::Future {
        let name = request.uri().path().rsplit('/').next().unwrap().to_owned();
        let method = self
            .pool
            .get_service_by_name(Self::NAME)
            .unwrap()
            .methods()
            .find(|method| method.name() == name)
            .unwrap();
        let output = method.output();
        let handler =
            tower::service_fn(move |request: tonic::Request<Streaming<DynamicMessage>>| {
                respond(name.clone(), output.clone(), request)
            });

        Box::pin(async move {
            let mut grpc = tonic::server::Grpc::new(TestCodec(method.input()));

            Ok(grpc.streaming(handler, request).await)
        })
    }
}

async fn respond(
    method: String,
    output: MessageDescriptor,
    request: tonic::Request<Streaming<DynamicMessage>>,
) -> Result<tonic::Response<Replies>, Status> {
    let echo = request.metadata().get("x-echo").cloned();
    let mut requests = request.into_inner();
    let reply = move |text: String, index: i32| {
        let mut reply = DynamicMessage::new(output.clone());
        reply.set_field_by_name("text", Value::String(text));
        reply.set_field_by_name("index", Value::I32(index));
        Ok(reply)
    };
    let text = |message: &DynamicMessage| {
        message
            .get_field_by_name("text")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    };

    let replies: Replies = match method.as_str() {
        "Say" => {
            let message = requests.message().await?.unwrap();
            Box::pin(stream::iter([reply(
                format!("hello {}", text(&message)),
                0,
            )]))
        }
        "Count" => {
            let message = requests.message().await?.unwrap();
            let times = message
                .get_field_by_name("times")
                .and_then(|value| value.as_i32())
                .unwrap_or(0);
            let text = text(&message);
            Box::pin(stream::iter(
                (0..times)
                    .map(|index| reply(format!("{text} {index}"), index))
                    .collect::<Vec<_>>(),
            ))
        }
        "Collect" => {
            let mut texts = Vec::new();
            while let Some(message) = requests.message().await? {
                texts.push(text(&message));
            }
            Box::pin(stream::iter([reply(texts.join(","), texts.len() as i32)]))
        }
        "Chat" => Box::pin(requests.map(move |message| {
            let message = message?;
            reply(format!("echo {}", text(&message)), 0)
        })),
        "Fail" => {
            let mut status = Status::invalid_argument("text is not allowed");
            status
                .metadata_mut()
                .insert("x-reason", MetadataValue::from_static("test"));
            return Err(status);
        }
        _ => return Err(Status::unimplemented(method)),
    };

    let mut response = tonic::Response::new(replies);
    if let Some(echo) = echo {
        response.metadata_mut().insert("x-echo", echo);
    }

    Ok(response)
}

/// Serve EchoService and reflection. `v1alpha` serves only the older
/// reflection service, like servers built before v1.
async fn serve(protos: &Protos, reflection: Option<&str>) -> SocketAddr {
    let bytes: &'static [u8] = Box::leak(descriptor_set(protos).into_boxed_slice());
    let pool = DescriptorPool::decode(bytes).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut server = tonic::transport::Server::builder();
    let mut router = server.add_service(Echo { pool });
    let builder = || {
        tonic_reflection::server::Builder::configure().register_encoded_file_descriptor_set(bytes)
    };

    match reflection {
        Some("v1") => router = router.add_service(builder().build_v1().unwrap()),
        Some("v1alpha") => router = router.add_service(builder().build_v1alpha().unwrap()),
        _ => {}
    }

    tokio::spawn(
        router.serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
    );

    address
}

fn client() -> GrpcClient {
    GrpcClient::new(&RequestPreferences {
        timeout_ms: 10_000,
        ..RequestPreferences::default()
    })
}

fn variables() -> RequestVariables {
    RequestVariables::new([("name".to_owned(), "eagle".to_owned())].into(), None)
}

fn request(address: SocketAddr, method: &str, message: &str) -> GrpcRequest {
    GrpcRequest {
        url: address.to_string(),
        method: format!("echo.v1.EchoService/{method}"),
        message: message.to_owned(),
        ..GrpcRequest::default()
    }
}

async fn reflect(request: &GrpcRequest) -> ServiceDefinition {
    client()
        .load_definition(request, &variables(), None)
        .await
        .unwrap()
}

/// Collect events until the call ends.
async fn collect(mut events: GrpcEvents) -> Vec<GrpcEvent> {
    let mut all = Vec::new();

    while let Some(event) = tokio::time::timeout(Duration::from_secs(10), events.next())
        .await
        .unwrap()
    {
        let done = matches!(event, GrpcEvent::Finished { .. } | GrpcEvent::Failed(_));
        all.push(event);

        if done {
            break;
        }
    }

    all
}

fn received(events: &[GrpcEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|event| match event {
            GrpcEvent::Received(message) => Some(serde_json::from_str(&message.json).unwrap()),
            _ => None,
        })
        .collect()
}

fn status(events: &[GrpcEvent]) -> (i32, String) {
    match events.last().unwrap() {
        GrpcEvent::Finished { status, .. } => (status.code, status.name().to_owned()),
        event => panic!("expected a status, got {event:?}"),
    }
}

#[tokio::test]
async fn reflection_lists_services_and_method_kinds() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let definition = reflect(&request(address, "Say", "")).await;
    let services = definition.services();

    assert_eq!(services.len(), 1);
    assert_eq!(services[0].name, "echo.v1.EchoService");
    assert_eq!(
        services[0]
            .methods
            .iter()
            .map(|method| (method.name.as_str(), method.kind))
            .collect::<Vec<_>>(),
        [
            ("Say", MethodKind::Unary),
            ("Count", MethodKind::ServerStreaming),
            ("Collect", MethodKind::ClientStreaming),
            ("Chat", MethodKind::BidiStreaming),
            ("Fail", MethodKind::Unary),
        ]
    );
}

#[tokio::test]
async fn reflection_falls_back_to_v1alpha() {
    let protos = protos();
    let address = serve(&protos, Some("v1alpha")).await;
    let definition = reflect(&request(address, "Say", "")).await;

    assert!(definition.method("echo.v1.EchoService/Chat").is_some());
}

#[tokio::test]
async fn servers_without_reflection_suggest_a_proto_file() {
    let protos = protos();
    let address = serve(&protos, None).await;
    let error = client()
        .load_definition(&request(address, "Say", ""), &variables(), None)
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("Import a .proto file"),
        "{error}"
    );
}

#[tokio::test]
async fn unary_calls_return_the_message_metadata_and_status() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(address, "Say", r#"{"text": "{{name}}"}"#);
    request.metadata = vec![("X-Echo".into(), "{{name}}".into())];
    let definition = reflect(&request).await;

    let (call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    assert_eq!(call.kind, MethodKind::Unary);
    let events = collect(events).await;

    assert_eq!(
        received(&events),
        [serde_json::json!({"text": "hello eagle", "index": 0})]
    );
    assert_eq!(status(&events), (0, "OK".into()));
    assert!(events.iter().any(|event| matches!(event,
        GrpcEvent::Metadata(metadata) if metadata.contains(&("x-echo".into(), "eagle".into())))));
}

#[tokio::test]
async fn server_streams_arrive_in_order() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let request = request(address, "Count", r#"{"text": "tick", "times": 3}"#);
    let definition = reflect(&request).await;

    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert_eq!(
        received(&events)
            .iter()
            .map(|message| message["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>(),
        ["tick 0", "tick 1", "tick 2"]
    );
    assert_eq!(status(&events), (0, "OK".into()));
}

#[tokio::test]
async fn client_streams_send_each_message_until_ended() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let request = request(address, "Collect", "");
    let definition = reflect(&request).await;

    let (mut call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    call.send(r#"{"text": "a"}"#).unwrap();
    call.send(r#"{"text": "{{name}}"}"#).unwrap();
    assert!(matches!(
        call.send(r#"{"unknown": 1}"#),
        Err(GrpcError::InvalidMessage(_))
    ));
    call.end();
    assert!(matches!(call.send("{}"), Err(GrpcError::StreamEnded)));
    let events = collect(events).await;

    let sent = events
        .iter()
        .filter(|event| matches!(event, GrpcEvent::Sent(_)))
        .count();
    assert_eq!(sent, 2);
    assert_eq!(
        received(&events),
        [serde_json::json!({"text": "a,eagle", "index": 2})]
    );
}

#[tokio::test]
async fn bidirectional_streams_reply_while_open() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let request = request(address, "Chat", "");
    let definition = reflect(&request).await;

    let (mut call, mut events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    call.send(r#"{"text": "one"}"#).unwrap();

    // The reply arrives before the client ends its stream.
    loop {
        match tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .unwrap()
            .unwrap()
        {
            GrpcEvent::Received(message) => {
                assert!(message.json.contains("echo one"));
                break;
            }
            GrpcEvent::Finished { .. } | GrpcEvent::Failed(_) => panic!("the call ended"),
            _ => {}
        }
    }

    call.send(r#"{"text": "two"}"#).unwrap();
    call.end();
    let events = collect(events).await;

    assert_eq!(
        received(&events),
        [serde_json::json!({"text": "echo two", "index": 0})]
    );
    assert_eq!(status(&events), (0, "OK".into()));
}

#[tokio::test]
async fn cancelling_a_stream_stops_the_call() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let request = request(address, "Chat", "");
    let definition = reflect(&request).await;

    let (call, mut events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    drop(call);

    // Aborting the call closes its event channel without a status.
    let next = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = events.next().await {
            if matches!(event, GrpcEvent::Finished { .. }) {
                return Some(event);
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(next.is_none());
}

#[tokio::test]
async fn error_statuses_complete_with_their_trailers() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let request = request(address, "Fail", "{}");
    let definition = reflect(&request).await;

    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    match events.last().unwrap() {
        GrpcEvent::Finished {
            status, trailers, ..
        } => {
            assert_eq!(status.code, 3);
            assert_eq!(status.name(), "INVALID_ARGUMENT");
            assert_eq!(status.message, "text is not allowed");
            assert!(trailers.contains(&("x-reason".into(), "test".into())));
        }
        event => panic!("expected a status, got {event:?}"),
    }
}

#[tokio::test]
async fn invalid_messages_are_rejected_before_connecting() {
    let request = GrpcRequest {
        url: "127.0.0.1:1".into(),
        method: "echo.v1.EchoService/Say".into(),
        message: r#"{"text": 5}"#.into(),
        ..GrpcRequest::default()
    };
    let protos = protos();
    let definition =
        ServiceDefinition::from_proto_file(&protos.echo, std::slice::from_ref(&protos.shared))
            .unwrap();

    let error = client()
        .invoke(&request, variables(), &definition)
        .await
        .err()
        .unwrap();
    assert!(matches!(error, GrpcError::InvalidMessage(_)), "{error}");

    let error = client()
        .invoke(
            &GrpcRequest {
                method: "echo.v1.EchoService/Missing".into(),
                ..request
            },
            variables(),
            &definition,
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(error, GrpcError::UnknownMethod(_)), "{error}");
}

#[tokio::test]
async fn unreachable_servers_fail_without_a_status() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let protos = protos();
    let definition =
        ServiceDefinition::from_proto_file(&protos.echo, std::slice::from_ref(&protos.shared))
            .unwrap();

    let (_call, events) = client()
        .invoke(&request(address, "Say", "{}"), variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert!(matches!(
        events.last(),
        Some(GrpcEvent::Failed(GrpcError::Connect(_)))
    ));
}

#[tokio::test]
async fn proto_files_resolve_imports_from_import_paths() {
    let protos = protos();

    let error = ServiceDefinition::from_proto_file(&protos.echo, &[]).unwrap_err();
    assert!(error.to_string().contains("common/types.proto"), "{error}");

    let definition =
        ServiceDefinition::from_proto_file(&protos.echo, std::slice::from_ref(&protos.shared))
            .unwrap();
    assert_eq!(definition.services()[0].methods.len(), 5);

    // Calls work with a local definition and a server without reflection.
    let address = serve(&protos, None).await;
    let request = request(address, "Say", r#"{"text": "proto"}"#);
    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();

    assert_eq!(
        received(&collect(events).await),
        [serde_json::json!({"text": "hello proto", "index": 0})]
    );
}

#[tokio::test]
async fn relative_proto_paths_resolve_from_the_collection() {
    let protos = protos();
    let collection = protos.echo.parent().unwrap().parent().unwrap();
    let request = GrpcRequest {
        definition: GrpcDefinition::ProtoFile {
            path: PathBuf::from("protos/echo.proto"),
            import_paths: vec![PathBuf::from("shared")],
        },
        ..GrpcRequest::default()
    };

    let definition = client()
        .load_definition(&request, &variables(), Some(collection))
        .await
        .unwrap();
    assert!(definition.method("echo.v1.EchoService/Say").is_some());

    let error = client()
        .load_definition(&request, &variables(), None)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("relative to a collection"),
        "{error}"
    );
}

#[tokio::test]
async fn example_messages_fill_every_field() {
    let protos = protos();
    let definition =
        ServiceDefinition::from_proto_file(&protos.echo, std::slice::from_ref(&protos.shared))
            .unwrap();
    let example: serde_json::Value = serde_json::from_str(
        &definition
            .example_message("echo.v1.EchoService/Say")
            .unwrap(),
    )
    .unwrap();

    assert_eq!(example["tag"]["level"], "LOW");
    assert!(example["at"].as_str().unwrap().ends_with('Z'));
    assert!(example["labels"].is_array());
    assert!(example["counts"].is_object());
    // Only the first field of a oneof is filled in.
    assert!(example.get("user").is_some());
    assert!(example.get("group").is_none());
    // An Any names a type that the definition knows.
    assert!(example["detail"]["@type"].is_string());

    // The example is a valid message for the method.
    let request = GrpcRequest {
        url: "127.0.0.1:1".into(),
        method: "echo.v1.EchoService/Say".into(),
        message: example.to_string(),
        ..GrpcRequest::default()
    };
    let (call, _) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    drop(call);
}

#[test]
fn saved_requests_round_trip_through_toml() {
    let request = request::Request::Grpc(GrpcRequest {
        url: "grpcs://example.com".into(),
        tls: true,
        method: "echo.v1.EchoService/Say".into(),
        message: "{\"text\": \"hi\"}".into(),
        metadata: vec![("authorization".into(), "Bearer {{token}}".into())],
        definition: GrpcDefinition::ProtoFile {
            path: Path::new("protos/echo.proto").into(),
            import_paths: vec!["shared".into()],
        },
        settings: GrpcSettings {
            include_default_fields: false,
            ..GrpcSettings::default()
        },
        scripts: GrpcScripts {
            on_message: "console.log(pm.message.data);".into(),
            ..GrpcScripts::default()
        },
    });

    #[derive(serde::Serialize, serde::Deserialize)]
    struct File {
        request: request::Request,
    }

    let text = toml::to_string_pretty(&File { request }).unwrap();
    assert!(text.contains("type = \"grpc\""), "{text}");
    assert!(text.contains("source = \"proto_file\""), "{text}");
    // Only the scripts that were written are saved.
    assert!(text.contains("on_message = "), "{text}");
    assert!(!text.contains("before_invoke"), "{text}");

    let File { request } = toml::from_str(&text).unwrap();
    let request::Request::Grpc(request) = request else {
        panic!("expected a gRPC request");
    };
    assert_eq!(request.url, "grpcs://example.com");
    assert_eq!(request.metadata.len(), 1);
    assert!(!request.settings.include_default_fields);
    assert_eq!(request.scripts.on_message, "console.log(pm.message.data);");

    // Reflection is the default and is not written.
    let File { request } =
        toml::from_str("[request]\ntype = \"grpc\"\nurl = \"localhost:50051\"\n").unwrap();
    let request::Request::Grpc(request) = request else {
        panic!("expected a gRPC request");
    };
    assert_eq!(request.definition, GrpcDefinition::Reflection);
    assert!(request.settings.is_default());
    assert!(request.scripts.is_empty());
    assert!(!request.uses_tls());
}

#[tokio::test]
async fn default_fields_can_be_left_out_of_messages() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(address, "Count", r#"{"text": "tick", "times": 1}"#);
    let definition = reflect(&request).await;

    // The first reply has index 0, a default value.
    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    assert_eq!(
        received(&collect(events).await),
        [serde_json::json!({"text": "tick 0", "index": 0})]
    );

    request.settings.include_default_fields = false;
    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    assert_eq!(
        received(&collect(events).await),
        [serde_json::json!({"text": "tick 0"})]
    );
}

#[tokio::test]
async fn response_messages_over_the_limit_end_the_call() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(
        address,
        "Say",
        &format!(r#"{{"text": "{}"}}"#, "x".repeat(2 * 1024 * 1024)),
    );
    request.settings.max_response_message_mb = Some(1);
    let definition = reflect(&request).await;

    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert_eq!(status(&events).1, "OUT_OF_RANGE");
}

/// Postman's public echo service: plaintext on port 443 with v1alpha
/// reflection. Run with `cargo test -p request --test grpc -- --ignored`.
#[tokio::test]
#[ignore = "uses the network"]
async fn postman_echo_service() {
    let request = GrpcRequest {
        url: "grpc.postman-echo.com".into(),
        method: "HelloService/SayHello".into(),
        message: r#"{"greeting": "eagle"}"#.into(),
        ..GrpcRequest::default()
    };
    let definition = reflect(&request).await;
    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert_eq!(status(&events), (0, "OK".into()));
    assert_eq!(
        received(&events),
        [serde_json::json!({"reply": "hello eagle"})]
    );

    let request = GrpcRequest {
        method: "HelloService/LotsOfReplies".into(),
        ..request
    };
    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert_eq!(status(&events), (0, "OK".into()));
    assert!(received(&events).len() > 1);
}

/// grpcb.in serves reflection over TLS on port 9001.
#[tokio::test]
#[ignore = "uses the network"]
async fn public_tls_service() {
    let request = GrpcRequest {
        url: "grpcs://grpcb.in:9001".into(),
        method: "hello.HelloService/SayHello".into(),
        message: r#"{"greeting": "{{name}}"}"#.into(),
        ..GrpcRequest::default()
    };
    let definition = reflect(&request).await;
    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert_eq!(status(&events), (0, "OK".into()));
    assert_eq!(
        received(&events),
        [serde_json::json!({"reply": "hello eagle"})]
    );

    // A certificate for another name fails unless verification is off.
    let mut request = request;
    request.settings.server_name = "example.com".into();
    let error = client()
        .load_definition(&request, &variables(), None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("certificate"), "{error}");

    request.settings.verify_certificates = Some(false);
    reflect(&request).await;
}

#[tokio::test]
async fn generated_values_match_across_metadata_and_message() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(address, "Say", r#"{"text": "{{$guid}}"}"#);
    request.metadata = vec![("x-echo".into(), "{{$guid}}".into())];
    let definition = reflect(&request).await;

    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    let echoed = events
        .iter()
        .find_map(|event| match event {
            GrpcEvent::Metadata(metadata) => metadata
                .iter()
                .find(|(name, _)| name == "x-echo")
                .map(|(_, value)| value.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(received(&events)[0]["text"], format!("hello {echoed}"));
}

#[test]
fn import_paths_keep_their_order() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let vendor = directory.path().join("vendor");

    fs::create_dir_all(root.join("api")).unwrap();
    fs::create_dir_all(root.join("common")).unwrap();
    fs::create_dir_all(vendor.join("common")).unwrap();
    fs::write(
        root.join("api/root.proto"),
        "syntax = \"proto3\";\npackage api;\nimport \"common/types.proto\";\nservice Api { rpc Get(common.Tag) returns (common.Tag); }\n",
    )
    .unwrap();
    fs::write(
        root.join("common/types.proto"),
        "syntax = \"proto3\";\npackage common;\nmessage Tag { string project = 1; }\n",
    )
    .unwrap();
    fs::write(
        vendor.join("common/types.proto"),
        "syntax = \"proto3\";\npackage common;\nmessage Tag { string vendor = 1; }\n",
    )
    .unwrap();

    // The first import path that has an import wins, as with protoc.
    let definition =
        ServiceDefinition::from_proto_file(&root.join("api/root.proto"), &[vendor, root]).unwrap();
    let example = definition.example_message("api.Api/Get").unwrap();
    assert!(example.contains("vendor"), "{example}");
}

#[test]
fn definitions_store_paths_inside_the_collection_relative_to_it() {
    let definition = GrpcDefinition::ProtoFile {
        path: "/collections/Demo/protos/root.proto".into(),
        import_paths: vec!["/collections/Demo/shared".into(), "/usr/include".into()],
    };

    assert_eq!(
        definition.relative_to(Path::new("/collections/Demo")),
        GrpcDefinition::ProtoFile {
            path: "protos/root.proto".into(),
            import_paths: vec!["shared".into(), "/usr/include".into()],
        }
    );
}

#[test]
fn target_keys_change_with_named_variables_only() {
    let request = GrpcRequest {
        url: "{{host}}:50051".into(),
        metadata: vec![("x-request-id".into(), "{{$guid}}".into())],
        ..GrpcRequest::default()
    };
    let variables =
        |host: &str| RequestVariables::new([("host".to_owned(), host.to_owned())].into(), None);

    // A generated value differs on every use but names the same server.
    assert_eq!(
        variables("one").grpc_target_key(&request),
        variables("one").grpc_target_key(&request)
    );
    assert_ne!(
        variables("one").grpc_target_key(&request),
        variables("two").grpc_target_key(&request)
    );
}

#[tokio::test]
async fn streams_open_before_their_message_variables_are_set() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let request = request(address, "Chat", r#"{"text": "{{next_message}}"}"#);
    let definition = reflect(&request).await;

    let (mut call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    assert!(matches!(
        call.send(&request.message),
        Err(GrpcError::Variables(_))
    ));
    call.end();

    assert_eq!(status(&collect(events).await), (0, "OK".into()));
}

/// Each script report's label, whether its tests passed, and its error.
fn script_results(events: &[GrpcEvent]) -> Vec<(String, bool, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match event {
            GrpcEvent::Script(report) => Some((
                report.label(),
                !report.tests.is_empty() && report.tests.iter().all(|test| test.error.is_none()),
                report.error.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn scripts_run_before_invoke_on_each_message_and_after_the_response() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(address, "Count", r#"{"text": "tick", "times": 5}"#);
    request.scripts = GrpcScripts {
        before_invoke: r#"
            pm.request.metadata.add({key: "x-echo", value: "{{name}}"});
            pm.request.message = {text: "tick", times: 2};
            pm.test("runs first", () => {});
        "#
        .into(),
        on_message: r#"
            pm.test("tick", () => pm.expect(pm.message.data.text).to.match(/^tick \d$/));
        "#
        .into(),
        after_response: r#"
            pm.test("echoed and counted", () => {
                pm.response.to.be.ok;
                pm.response.to.have.metadata("x-echo", "eagle");
                pm.expect(pm.response.messages.count()).to.equal(2);
                pm.request.messages.to.include({text: "tick", times: 2});
            });
        "#
        .into(),
    };
    let definition = reflect(&request).await;

    let (_call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    let events = collect(events).await;

    assert!(matches!(events.first(), Some(GrpcEvent::Script(_))));
    assert_eq!(
        script_results(&events),
        [
            ("Before invoke".into(), true, None),
            ("On message 1".into(), true, None),
            ("On message 2".into(), true, None),
            ("After response".into(), true, None),
        ]
    );
    assert_eq!(status(&events), (0, "OK".into()));
}

#[tokio::test]
async fn after_response_sees_the_messages_sent_on_a_client_stream() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(address, "Collect", "");
    request.scripts.after_response = r#"
        pm.test("sent and collected", () => {
            pm.expect(pm.request.messages.map(message => message.data.text)).to.eql(["a", "eagle"]);
            pm.response.messages.to.include({text: "a,eagle", index: 2});
        });
    "#
    .into();
    let definition = reflect(&request).await;

    let (mut call, events) = client()
        .invoke(&request, variables(), &definition)
        .await
        .unwrap();
    call.send(r#"{"text": "a"}"#).unwrap();
    call.send(r#"{"text": "{{name}}"}"#).unwrap();
    call.end();
    let events = collect(events).await;

    assert_eq!(
        script_results(&events),
        [("After response".into(), true, None)]
    );
}

#[tokio::test]
async fn failing_before_invoke_scripts_stop_the_call() {
    let protos = protos();
    let address = serve(&protos, Some("v1")).await;
    let mut request = request(address, "Say", "{}");
    request.scripts.before_invoke = "throw new Error('no token');".into();
    let definition = reflect(&request).await;

    let error = client()
        .invoke(&request, variables(), &definition)
        .await
        .err()
        .unwrap();

    assert!(
        matches!(&error, GrpcError::Script { report, .. } if report.error.is_some()),
        "{error}"
    );
    assert!(error.to_string().contains("no token"), "{error}");
}
