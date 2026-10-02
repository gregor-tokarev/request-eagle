use std::collections::HashMap;

use request::{
    Auth, AwsSignatureAuth, BearerAuth, Body, FormPart, HttpRequest, HttpSettings, Method,
    PasswordAuth, RawLanguage,
};

use crate::{CurlError, is_curl, parse_curl};

/// The text of a raw body.
fn raw(request: &HttpRequest) -> Option<&str> {
    match &request.body {
        Some(Body::Raw { text, .. }) => Some(text),
        _ => None,
    }
}

fn part(name: &str, value: &str, file: bool) -> FormPart {
    FormPart {
        name: name.into(),
        value: value.into(),
        file,
    }
}

fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

#[test]
fn recognizes_curl_commands_but_not_urls() {
    assert!(is_curl("curl https://example.com"));
    assert!(is_curl("  curl\\\n  https://example.com"));
    assert!(!is_curl("curl"));
    assert!(!is_curl("curl.se/docs"));
    assert!(!is_curl("https://example.com/curl"));
    assert_eq!(parse_curl("https://example.com"), Err(CurlError::NotCurl));
}

#[test]
fn reads_a_command_written_over_several_lines() {
    let request = parse_curl(
        "curl --location --request PUT 'https://api.example.com/pets/1?verbose=true' \\\n\
         --header 'Content-Type: application/json' \\\n\
         --header \"Authorization: Bearer abc\" \\\n\
         --data '{\n    \"name\": \"Rex\"\n}'",
    )
    .unwrap();

    assert_eq!(
        request,
        HttpRequest {
            method: Method::Put,
            path: "https://api.example.com/pets/1?verbose=true".into(),
            headers: headers(&[
                ("Content-Type", "application/json"),
                ("Authorization", "Bearer abc"),
            ]),
            body: Some(Body::json("{\n    \"name\": \"Rex\"\n}")),
            ..HttpRequest::default()
        }
    );
}

#[test]
fn reads_browser_copies_with_ansi_c_quoting() {
    // Chrome writes bodies with control characters or quotes as $'…'.
    let request = parse_curl(
        "curl 'https://example.com/api' \\\r\n  -H 'accept: */*' \\\r\n  -H 'content-type: text/plain' \\\r\n  --data-raw $'it\\'s\\n\\u00e9 \\xc3\\xa9 \\101' \\\r\n  --compressed",
    )
    .unwrap();

    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.headers,
        headers(&[("accept", "*/*"), ("content-type", "text/plain")])
    );
    assert_eq!(
        request.body,
        Some(Body::Raw {
            language: RawLanguage::Text,
            text: "it's\né é A".into(),
        })
    );
}

#[test]
fn reads_combined_and_attached_short_options() {
    let request = parse_curl("curl -sSLXPATCH -HAccept:text/html -d x=1 example.com/a").unwrap();

    assert_eq!(request.method, Method::Patch);
    assert_eq!(request.path, "http://example.com/a");
    assert_eq!(
        request.headers,
        headers(&[
            ("Accept", "text/html"),
            ("Content-Type", "application/x-www-form-urlencoded"),
        ])
    );
    assert_eq!(raw(&request), Some("x=1"));
}

#[test]
fn skips_values_of_options_that_do_not_change_the_request() {
    let request = parse_curl(
        "curl -o out.json --max-time 10 -w '%{http_code}' --url https://example.com/x --retry 3",
    )
    .unwrap();

    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "https://example.com/x");
    assert!(request.headers.is_empty());
    assert!(request.body.is_none());
}

#[test]
fn ends_the_command_at_shell_operators() {
    let request =
        parse_curl("curl https://example.com/a?b=1&c=2 -H 'X: 1' | jq -r '.name' > out.json")
            .unwrap();

    assert_eq!(request.path, "https://example.com/a?b=1&c=2");
    assert_eq!(request.headers, headers(&[("X", "1")]));
}

#[test]
fn joins_data_and_sends_it_in_the_query_with_get() {
    let request = parse_curl(
        "curl -d a=1 --data-urlencode 'q=hello world&more' --data-urlencode =x/y https://example.com",
    )
    .unwrap();
    assert_eq!(raw(&request), Some("a=1&q=hello%20world%26more&x%2Fy"));

    let request = parse_curl("curl -G -d a=1 -d b=2 'https://example.com/s?x=0'").unwrap();
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "https://example.com/s?x=0&a=1&b=2");
    assert!(request.body.is_none());
    assert!(request.headers.is_empty());
}

#[test]
fn reads_json_forms_and_credentials() {
    let request = parse_curl(r#"curl --json '{"a":1}' https://example.com"#).unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.headers,
        headers(&[
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
        ])
    );

    let request = parse_curl(
        "curl -u user:secret -A agent -e https://ref.example -b 'a=1' --cookie b=2 \
         -F name=Rex -F 'note=a=b' https://example.com",
    )
    .unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.headers,
        headers(&[
            ("User-Agent", "agent"),
            ("Referer", "https://ref.example"),
            ("Cookie", "a=1; b=2"),
        ])
    );
    assert_eq!(
        request.auth,
        Auth::Basic(PasswordAuth {
            username: "user".into(),
            password: "secret".into(),
        })
    );
    assert_eq!(
        request.body,
        Some(Body::Multipart {
            parts: vec![part("name", "Rex", false), part("note", "a=b", false)],
        })
    );
}

#[test]
fn reads_header_forms_and_head_requests() {
    let request =
        parse_curl("curl -I -H 'Empty;' -H 'Removed:' -H 'X-Pad:   v  ' localhost:3000").unwrap();

    assert_eq!(request.method, Method::Head);
    assert_eq!(request.path, "http://localhost:3000");
    assert_eq!(request.headers, headers(&[("Empty", ""), ("X-Pad", "v")]));
}

#[test]
fn explains_what_it_cannot_import() {
    assert_eq!(
        parse_curl("curl 'https://example.com"),
        Err(CurlError::UnclosedQuote)
    );
    assert_eq!(
        parse_curl("curl -X"),
        Err(CurlError::MissingValue("-X".into()))
    );
    assert_eq!(parse_curl("curl -s -L"), Err(CurlError::MissingUrl));
    assert_eq!(
        parse_curl("curl -X TRACE https://example.com"),
        Err(CurlError::UnsupportedMethod("TRACE".into()))
    );
    assert_eq!(
        parse_curl("curl -F 'notes=<notes.txt' https://example.com"),
        Err(CurlError::File("--form notes=<notes.txt".into()))
    );
    // `--data` leaves out line breaks of the file it reads.
    assert_eq!(
        parse_curl("curl --data @body.json https://example.com"),
        Err(CurlError::File("--data @body.json".into()))
    );
    assert_eq!(
        parse_curl("curl --data-binary @a.bin --data-binary @b.bin https://example.com"),
        Err(CurlError::File(
            "--data-binary @b.bin with other data".into()
        ))
    );
    assert_eq!(
        parse_curl("curl -d a=1 -F b=2 https://example.com"),
        Err(CurlError::FormAndData)
    );
    // Request Eagle leaves out GET bodies, so importing would change the request.
    assert_eq!(
        parse_curl("curl -X GET -d '{\"query\":{}}' https://example.com/_search"),
        Err(CurlError::BodyWithoutMethod("GET"))
    );
}

#[test]
fn reads_values_attached_to_long_options() {
    let request = parse_curl("curl --request=PATCH --data-raw=x=1 https://example.com").unwrap();

    assert_eq!(request.method, Method::Patch);
    assert_eq!(raw(&request), Some("x=1"));
}

#[test]
fn joins_data_as_curl_does() {
    // `--json` continues the previous data, while `--data` adds a field.
    let request = parse_curl("curl --json '{\"a\":' --json '1}' https://example.com").unwrap();
    assert_eq!(request.body, Some(Body::json("{\"a\":1}")));

    // `--get` adds the query before the fragment, which is not sent.
    let request = parse_curl("curl -G -d a=1 'https://example.com/path#top'").unwrap();
    assert_eq!(request.path, "https://example.com/path?a=1#top");
}

#[test]
fn reads_form_fields_and_their_content_type() {
    let request = parse_curl(
        "curl -H 'Content-Type: multipart/form-data' -F 'name=Rex;type=text/plain' \
         -F 'note=\"a;b \\\"c\\\"\"' --form-string 'raw=x;y' https://example.com",
    )
    .unwrap();

    // Sending adds the boundary.
    assert_eq!(
        request.headers,
        headers(&[("Content-Type", "multipart/form-data")])
    );
    assert_eq!(
        request.body,
        Some(Body::Multipart {
            parts: vec![
                part("name", "Rex", false),
                part("note", "a;b \"c\"", false),
                part("raw", "x;y", false),
            ],
        })
    );

    // Without its closing quote, cURL sends the value as it is written.
    let request = parse_curl("curl -F 'name=\"Rex' https://example.com").unwrap();
    assert_eq!(
        request.body,
        Some(Body::Multipart {
            parts: vec![part("name", "\"Rex", false)],
        })
    );
}

#[test]
fn reads_the_files_forms_and_data_send() {
    let request = parse_curl(
        "curl -F 'avatar=@\"my;photo,1.jpg\";type=image/jpeg' -F 'doc=@notes.txt;filename=n' \
         -F 'files=@a.txt,b.txt;type=text/plain' \
         -F 'named=@c.txt;filename=\"client,copy.txt\"' -F 'quote=@a\"b.txt,b.txt' \
         -F 'spaced=@d.txt;filename= \"e,f.txt\"' -F 'listed=@ \"g,h.txt\" , i.txt' \
         -F 'unclosed=@j.txt;filename=\"k,l.txt' https://example.com",
    )
    .unwrap();
    assert_eq!(
        request.body,
        Some(Body::Multipart {
            parts: vec![
                part("avatar", "my;photo,1.jpg", true),
                part("doc", "notes.txt", true),
                part("files", "a.txt", true),
                part("files", "b.txt", true),
                part("named", "c.txt", true),
                part("quote", "a\"b.txt", true),
                part("quote", "b.txt", true),
                part("spaced", "d.txt", true),
                part("listed", "g,h.txt", true),
                part("listed", "i.txt", true),
                part("unclosed", "j.txt", true),
                part("unclosed", "l.txt", true),
            ],
        })
    );

    let request = parse_curl(
        "curl -H 'Content-Type: image/png' --data-binary @/tmp/eagle.png https://example.com",
    )
    .unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.headers, headers(&[("Content-Type", "image/png")]));
    assert_eq!(
        request.body,
        Some(Body::Binary {
            file: "/tmp/eagle.png".into()
        })
    );
}

#[test]
fn leaves_out_headers_the_command_removes() {
    let request = parse_curl("curl --json '{}' -H 'Accept:' https://example.com").unwrap();

    assert_eq!(
        request.headers,
        headers(&[("Content-Type", "application/json")])
    );
}

#[test]
fn reads_the_commands_request_eagle_writes() {
    let requests = [
        HttpRequest {
            method: Method::Put,
            path: "https://example.com/pets?x=1".into(),
            headers: headers(&[("Content-Type", "application/json"), ("X-Empty", "")]),
            body: Some(Body::json(r#"{"it's": "@home"}"#)),
            ..HttpRequest::default()
        },
        HttpRequest {
            method: Method::Post,
            path: "https://example.com/form".into(),
            headers: headers(&[("Content-Type", "text/plain")]),
            body: Some(Body::Raw {
                language: RawLanguage::Text,
                text: "line one\nline two".into(),
            }),
            ..HttpRequest::default()
        },
        HttpRequest {
            method: Method::Post,
            path: "https://example.com/upload".into(),
            body: Some(Body::Multipart {
                parts: vec![
                    part("title", "@me;type=x", false),
                    part("avatar", "/tmp/my \"eagle\";1.png", true),
                ],
            }),
            ..HttpRequest::default()
        },
        HttpRequest {
            method: Method::Put,
            path: "https://example.com/upload".into(),
            headers: headers(&[("Content-Type", "application/octet-stream")]),
            body: Some(Body::Binary {
                file: "/tmp/eagle.bin".into(),
            }),
            ..HttpRequest::default()
        },
        HttpRequest {
            method: Method::Head,
            path: "http://localhost:3000/".into(),
            ..HttpRequest::default()
        },
    ];

    for request in requests {
        let command = request.curl_command(&HashMap::new(), None);
        assert_eq!(parse_curl(&command).as_ref(), Ok(&request), "{command}");
    }
}

#[test]
fn reads_certificate_checks_and_the_timeout_into_the_request_settings() {
    let request = parse_curl("curl -sSk -m 2.5 https://example.com").unwrap();
    assert_eq!(
        request.settings,
        HttpSettings {
            timeout_ms: Some(2500),
            follow_redirects: None,
            verify_certificates: Some(false),
        }
    );

    let request =
        parse_curl("curl --location --insecure --max-time=30 https://example.com").unwrap();
    assert_eq!(request.settings.timeout_ms, Some(30_000));
    assert_eq!(request.settings.verify_certificates, Some(false));

    // A command written from a request's settings reads back the same.
    let exported = HttpRequest {
        path: "https://example.com/".into(),
        settings: HttpSettings {
            timeout_ms: Some(1500),
            follow_redirects: None,
            verify_certificates: Some(false),
        },
        ..HttpRequest::default()
    };
    let imported = parse_curl(&exported.curl_command(&HashMap::new(), None)).unwrap();
    assert_eq!(imported.settings, exported.settings);

    assert!(
        parse_curl("curl https://example.com")
            .unwrap()
            .settings
            .is_default()
    );
}

#[test]
fn credentials_become_the_requests_authorization() {
    let auth = |command: &str| parse_curl(command).unwrap().auth;

    assert_eq!(auth("curl https://example.com"), Auth::Inherit);
    assert_eq!(
        auth("curl --digest -u user https://example.com"),
        Auth::Digest(PasswordAuth {
            username: "user".into(),
            password: String::new(),
        })
    );
    assert_eq!(
        auth("curl --oauth2-bearer t0ken https://example.com"),
        Auth::Bearer(BearerAuth {
            token: "t0ken".into()
        })
    );
    assert_eq!(
        auth("curl --aws-sigv4 aws:amz:eu-west-1:s3 --user AKID:secret https://example.com"),
        Auth::AwsSignature(AwsSignatureAuth {
            access_key: "AKID".into(),
            secret_key: "secret".into(),
            region: "eu-west-1".into(),
            service: "s3".into(),
            ..AwsSignatureAuth::default()
        })
    );

    // The commands Request Eagle writes read back to the same authorization.
    let request = HttpRequest {
        path: "https://example.com/".into(),
        auth: Auth::Digest(PasswordAuth {
            username: "user".into(),
            password: "p@ss:word".into(),
        }),
        ..HttpRequest::default()
    };
    let command = request.curl_command(&HashMap::new(), None);
    assert_eq!(parse_curl(&command).unwrap().auth, request.auth);
}
