use collection::ImportedItem;
use request::{HttpRequest, Method, Request};

use crate::parse;

fn http(item: &ImportedItem) -> (&str, &HttpRequest) {
    match item {
        ImportedItem::Request {
            name,
            request: Request::Http(request),
        } => (name, request),
        ImportedItem::Request { name, .. } => panic!("{name} is not an HTTP request"),
        ImportedItem::Folder { name, .. } => panic!("{name} is a folder"),
    }
}

fn folder(item: &ImportedItem) -> (&str, &[ImportedItem]) {
    match item {
        ImportedItem::Folder { name, items } => (name, items),
        ImportedItem::Request { name, .. } => panic!("{name} is a request"),
    }
}

const COLLECTION: &str = r#"{
    "info": {
        "name": "Pet Store",
        "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
    },
    "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}", "type": "string"}]},
    "event": [{"listen": "prerequest", "script": {"exec": ["pm.variables.set('run', 1);"]}}],
    "variable": [
        {"key": "base_url", "value": "https://pets.test"},
        {"key": "limit", "value": 10},
        {"key": "old", "value": "x", "disabled": true}
    ],
    "item": [
        {
            "name": "Pets",
            "event": [{"listen": "test", "script": {"exec": ["pm.test('folder', () => {});"]}}],
            "item": [
                {
                    "name": "Find pet",
                    "event": [
                        {"listen": "test", "script": {"exec": ["pm.response.to.have.status(200);"]}},
                        {"listen": "prerequest", "script": {"exec": [""]}}
                    ],
                    "request": {
                        "method": "GET",
                        "header": [
                            {"key": "Accept", "value": "application/json"},
                            {"key": "X-Debug", "value": "1", "disabled": true}
                        ],
                        "url": {
                            "raw": "{{base_url}}/pets/:id?expand=owner%20name",
                            "host": ["{{base_url}}"],
                            "path": ["pets", ":id"],
                            "variable": [{"key": "id", "value": "7"}]
                        }
                    }
                },
                {
                    "name": "Add pet",
                    "request": {
                        "method": "POST",
                        "auth": {"type": "noauth"},
                        "body": {
                            "mode": "raw",
                            "raw": "{\"name\": \"Rex\"}",
                            "options": {"raw": {"language": "json"}}
                        },
                        "url": "{{base_url}}/pets"
                    }
                }
            ]
        },
        {
            "name": "Lock pet",
            "request": {"method": "LOCK", "url": "{{base_url}}/pets/1"}
        },
        {
            "name": "Login",
            "request": {
                "method": "post",
                "auth": {"type": "basic", "basic": [
                    {"key": "username", "value": "admin"},
                    {"key": "password", "value": "secret"}
                ]},
                "body": {
                    "mode": "urlencoded",
                    "urlencoded": [
                        {"key": "grant type", "value": "password & more"},
                        {"key": "scope", "value": "{{scope}}"},
                        {"key": "id", "value": "{{$guid}}"},
                        {"key": "skip", "value": "x", "disabled": true}
                    ]
                },
                "url": "{{base_url}}/login"
            }
        }
    ]
}"#;

#[test]
fn postman_collections_keep_their_folders_variables_and_scripts() {
    let import = parse(COLLECTION).unwrap();
    let collection = &import.collection;

    assert_eq!(collection.name, "Pet Store");
    assert_eq!(collection.variables["base_url"], "https://pets.test");
    assert_eq!(collection.variables["limit"], "10");
    assert!(!collection.variables.contains_key("old"));
    assert_eq!(
        collection.scripts.pre_request,
        "pm.variables.set('run', 1);"
    );

    let (name, items) = folder(&collection.items[0]);
    assert_eq!(name, "Pets");

    let (name, find) = http(&items[0]);
    assert_eq!(name, "Find pet");
    assert_eq!(find.method, Method::Get);
    assert_eq!(find.path, "{{base_url}}/pets/7?expand=owner%20name");
    assert_eq!(
        find.headers,
        [
            ("Accept".to_owned(), "application/json".to_owned()),
            ("Authorization".to_owned(), "Bearer {{token}}".to_owned()),
        ]
    );
    // Folder scripts run before the request's own, each in its own block.
    assert_eq!(
        find.scripts.post_response,
        "{\npm.test('folder', () => {});\n}\n\n{\npm.response.to.have.status(200);\n}"
    );
    assert_eq!(find.scripts.pre_request, "");

    let (_, add) = http(&items[1]);
    assert_eq!(add.body.as_deref(), Some(br#"{"name": "Rex"}"#.as_slice()));
    assert_eq!(
        add.headers,
        [("Content-Type".to_owned(), "application/json".to_owned())]
    );
}

#[test]
fn postman_requests_with_unsupported_methods_are_reported() {
    let import = parse(COLLECTION).unwrap();

    assert_eq!(import.skipped, ["Lock pet"]);
    assert_eq!(import.collection.items.len(), 2);
}

#[test]
fn postman_forms_and_basic_auth_are_encoded() {
    let import = parse(COLLECTION).unwrap();
    let (name, login) = http(&import.collection.items[1]);

    assert_eq!(name, "Login");
    assert_eq!(login.method, Method::Post);
    assert_eq!(
        String::from_utf8(login.body.clone().unwrap()).unwrap(),
        "grant%20type=password%20%26%20more&scope={{scope}}&id={{$guid}}"
    );
    // Variables are encoded once they are filled in, when sending.
    assert!(
        login
            .scripts
            .pre_request
            .starts_with("// Encode the form after filling in its variables")
    );
    assert_eq!(
        login.headers,
        [
            (
                "Content-Type".to_owned(),
                "application/x-www-form-urlencoded".to_owned()
            ),
            (
                "Authorization".to_owned(),
                "Basic YWRtaW46c2VjcmV0".to_owned()
            ),
        ]
    );
}

#[test]
fn postman_basic_auth_with_variables_is_encoded_when_sending() {
    let import = parse(
        r#"{
            "info": {"name": "Auth"},
            "item": [{
                "name": "Me",
                "event": [{"listen": "prerequest", "script": {"exec": "console.log('me')"}}],
                "request": {
                    "auth": {"type": "basic", "basic": {"username": "{{user}}", "password": "{{password}}"}},
                    "url": "https://example.com/me"
                }
            }]
        }"#,
    )
    .unwrap();
    let (_, me) = http(&import.collection.items[0]);

    assert!(me.headers.is_empty());
    // Authorization follows the scripts that may set its credentials.
    assert_eq!(
        me.scripts.pre_request,
        "{\nconsole.log('me')\n}\n\n{\n\
         pm.request.headers.upsert({key: \"Authorization\", value: \"Basic \" + \
         pm.encoding.base64Encode(pm.variables.replaceIn(\"{{user}}:{{password}}\"))});\n}"
    );
}

#[test]
fn postman_form_data_and_graphql_bodies_become_raw_bodies() {
    let import = parse(
        r#"{
            "info": {"name": "Bodies"},
            "item": [
                {
                    "name": "Upload",
                    "request": {
                        "method": "POST",
                        "body": {"mode": "formdata", "formdata": [
                            {"key": "title", "value": "Cat", "type": "text"},
                            {"key": "photo", "src": "/tmp/cat.png", "type": "file"}
                        ]},
                        "url": "https://example.com/upload"
                    }
                },
                {
                    "name": "Query",
                    "request": {
                        "method": "POST",
                        "body": {"mode": "graphql", "graphql": {
                            "query": "query { pets { id } }",
                            "variables": "{\"first\": 2}"
                        }},
                        "url": "https://example.com/graphql"
                    }
                },
                {
                    "name": "Templated query",
                    "request": {
                        "method": "POST",
                        "body": {"mode": "graphql", "graphql": {
                            "query": "query($limit: Int!) { pets(limit: $limit) { id } }",
                            "variables": "{\"limit\": {{limit}}}"
                        }},
                        "url": {"protocol": "https", "host": ["example", "com"], "port": "8443", "path": ["graphql"]}
                    }
                },
                {
                    "name": "Key",
                    "request": {
                        "auth": {"type": "apikey", "apikey": [
                            {"key": "key", "value": "api_key"},
                            {"key": "value", "value": "{{key}}"},
                            {"key": "in", "value": "query"}
                        ]},
                        "url": "https://example.com/key"
                    }
                }
            ]
        }"#,
    )
    .unwrap();

    let (_, upload) = http(&import.collection.items[0]);
    assert_eq!(
        String::from_utf8(upload.body.clone().unwrap()).unwrap(),
        "--RequestEagleFormBoundary\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nCat\r\n\
         --RequestEagleFormBoundary--\r\n"
    );
    assert_eq!(
        upload.headers[0].1,
        "multipart/form-data; boundary=RequestEagleFormBoundary"
    );

    let (_, query) = http(&import.collection.items[1]);
    let payload: serde_json::Value = serde_json::from_slice(query.body.as_ref().unwrap()).unwrap();
    assert_eq!(
        payload,
        serde_json::json!({"query": "query { pets { id } }", "variables": {"first": 2}})
    );

    // Variables that are JSON only once filled in are kept as written.
    let (_, templated) = http(&import.collection.items[2]);
    assert_eq!(
        String::from_utf8(templated.body.clone().unwrap()).unwrap(),
        "{\n  \"query\": \"query($limit: Int!) { pets(limit: $limit) { id } }\",\n  \
         \"variables\": {\"limit\": {{limit}}}\n}"
    );
    assert_eq!(templated.path, "https://example.com:8443/graphql");

    let (_, key) = http(&import.collection.items[3]);
    assert_eq!(key.method, Method::Get);
    assert_eq!(key.query, [("api_key".to_owned(), "{{key}}".to_owned())]);
}
