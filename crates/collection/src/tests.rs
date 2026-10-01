use std::{fs, path::PathBuf, time::SystemTime};

use crate::{CollectionRegistry, Entry};

use request::{GrpcRequest, GrpcScripts, Method, Request};

fn test_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "request-eagle-collection-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn loads_edits_and_saves_a_directory_without_losing_user_content() {
    let root = test_directory();
    let collection = root.join("API");
    let nested = collection.join("users");
    let request_path = nested.join("list.toml");

    fs::create_dir_all(&nested).unwrap();
    fs::write(
        &request_path,
        r#"# user's request
id = "list-users"
name = "List users"
schema_version = 1
custom = "keep me"

[request]
type = "http"
method = "GET"
path = "/users"
headers = [["Accept", "application/json"]]
request_custom = "keep me too"
"#,
    )
    .unwrap();
    fs::write(
        collection.join("environment.toml"),
        "base_url = \"local\"\n",
    )
    .unwrap();

    let mut registry = CollectionRegistry::from_path(&root).unwrap();
    let [loaded] = registry.collections() else {
        panic!("expected one collection");
    };
    let [Entry::Directory(users)] = loaded.entries.as_slice() else {
        panic!("expected only the users directory");
    };
    let [Entry::File(request)] = users.entries.as_slice() else {
        panic!("expected request file");
    };
    let Request::Http(mut request) = request.request.clone() else {
        panic!("expected an HTTP request");
    };

    assert!(matches!(request.method, Method::Get));

    request.path = "/v2/users".into();
    registry
        .update_request(&request_path, "list-users", request.into())
        .unwrap();

    let saved = fs::read_to_string(&request_path).unwrap();
    assert!(saved.contains("# user's request"));
    assert!(saved.contains("custom = \"keep me\""));
    assert!(saved.contains("request_custom = \"keep me too\""));
    assert!(saved.contains("path = \"/v2/users\""));

    let reloaded = CollectionRegistry::from_path(&root).unwrap();
    let Request::Http(request) = &reloaded.file(&request_path).unwrap().request else {
        panic!("expected an HTTP request");
    };

    assert_eq!(request.path, "/v2/users");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleared_grpc_scripts_are_removed_from_the_file() {
    let root = test_directory();
    let collection = root.join("API");
    let request_path = collection.join("say.toml");

    fs::create_dir_all(&collection).unwrap();
    fs::write(
        &request_path,
        r#"id = "say"
name = "Say"
schema_version = 1

[request]
type = "grpc"
url = "localhost:50051"

[request.scripts]
before_invoke = "console.log(1);"
on_message = "console.log(2);"
"#,
    )
    .unwrap();

    let mut registry = CollectionRegistry::from_path(&root).unwrap();
    let request = GrpcRequest {
        url: "localhost:50051".into(),
        scripts: GrpcScripts {
            on_message: "console.log(2);".into(),
            ..GrpcScripts::default()
        },
        ..GrpcRequest::default()
    };
    registry
        .update_request(&request_path, "say", request.into())
        .unwrap();

    let saved = fs::read_to_string(&request_path).unwrap();
    assert!(!saved.contains("before_invoke"), "{saved}");
    assert!(saved.contains("on_message"), "{saved}");

    fs::remove_dir_all(root).unwrap();
}
