use crate::{ImportError, parse};

#[test]
fn formats_are_recognized_by_their_content() {
    let postman = "\u{feff}{\"info\": {\"name\": \"Postman\"}, \"item\": []}";
    let openapi = "openapi: 3.1.0\ninfo:\n  title: OpenAPI\npaths: {}\n";

    assert_eq!(parse(postman).unwrap().collection.name, "Postman");
    assert_eq!(parse(openapi).unwrap().collection.name, "OpenAPI");
}

#[test]
fn unreadable_and_unknown_files_are_explained() {
    assert!(matches!(
        parse("{\"info\": ").err().unwrap(),
        ImportError::Syntax(_)
    ));
    assert!(matches!(
        parse("paths: [unclosed").err().unwrap(),
        ImportError::Syntax(_)
    ));
    assert!(matches!(
        parse("{\"values\": []}").err().unwrap(),
        ImportError::UnknownFormat
    ));
    assert!(matches!(
        parse("{\"id\": \"1\", \"requests\": [], \"order\": []}")
            .err()
            .unwrap(),
        ImportError::PostmanV1
    ));
    // Postman writes each request of a collection folder to its own file.
    assert!(matches!(
        parse("$kind: grpc-request\nurl: localhost:50051\n")
            .err()
            .unwrap(),
        ImportError::PostmanV3File
    ));
}

#[test]
fn names_are_single_lines() {
    let import = parse(
        r#"{"info": {"name": "  Pets\nAPI  "}, "item": [{"name": "", "request": {"url": "/"}}]}"#,
    )
    .unwrap();

    assert_eq!(import.collection.name, "Pets API");

    let collection::ImportedItem::Request { name, .. } = &import.collection.items[0] else {
        panic!("expected a request");
    };
    assert_eq!(name, "Untitled");
}
