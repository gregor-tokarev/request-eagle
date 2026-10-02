use std::path::{Path, PathBuf};

use crate::{Body, ExecutionError, FormPart, HttpRequest, Method, RawLanguage};

fn post(body: Body) -> HttpRequest {
    HttpRequest {
        method: Method::Post,
        path: "https://example.com/".into(),
        body: Some(body),
        ..Default::default()
    }
}

fn content_type(request: &HttpRequest) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str())
}

#[test]
fn bodies_saved_as_bytes_load_as_raw_json() {
    let request: HttpRequest =
        toml::from_str("method = 'POST'\npath = '/'\nbody = [123, 125]\n").unwrap();
    assert_eq!(request.body, Some(Body::json("{}")));

    let request: HttpRequest =
        serde_json::from_str(r#"{"method":"POST","path":"/","body":null}"#).unwrap();
    assert_eq!(request.body, None);

    let request: HttpRequest = serde_json::from_str(r#"{"method":"POST","path":"/"}"#).unwrap();
    assert_eq!(request.body, None);
}

#[test]
fn bodies_are_saved_as_readable_tables() {
    let request = HttpRequest {
        body: Some(Body::Multipart {
            parts: vec![
                FormPart {
                    name: "title".into(),
                    value: "Hello".into(),
                    file: false,
                },
                FormPart {
                    name: "avatar".into(),
                    value: "files/eagle.png".into(),
                    file: true,
                },
            ],
        }),
        ..post(Body::json(""))
    };

    let saved = toml::to_string(&request).unwrap();
    assert!(saved.contains("type = \"multipart\""), "{saved}");
    assert!(saved.contains("file = true"), "{saved}");
    assert_eq!(saved.matches("file = ").count(), 1, "{saved}");
    assert_eq!(toml::from_str::<HttpRequest>(&saved).unwrap(), request);

    for body in [
        Body::Raw {
            language: RawLanguage::Xml,
            text: "<a/>".into(),
        },
        Body::UrlEncoded {
            fields: vec![("name".into(), "Eagle".into())],
        },
        Body::Binary {
            file: "data.bin".into(),
        },
    ] {
        let request = post(body);
        let saved = toml::to_string(&request).unwrap();
        assert_eq!(toml::from_str::<HttpRequest>(&saved).unwrap(), request);
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<HttpRequest>(&json).unwrap(), request);
    }

    let error =
        toml::from_str::<HttpRequest>("method = 'POST'\npath = '/'\n[body]\ntype = 'graphql'\n")
            .unwrap_err();
    assert!(error.to_string().contains("graphql"), "{error}");
}

#[test]
fn forms_encode_each_field_and_name_their_type() {
    let mut request = post(Body::UrlEncoded {
        fields: vec![
            ("name".into(), "Rex & co".into()),
            ("tag[]".into(), "a=b+c".into()),
            ("emoji".into(), "🦅".into()),
        ],
    });

    let body = request.encode_body().unwrap().unwrap();
    assert_eq!(
        String::from_utf8(body).unwrap(),
        "name=Rex+%26+co&tag%5B%5D=a%3Db%2Bc&emoji=%F0%9F%A6%85"
    );
    assert_eq!(
        content_type(&request),
        Some("application/x-www-form-urlencoded")
    );
}

#[test]
fn raw_bodies_are_sent_with_their_language_unless_a_header_names_a_type() {
    let mut xml = post(Body::Raw {
        language: RawLanguage::Xml,
        text: "<a/>".into(),
    });
    assert_eq!(xml.encode_body().unwrap().unwrap(), b"<a/>");
    assert_eq!(content_type(&xml), Some("application/xml"));

    let mut text = post(Body::Raw {
        language: RawLanguage::Text,
        text: "hello".into(),
    });
    text.headers
        .push(("content-type".into(), "text/csv".into()));
    text.encode_body().unwrap();
    assert_eq!(text.headers.len(), 1);
    assert_eq!(content_type(&text), Some("text/csv"));

    // Empty text sends no body, as an empty editor did before body types.
    let mut empty = post(Body::json(""));
    assert_eq!(empty.encode_body().unwrap(), None);
    assert!(empty.headers.is_empty());
}

#[test]
fn multipart_forms_send_text_and_files_between_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let image = directory.path().join("eagle \"1\".png");
    std::fs::write(&image, [0x89, b'P', b'N', b'G']).unwrap();

    let mut request = post(Body::Multipart {
        parts: vec![
            FormPart {
                name: "title".into(),
                value: "Hello\r\nworld".into(),
                file: false,
            },
            FormPart {
                name: "avatar".into(),
                value: image.to_string_lossy().into_owned(),
                file: true,
            },
        ],
    });
    let body = request.encode_body().unwrap().unwrap();

    let sent_type = content_type(&request).unwrap();
    let boundary = sent_type
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap();
    let mut expected = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nHello\r\nworld\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"eagle %221%22.png\"\r\nContent-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    expected.extend_from_slice(&[0x89, b'P', b'N', b'G']);
    expected.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    assert_eq!(body, expected);

    // Another send separates its parts with another boundary.
    let mut again = post(request.body.clone().unwrap());
    again.encode_body().unwrap();
    assert_ne!(content_type(&again).unwrap(), sent_type);
}

#[test]
fn a_multipart_type_written_without_a_boundary_gets_the_one_sent() {
    let mut request = post(Body::Multipart { parts: Vec::new() });
    request
        .headers
        .push(("Content-Type".into(), "multipart/mixed".into()));

    let body = String::from_utf8(request.encode_body().unwrap().unwrap()).unwrap();
    let boundary = content_type(&request)
        .unwrap()
        .strip_prefix("multipart/mixed; boundary=")
        .unwrap();
    assert_eq!(body, format!("--{boundary}--\r\n"));
}

#[test]
fn files_are_read_when_sent_and_must_be_chosen() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("data.json");
    std::fs::write(&file, "{}").unwrap();

    let mut request = post(Body::Binary { file: file.clone() });
    assert_eq!(request.encode_body().unwrap().unwrap(), b"{}");
    assert_eq!(content_type(&request), Some("application/json"));

    let mut unknown = post(Body::Binary {
        file: directory.path().join("data"),
    });
    let error = unknown.encode_body().unwrap_err();
    assert!(matches!(error, ExecutionError::BodyFile { .. }), "{error}");

    let mut unchosen = post(Body::Binary {
        file: PathBuf::new(),
    });
    assert_eq!(
        unchosen.encode_body().unwrap_err().to_string(),
        "choose a file to send for the body"
    );

    let mut unchosen = post(Body::Multipart {
        parts: vec![FormPart {
            name: "avatar".into(),
            value: String::new(),
            file: true,
        }],
    });
    assert_eq!(
        unchosen.encode_body().unwrap_err().to_string(),
        "choose a file to send for the form field \"avatar\""
    );
}

#[test]
fn relative_files_resolve_from_the_collection() {
    let collection = Path::new("/collections/pets");
    let absolute = if cfg!(windows) {
        "C:\\data.bin"
    } else {
        "/data.bin"
    };
    let mut body = Body::Multipart {
        parts: vec![
            FormPart {
                name: "relative".into(),
                value: "files/a.png".into(),
                file: true,
            },
            FormPart {
                name: "absolute".into(),
                value: absolute.into(),
                file: true,
            },
            FormPart {
                name: "unchosen".into(),
                value: String::new(),
                file: true,
            },
            FormPart {
                name: "text".into(),
                value: "files/a.png".into(),
                file: false,
            },
        ],
    };
    body.resolve_files(collection);

    let Body::Multipart { parts } = body else {
        unreachable!()
    };
    let values: Vec<_> = parts.iter().map(|part| part.value.as_str()).collect();
    assert_eq!(
        values,
        [
            collection.join("files/a.png").to_str().unwrap(),
            absolute,
            "",
            "files/a.png"
        ]
    );

    let mut binary = Body::Binary {
        file: "data.bin".into(),
    };
    binary.resolve_files(collection);
    assert_eq!(
        binary,
        Body::Binary {
            file: collection.join("data.bin")
        }
    );
}
