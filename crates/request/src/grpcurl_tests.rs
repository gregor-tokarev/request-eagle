use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{GrpcDefinition, GrpcRequest, GrpcSettings};

fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn unary(url: &str) -> GrpcRequest {
    GrpcRequest {
        url: url.into(),
        method: "helloworld.Greeter/SayHello".into(),
        settings: GrpcSettings {
            include_default_fields: false,
            ..GrpcSettings::default()
        },
        ..GrpcRequest::default()
    }
}

#[test]
fn writes_calls_with_metadata_and_a_message() {
    let request = GrpcRequest {
        url: "grpc://{{host}}:9000".into(),
        metadata: vec![
            ("Authorization".into(), "Bearer {{token}}".into()),
            ("x-empty".into(), String::new()),
            (" ".into(), "unnamed".into()),
        ],
        message: "{\n  \"name\": \"{{name}}'s\"\n}".into(),
        ..unary("")
    };

    assert_eq!(
        request.grpcurl_command(
            &values(&[("host", "localhost"), ("token", "abc"), ("name", "Rex")]),
            None
        ),
        "grpcurl -plaintext \\\n\
         -H 'Authorization: Bearer abc' \\\n\
         -H x-empty: \\\n\
         -d '{\n  \"name\": \"Rex'\\''s\"\n}' \\\n\
         localhost:9000 helloworld.Greeter/SayHello"
    );
}

#[test]
fn connects_as_invoking_does() {
    let command = |request: GrpcRequest| request.grpcurl_command(&HashMap::new(), None);

    // Without a port, invoking connects to 443 over TLS or plaintext.
    assert_eq!(
        command(unary("grpcs://example.com/")),
        "grpcurl example.com:443 helloworld.Greeter/SayHello"
    );
    assert_eq!(
        command(unary("example.com:50051")),
        "grpcurl -plaintext example.com:50051 helloworld.Greeter/SayHello"
    );
    assert_eq!(
        command(GrpcRequest {
            tls: true,
            ..unary("[::1]:50051")
        }),
        "grpcurl '[::1]:50051' helloworld.Greeter/SayHello"
    );
    // Invoking finds a method with a leading slash too.
    assert_eq!(
        command(GrpcRequest {
            method: "/helloworld.Greeter/SayHello".into(),
            ..unary("localhost:50051")
        }),
        "grpcurl -plaintext localhost:50051 helloworld.Greeter/SayHello"
    );
    // An address cannot become an option, such as one that writes a file.
    assert_eq!(
        command(GrpcRequest {
            method: String::new(),
            ..unary("-protoset-out=/tmp/x")
        }),
        "grpcurl -plaintext -- -protoset-out=/tmp/x list"
    );
    // An empty message is sent as an empty message; without a method, the
    // command lists the services.
    assert_eq!(
        command(GrpcRequest {
            method: String::new(),
            message: "{}".into(),
            ..unary("grpc://localhost:50051")
        }),
        "grpcurl -plaintext localhost:50051 list"
    );
}

#[test]
fn keeps_unknown_and_generated_variables() {
    let request = GrpcRequest {
        metadata: vec![("x-request-id".into(), "{{$guid}}".into())],
        message: r#"{"id": "{{id}}", "literal": "{{!name}}"}"#.into(),
        ..unary("grpcs://{{host}}/")
    };

    assert_eq!(
        request.grpcurl_command(&values(&[("$guid", "fixed")]), None),
        "grpcurl \\\n\
         -H 'x-request-id: {{$guid}}' \\\n\
         -d '{\"id\": \"{{id}}\", \"literal\": \"{{name}}\"}' \\\n\
         '{{host}}' helloworld.Greeter/SayHello"
    );

    // An unfinished reference leaves the text as written.
    let request = GrpcRequest {
        message: r#"{"id": "{{id"}"#.into(),
        ..unary("localhost:1")
    };
    assert!(
        request
            .grpcurl_command(&values(&[("id", "7")]), None)
            .contains(r#"-d '{"id": "{{id"}'"#)
    );
}

#[test]
fn applies_the_request_settings() {
    let command = |settings: GrpcSettings| {
        GrpcRequest {
            settings,
            ..unary("grpcs://example.com:443")
        }
        .grpcurl_command(&HashMap::new(), None)
    };

    assert_eq!(
        command(GrpcSettings {
            server_name: " api.internal ".into(),
            max_response_message_mb: Some(8),
            ..GrpcSettings::default()
        }),
        "grpcurl -emit-defaults \\\n\
         -servername api.internal \\\n\
         -max-msg-sz 8388608 \\\n\
         example.com:443 helloworld.Greeter/SayHello"
    );
    // Without certificate checks, the server name does not matter.
    assert_eq!(
        command(GrpcSettings {
            verify_certificates: Some(false),
            server_name: "api.internal".into(),
            include_default_fields: false,
            max_response_message_mb: Some(0),
        }),
        "grpcurl -insecure \\\n\
         -max-msg-sz 4294967295 \\\n\
         example.com:443 helloworld.Greeter/SayHello"
    );
}

#[test]
fn names_proto_files_as_their_import_paths_do() {
    let command = |definition: GrpcDefinition, collection: Option<&Path>| {
        GrpcRequest {
            definition,
            ..unary("localhost:50051")
        }
        .grpcurl_command(&HashMap::new(), collection)
    };

    // Relative paths resolve from the collection, and the file's directory
    // comes after the import paths.
    assert_eq!(
        command(
            GrpcDefinition::ProtoFile {
                path: "protos/greeter/hello.proto".into(),
                import_paths: vec!["protos".into(), PathBuf::from("/opt/my protos")],
            },
            Some(Path::new("/collections/demo")),
        ),
        "grpcurl -plaintext \\\n\
         -import-path /collections/demo/protos \\\n\
         -import-path '/opt/my protos' \\\n\
         -import-path /collections/demo/protos/greeter \\\n\
         -proto greeter/hello.proto \\\n\
         localhost:50051 helloworld.Greeter/SayHello"
    );
    assert_eq!(
        command(
            GrpcDefinition::ProtoFile {
                path: "/protos/hello.proto".into(),
                import_paths: vec!["/protos".into()],
            },
            None,
        ),
        "grpcurl -plaintext \\\n\
         -import-path /protos \\\n\
         -proto hello.proto \\\n\
         localhost:50051 helloworld.Greeter/SayHello"
    );
}
