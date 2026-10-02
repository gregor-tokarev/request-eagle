use std::{fs, path::Path};

use collection::ImportedItem;
use request::{
    Body, Field, GrpcDefinition, GrpcRequest, GrpcScripts, GrpcSettings, HttpRequest, Method,
    Request, WebSocketRequest,
};

use crate::{ImportError, document_tests::read_collection};

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn request(item: &ImportedItem) -> (&str, &Request) {
    match item {
        ImportedItem::Request { name, request } => (name, request),
        ImportedItem::Folder { name, .. } => panic!("{name} is a folder"),
    }
}

fn grpc(item: &ImportedItem) -> &GrpcRequest {
    match request(item) {
        (_, Request::Grpc(request)) => request,
        (name, _) => panic!("{name} is not a gRPC request"),
    }
}

fn http(item: &ImportedItem) -> &HttpRequest {
    match request(item) {
        (_, Request::Http(request)) => request,
        (name, _) => panic!("{name} is not an HTTP request"),
    }
}

fn folder(item: &ImportedItem) -> (&str, &[ImportedItem]) {
    match item {
        ImportedItem::Folder { name, items } => (name, items),
        ImportedItem::Request { name, .. } => panic!("{name} is a request"),
    }
}

#[test]
fn postman_collection_folders_keep_their_grpc_requests() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("shop");
    write(
        &collection,
        ".resources/definition.yaml",
        r#"$kind: collection
name: Shop API
variables:
  host: localhost:50051
auth:
  - id: 52cdc78a-7186-47d0-8d61-ab1b49d90fa9
    type: bearer
    name: Bearer Token
    credentials:
      - key: token
        value: "{{token}}"
scripts:
  - type: http:beforeRequest
    code: pm.variables.set('run', 1);
    language: text/javascript
"#,
    );
    write(
        &collection,
        "Get product.request.yaml",
        r#"$kind: grpc-request
url: "{{host}}"
methodPath: shop.v1.ProductService.GetProduct
methodDescriptor: CooCCg5kaXNjb3VudC5wcm90bxII
message:
  content: |-
    {
      "id": "7"
    }
metadata:
  - key: x-request-id
    value: "{{$guid}}"
  - key: x-debug
    value: "1"
    disabled: true
settings:
  secureConnection: true
  strictSSL: false
  serverNameOverride: shop.internal
  maxResponseMessageSize: 16
  includeDefaultFields: false
schema:
  source: file
  location: /work/shop/api/product.proto
scripts:
  - type: beforeInvoke
    code: console.log('invoke');
    language: text/javascript
  - type: onMessage
    code: console.log(pm.message.data);
    language: text/javascript
  - type: afterResponse
    code: pm.test('ok', () => pm.response.to.be.ok);
    language: text/javascript
order: 1000
"#,
    );

    let import = read_collection(&collection).unwrap();

    assert_eq!(import.collection.name, "Shop API");
    assert_eq!(import.collection.variables["host"], "localhost:50051");
    assert_eq!(
        import.collection.scripts.pre_request,
        "pm.variables.set('run', 1);"
    );
    assert_eq!(request(&import.collection.items[0]).0, "Get product");
    assert_eq!(
        *grpc(&import.collection.items[0]),
        GrpcRequest {
            url: "{{host}}".into(),
            tls: true,
            method: "shop.v1.ProductService/GetProduct".into(),
            message: "{\n  \"id\": \"7\"\n}".into(),
            // The collection's authorization is sent as metadata.
            metadata: vec![
                Field::new("x-request-id", "{{$guid}}"),
                Field {
                    enabled: false,
                    ..Field::new("x-debug", "1")
                },
                Field::new("Authorization", "Bearer {{token}}"),
            ],
            definition: GrpcDefinition::ProtoFile {
                path: "/work/shop/api/product.proto".into(),
                import_paths: Vec::new(),
            },
            settings: GrpcSettings {
                verify_certificates: Some(false),
                server_name: "shop.internal".into(),
                include_default_fields: false,
                max_response_message_mb: Some(16),
                timeout_ms: None,
            },
            scripts: GrpcScripts {
                before_invoke: "console.log('invoke');".into(),
                on_message: "console.log(pm.message.data);".into(),
                after_response: "pm.test('ok', () => pm.response.to.be.ok);".into(),
            },
        }
    );
    assert!(import.skipped.is_empty());
}

#[test]
fn grpc_definitions_keep_proto_files_or_use_reflection() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("shop");
    write(
        &collection,
        ".resources/definition.yaml",
        "$kind: collection\n",
    );
    write(
        &collection,
        "Orders/List orders.request.yaml",
        r#"$kind: grpc-request
url: grpcs://orders.test
methodPath: /orders.OrderService/ListOrders
schema:
  source: file
  location: ../protos/orders.proto
order: 1000
"#,
    );
    write(
        &collection,
        "Orders/Cancel order.request.yaml",
        r#"$kind: grpc-request
url: orders.test:443
methodPath: orders.OrderService.CancelOrder
schema:
  source: api
  apiId: 6f0c1a52
order: 2000
"#,
    );

    let import = read_collection(&collection).unwrap();
    let (_, orders) = folder(&import.collection.items[0]);

    let list = grpc(&orders[0]);
    assert_eq!(list.method, "orders.OrderService/ListOrders");
    assert!(list.uses_tls());
    // A path relative to the request still finds the file once imported.
    assert_eq!(
        list.definition,
        GrpcDefinition::ProtoFile {
            path: collection.join("Orders/../protos/orders.proto"),
            import_paths: Vec::new(),
        }
    );

    let cancel = grpc(&orders[1]);
    assert_eq!(cancel.method, "orders.OrderService/CancelOrder");
    assert_eq!(cancel.definition, GrpcDefinition::Reflection);
}

#[test]
fn postman_collection_folders_keep_http_and_websocket_requests_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("Pet Store");
    // Postman lists an authorization without credentials when none is set.
    write(
        &collection,
        ".resources/definition.yaml",
        "$kind: collection\nauth:\n  - id: f98fff6a\n    type: bearer\n    name: Bearer Token\n",
    );
    write(
        &collection,
        "pets/.resources/definition.yaml",
        r#"$kind: collection
name: Pets
order: 1000
scripts:
  - type: http:afterResponse
    code: pm.test('folder', () => {});
    language: text/javascript
"#,
    );
    write(
        &collection,
        "pets/Find pet.request.yaml",
        r#"$kind: http-request
url: "{{base_url}}/pets/:id?expand=owner"
method: GET
headers:
  Accept: application/json
queryParams:
  - key: expand
    value: owner
pathVariables:
  id: "7"
scripts:
  - type: afterResponse
    code: pm.response.to.have.status(200);
    language: text/javascript
"#,
    );
    write(
        &collection,
        "pets/Add pet.request.yaml",
        r#"$kind: http-request
name: Add a pet
url: "{{base_url}}/pets"
method: POST
headers:
  - key: X-Debug
    value: "1"
    disabled: true
body:
  type: json
  content: '{"name": "Rex"}'
auth:
  type: basic
  credentials:
    username: admin
    password: secret
order: 1000
"#,
    );
    write(
        &collection,
        "Updates.request.yaml",
        r#"$kind: websocket-request
url: wss://pets.test/updates
headers:
  Authorization: Bearer {{token}}
messages: .resources/Updates.resources/messages
order: 2000
"#,
    );
    write(
        &collection,
        "Feed.request.yaml",
        "$kind: mqtt-request\nurl: mqtt://pets.test\norder: 500\n",
    );

    let import = read_collection(&collection).unwrap();
    let items = &import.collection.items;

    assert_eq!(import.collection.name, "Pet Store");
    assert_eq!(import.skipped, ["Feed"]);
    assert_eq!(items.len(), 2);

    let (name, pets) = folder(&items[0]);
    assert_eq!(name, "Pets");

    // Ordered requests come before the ones without an order.
    assert_eq!(request(&pets[0]).0, "Add a pet");
    let add = http(&pets[0]);
    assert_eq!(add.method, Method::Post);
    assert_eq!(add.body, Some(Body::json(r#"{"name": "Rex"}"#)));
    // JSON bodies are sent as JSON without a header of their own.
    assert_eq!(
        add.headers,
        [
            Field {
                enabled: false,
                ..Field::new("X-Debug", "1")
            },
            Field::new("Authorization", "Basic YWRtaW46c2VjcmV0"),
        ]
    );

    let find = http(&pets[1]);
    assert_eq!(find.path, "{{base_url}}/pets/:id?expand=owner");
    assert_eq!(find.path_variables, [("id".to_owned(), "7".to_owned())]);
    assert_eq!(find.headers, [Field::new("Accept", "application/json")]);
    assert_eq!(
        find.scripts.post_response,
        "{\npm.test('folder', () => {});\n}\n\n{\npm.response.to.have.status(200);\n}"
    );

    let (name, Request::WebSocket(updates)) = request(&items[1]) else {
        panic!("expected a WebSocket request");
    };
    assert_eq!(name, "Updates");
    assert_eq!(
        *updates,
        WebSocketRequest {
            url: "wss://pets.test/updates".into(),
            headers: vec![Field::new("Authorization", "Bearer {{token}}")],
            ..WebSocketRequest::default()
        }
    );
}

#[test]
fn requests_can_select_one_of_the_listed_authorizations() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("shop");
    write(
        &collection,
        ".resources/definition.yaml",
        r#"$kind: collection
auth:
  - id: first
    type: bearer
    name: First
    credentials:
      token: ONE
  - id: second
    type: bearer
    name: Second
    credentials:
      token: TWO
"#,
    );
    write(
        &collection,
        "Default.request.yaml",
        "$kind: grpc-request\nmethodPath: shop.Shop.Get\norder: 1\n",
    );
    write(
        &collection,
        "Selected.request.yaml",
        r#"$kind: grpc-request
methodPath: shop.Shop.Get
auth:
  type: inherit
  credentials:
    id: second
order: 2
"#,
    );
    write(
        &collection,
        "Admin/.resources/definition.yaml",
        "$kind: collection\nauth:\n  - id: admin\n    type: bearer\n    name: Admin\n    credentials:\n      token: ADMIN\n",
    );
    write(
        &collection,
        "Admin/Selected.request.yaml",
        r#"$kind: http-request
url: https://shop.test/admin
auth:
  type: inherit
  credentials:
    - key: id
      value: second
"#,
    );

    let import = read_collection(&collection).unwrap();
    let items = &import.collection.items;
    let bearer = |token: &str| vec![Field::new("Authorization", format!("Bearer {token}"))];

    assert_eq!(grpc(&items[0]).metadata, bearer("ONE"));
    assert_eq!(grpc(&items[1]).metadata, bearer("TWO"));
    let (_, admin) = folder(&items[2]);
    assert_eq!(http(&admin[0]).headers, bearer("TWO"));
}

#[test]
fn collection_folders_without_a_definition_are_named_after_the_folder() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("Pet Store");
    write(
        &collection,
        "List pets.request.yaml",
        "$kind: http-request\nurl: https://pets.test/pets\n",
    );

    let import = read_collection(&collection).unwrap();

    assert_eq!(import.collection.name, "Pet Store");
    assert_eq!(
        http(&import.collection.items[0]).path,
        "https://pets.test/pets"
    );
}

#[test]
fn folders_that_are_not_postman_collections_are_explained() {
    let directory = tempfile::tempdir().unwrap();
    // A workspace keeps its collections in `postman/collections`.
    write(
        directory.path(),
        "postman/collections/Shop/.resources/definition.yaml",
        "$kind: collection\n",
    );

    assert!(matches!(
        read_collection(directory.path()).err().unwrap(),
        ImportError::NotPostmanFolder
    ));
    assert!(matches!(
        read_collection(&directory.path().join("missing.json"))
            .err()
            .unwrap(),
        ImportError::Read { .. }
    ));
}
