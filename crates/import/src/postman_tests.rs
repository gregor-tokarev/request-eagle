use collection::ImportedItem;
use request::{
    ApiKeyAuth, Auth, AuthLocation, AwsSignatureAuth, BearerAuth, Body, Field, FormPart,
    HttpRequest, JwtAlgorithm, JwtAuth, Method, OAuth1Auth, OAuth1Signature, OAuth2Auth,
    OAuth2ClientAuthentication, OAuth2Grant, PasswordAuth, Request,
};

use crate::document_tests::parse_collection;

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
                            {"key": "X-Debug", "value": "1", "disabled": true, "description": "Server traces"}
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
    let import = parse_collection(COLLECTION).unwrap();
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
    assert_eq!(find.path, "{{base_url}}/pets/:id?expand=owner%20name");
    assert_eq!(find.path_variables, [("id".to_owned(), "7".to_owned())]);
    // Headers that are switched off stay with their descriptions, but are
    // not sent.
    assert_eq!(
        find.headers,
        [
            Field::new("Accept", "application/json"),
            Field {
                enabled: false,
                description: "Server traces".into(),
                ..Field::new("X-Debug", "1")
            },
        ]
    );
    // The collection's authorization is its own, which requests inherit.
    assert_eq!(
        collection.auth,
        Auth::Bearer(BearerAuth {
            token: "{{token}}".into()
        })
    );
    assert_eq!(find.auth, Auth::Inherit);
    // Folder scripts run before the request's own, each in its own block.
    assert_eq!(
        find.scripts.post_response,
        "{\npm.test('folder', () => {});\n}\n\n{\npm.response.to.have.status(200);\n}"
    );
    assert_eq!(find.scripts.pre_request, "");

    let (_, add) = http(&items[1]);
    assert_eq!(add.body, Some(Body::json(r#"{"name": "Rex"}"#)));
    // JSON bodies are sent as JSON without a header of their own.
    assert!(add.headers.is_empty());
    assert_eq!(add.auth, Auth::None);
}

#[test]
fn postman_requests_with_unsupported_methods_are_reported() {
    let import = parse_collection(COLLECTION).unwrap();

    assert_eq!(import.skipped, ["Lock pet"]);
    assert_eq!(import.collection.items.len(), 2);
}

#[test]
fn postman_request_settings_keep_redirects_and_certificate_checks() {
    let import = parse_collection(
        r#"{
            "info": {"name": "Settings", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item": [
                {"name": "Changed", "protocolProfileBehavior": {"followRedirects": false, "strictSSL": false, "disableBodyPruning": true},
                 "request": {"method": "GET", "url": "https://example.test"}},
                {"name": "Default", "request": {"method": "GET", "url": "https://example.test"}},
                {"name": "Internal", "protocolProfileBehavior": {"strictSSL": false}, "item": [
                    {"name": "Inherits", "request": {"method": "GET", "url": "https://internal.test"}},
                    {"name": "Overrides", "protocolProfileBehavior": {"strictSSL": true},
                     "request": {"method": "GET", "url": "https://internal.test"}}
                ]}
            ]
        }"#,
    )
    .unwrap();

    let (_, changed) = http(&import.collection.items[0]);
    assert_eq!(changed.settings.follow_redirects, Some(false));
    assert_eq!(changed.settings.verify_certificates, Some(false));
    assert_eq!(changed.settings.timeout_ms, None);

    let (_, default) = http(&import.collection.items[1]);
    assert!(default.settings.is_default());

    // Folders set it for the requests inside them, which can change it again.
    let (_, items) = folder(&import.collection.items[2]);
    assert_eq!(http(&items[0]).1.settings.verify_certificates, Some(false));
    assert_eq!(http(&items[1]).1.settings.verify_certificates, Some(true));
}

#[test]
fn postman_forms_and_basic_auth_are_kept() {
    let import = parse_collection(COLLECTION).unwrap();
    let (name, login) = http(&import.collection.items[1]);

    assert_eq!(name, "Login");
    assert_eq!(login.method, Method::Post);
    // Sending encodes each field once its variables are filled in.
    assert_eq!(
        login.body,
        Some(Body::UrlEncoded {
            fields: vec![
                ("grant type".into(), "password & more".into()),
                ("scope".into(), "{{scope}}".into()),
                ("id".into(), "{{$guid}}".into()),
            ],
        })
    );
    assert!(login.scripts.pre_request.is_empty());
    assert!(login.headers.is_empty());
    assert_eq!(
        login.auth,
        Auth::Basic(PasswordAuth {
            username: "admin".into(),
            password: "secret".into(),
        })
    );
}

#[test]
fn postman_authorizations_keep_their_settings() {
    let import = parse_collection(
        r#"{
            "info": {"name": "Auth"},
            "item": [
                {"name": "Folder", "auth": {"type": "digest", "digest": {"username": "{{user}}", "password": "{{password}}"}}, "item": [
                    {"name": "Inherits the folder's", "request": {"url": "https://example.com/digest"}}
                ]},
                {"name": "OAuth 1.0", "request": {"url": "https://example.com", "auth": {"type": "oauth1", "oauth1": [
                    {"key": "signatureMethod", "value": "HMAC-SHA256"},
                    {"key": "consumerKey", "value": "key"},
                    {"key": "consumerSecret", "value": "secret"},
                    {"key": "token", "value": "token"},
                    {"key": "tokenSecret", "value": "{{tokenSecret}}"},
                    {"key": "addParamsToHeader", "value": false}
                ]}}},
                {"name": "OAuth 2.0", "request": {"url": "https://example.com", "auth": {"type": "oauth2", "oauth2": [
                    {"key": "accessToken", "value": "{{access}}"},
                    {"key": "addTokenTo", "value": "queryParams"},
                    {"key": "grant_type", "value": "authorization_code_with_pkce"},
                    {"key": "authUrl", "value": "https://id.example.com/authorize"},
                    {"key": "accessTokenUrl", "value": "https://id.example.com/token"},
                    {"key": "redirect_uri", "value": "https://oauth.pstmn.io/v1/callback"},
                    {"key": "clientId", "value": "app"},
                    {"key": "scope", "value": "openid"},
                    {"key": "client_authentication", "value": "body"}
                ]}}},
                {"name": "JWT", "request": {"url": "https://example.com", "auth": {"type": "jwt", "jwt": [
                    {"key": "algorithm", "value": "RS256"},
                    {"key": "privateKey", "value": "{{key}}"},
                    {"key": "payload", "value": "{\"sub\": \"me\"}"},
                    {"key": "header", "value": "{}"},
                    {"key": "addTokenTo", "value": "queryParam"},
                    {"key": "queryParamKey", "value": "jwt"}
                ]}}},
                {"name": "AWS", "request": {"url": "https://example.com", "auth": {"type": "awsv4", "awsv4": [
                    {"key": "accessKey", "value": "AKID"},
                    {"key": "secretKey", "value": "{{secret}}"},
                    {"key": "region", "value": "eu-west-1"},
                    {"key": "service", "value": "execute-api"}
                ]}}},
                {"name": "API key", "request": {"url": "https://example.com", "auth": {"type": "apikey", "apikey": {"key": "X-Key", "value": "{{key}}"}}}},
                {"name": "NTLM", "request": {"url": "https://example.com", "auth": {"type": "ntlm", "ntlm": []}}}
            ]
        }"#,
    )
    .unwrap();
    let items = &import.collection.items;

    // Request Eagle has no folder authorization, so requests take their
    // folder's as their own.
    let (_, folder_items) = folder(&items[0]);
    assert_eq!(
        http(&folder_items[0]).1.auth,
        Auth::Digest(PasswordAuth {
            username: "{{user}}".into(),
            password: "{{password}}".into(),
        })
    );
    assert_eq!(
        http(&items[1]).1.auth,
        Auth::OAuth1(Box::new(OAuth1Auth {
            signature_method: OAuth1Signature::HmacSha256,
            consumer_key: "key".into(),
            consumer_secret: "secret".into(),
            access_token: "token".into(),
            token_secret: "{{tokenSecret}}".into(),
            add_to: AuthLocation::Query,
            ..OAuth1Auth::default()
        }))
    );
    assert_eq!(
        http(&items[2]).1.auth,
        Auth::OAuth2(Box::new(OAuth2Auth {
            access_token: "{{access}}".into(),
            add_to: AuthLocation::Query,
            grant_type: OAuth2Grant::AuthorizationCode,
            pkce: true,
            auth_url: "https://id.example.com/authorize".into(),
            token_url: "https://id.example.com/token".into(),
            client_id: "app".into(),
            scope: "openid".into(),
            client_authentication: OAuth2ClientAuthentication::Body,
            // Postman's callback page returns to Postman.
            ..OAuth2Auth::default()
        }))
    );
    assert_eq!(
        http(&items[3]).1.auth,
        Auth::Jwt(Box::new(JwtAuth {
            algorithm: JwtAlgorithm::Rs256,
            private_key: "{{key}}".into(),
            payload: "{\"sub\": \"me\"}".into(),
            add_to: AuthLocation::Query,
            query_param: "jwt".into(),
            ..JwtAuth::default()
        }))
    );
    assert_eq!(
        http(&items[4]).1.auth,
        Auth::AwsSignature(Box::new(AwsSignatureAuth {
            access_key: "AKID".into(),
            secret_key: "{{secret}}".into(),
            region: "eu-west-1".into(),
            service: "execute-api".into(),
            ..AwsSignatureAuth::default()
        }))
    );
    assert_eq!(
        http(&items[5]).1.auth,
        Auth::ApiKey(ApiKeyAuth {
            key: "X-Key".into(),
            value: "{{key}}".into(),
            add_to: AuthLocation::Header,
        })
    );
    assert_eq!(http(&items[6]).1.auth, Auth::None);
}

#[test]
fn postman_form_data_files_and_graphql_bodies_are_kept() {
    let import = parse_collection(
        r#"{
            "info": {"name": "Bodies"},
            "item": [
                {
                    "name": "Upload",
                    "request": {
                        "method": "POST",
                        "body": {"mode": "formdata", "formdata": [
                            {"key": "title", "value": "Cat", "type": "text"},
                            {"key": "photo", "src": "/tmp/cat.png", "type": "file"},
                            {"key": "more", "src": ["/tmp/a.png", "/tmp/b.png"], "type": "file"},
                            {"key": "later", "src": null, "type": "file"},
                            {"key": "off", "value": "x", "disabled": true}
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
                    "name": "File",
                    "request": {
                        "method": "PUT",
                        "body": {"mode": "file", "file": {"src": "/tmp/data.bin"}},
                        "url": "https://example.com/data"
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
    let part = |name: &str, value: &str, file| FormPart {
        name: name.into(),
        value: value.into(),
        file,
    };
    assert_eq!(
        upload.body,
        Some(Body::Multipart {
            parts: vec![
                part("title", "Cat", false),
                part("photo", "/tmp/cat.png", true),
                part("more", "/tmp/a.png", true),
                part("more", "/tmp/b.png", true),
                part("later", "", true),
            ],
        })
    );
    assert!(upload.headers.is_empty());

    let (_, query) = http(&import.collection.items[1]);
    let Some(Body::Raw { text, .. }) = &query.body else {
        panic!("expected a raw body");
    };
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        payload,
        serde_json::json!({"query": "query { pets { id } }", "variables": {"first": 2}})
    );

    // Variables that are JSON only once filled in are kept as written.
    let (_, templated) = http(&import.collection.items[2]);
    assert_eq!(
        templated.body,
        Some(Body::json(
            "{\n  \"query\": \"query($limit: Int!) { pets(limit: $limit) { id } }\",\n  \
             \"variables\": {\"limit\": {{limit}}}\n}"
        ))
    );
    assert_eq!(templated.path, "https://example.com:8443/graphql");

    let (_, file) = http(&import.collection.items[3]);
    assert_eq!(
        file.body,
        Some(Body::Binary {
            file: "/tmp/data.bin".into()
        })
    );

    let (_, key) = http(&import.collection.items[4]);
    assert_eq!(key.method, Method::Get);
    assert_eq!(
        key.auth,
        Auth::ApiKey(ApiKeyAuth {
            key: "api_key".into(),
            value: "{{key}}".into(),
            add_to: AuthLocation::Query,
        })
    );
}
