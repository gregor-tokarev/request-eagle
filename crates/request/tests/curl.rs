//! cURL snippets run by cURL itself. A local server must receive from each
//! command what it receives when Request Eagle sends the same request.

#![cfg(unix)]

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use md5::{Digest, Md5};
use request::{
    ApiKeyAuth, Auth, AuthLocation, AwsSignatureAuth, BearerAuth, Body, CookieJar, Field, FormPart,
    HttpRequest, HttpSettings, HttpVersion, JwtAuth, Method, OAuth1Auth, OAuth2Auth, PasswordAuth,
    RawLanguage, RequestExecutor, RequestPreferences, RequestVariables,
};

/// A request as the server read it.
#[derive(Clone, Debug)]
struct Received {
    method: String,
    target: String,
    version: String,
    /// Names in lowercase, in the order they arrived.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The response to a request: its status line, headers and body.
type Respond = fn(&Received) -> String;

fn ok(_: &Received) -> String {
    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".into()
}

/// Records the requests it reads and answers each on its own connection.
struct Server {
    url: String,
    received: Arc<Mutex<Vec<Received>>>,
}

impl Server {
    fn start(respond: Respond) -> Self {
        Self::listen("http", respond, |stream, received, respond| {
            serve(stream, &received, respond)
        })
    }

    /// Over TLS, with a certificate that no one trusts.
    fn start_tls(respond: Respond) -> Self {
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
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let config = Arc::new(config);

        Self::listen("https", respond, move |stream, received, respond| {
            let connection = rustls::ServerConnection::new(config.clone()).unwrap();
            serve(
                rustls::StreamOwned::new(connection, stream),
                &received,
                respond,
            )
        })
    }

    fn listen(
        scheme: &str,
        respond: Respond,
        handle: impl Fn(TcpStream, Arc<Mutex<Vec<Received>>>, Respond) + Send + Clone + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("{scheme}://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let recorded = received.clone();

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let (handle, recorded) = (handle.clone(), recorded.clone());
                thread::spawn(move || handle(stream, recorded, respond));
            }
        });

        Self { url, received }
    }

    /// The requests read since the last call.
    fn take(&self) -> Vec<Received> {
        std::mem::take(&mut self.received.lock().unwrap())
    }
}

/// Reads one request, records it and answers it, then closes.
fn serve(stream: impl Read + Write, received: &Mutex<Vec<Received>>, respond: Respond) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }

    let mut words = line.split_whitespace().map(str::to_owned);
    let (method, target, version) = (
        words.next().unwrap_or_default(),
        words.next().unwrap_or_default(),
        words.next().unwrap_or_default(),
    );
    let mut request = Received {
        method,
        target,
        version,
        headers: Vec::new(),
        body: Vec::new(),
    };

    // An HTTP/2 connection without negotiation starts with this line.
    if request.method == "PRI" {
        received.lock().unwrap().push(request);
        return;
    }

    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }

        let (name, value) = line.split_once(':').unwrap();
        request
            .headers
            .push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }

    if request.header("expect") == Some("100-continue") {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .unwrap();
    }

    if let Some(length) = request.header("content-length") {
        let mut body = vec![0; length.parse().unwrap()];
        reader.read_exact(&mut body).unwrap();
        request.body = body;
    } else if request.header("transfer-encoding") == Some("chunked") {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).unwrap();
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk).unwrap();
            if size == 0 {
                break;
            }
            request.body.extend_from_slice(&chunk[..size]);
        }
    }

    received.lock().unwrap().push(request.clone());
    let response = respond(&request);
    let stream = reader.get_mut();
    let _ = stream.write_all(
        response
            .replacen("\r\n", "\r\nConnection: close\r\n", 1)
            .as_bytes(),
    );
    let _ = stream.flush();
}

/// What the server received when Request Eagle sent the request, and when
/// its cURL command ran. `{{server}}` is the server's URL.
struct Sent {
    command: String,
    by_request_eagle: Vec<Received>,
    by_curl: Vec<Received>,
    sent: bool,
    curl: Output,
}

fn send(
    server: &Server,
    request: &HttpRequest,
    values: &[(&str, &str)],
    preferences: &RequestPreferences,
    jar: Option<&CookieJar>,
) -> Sent {
    let values: HashMap<String, String> = values
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .chain([("server".into(), server.url.clone())])
        .collect();
    // Written before sending, which may change the jar.
    let command = request.curl_command(&values, jar, preferences);

    let mut executor = RequestExecutor::new(preferences).unwrap();
    if let Some(jar) = jar {
        executor = executor.with_cookie_jar(jar.clone());
    }
    let sent = smol::block_on(
        executor.execute(request.clone(), RequestVariables::new(values.clone(), None)),
    )
    .is_ok();
    let by_request_eagle = server.take();

    let curl = run(&command);
    let by_curl = server.take();

    Sent {
        command,
        by_request_eagle,
        by_curl,
        sent,
        curl,
    }
}

/// Runs a command in a shell without the user's cURL configuration and
/// proxies.
fn run(command: &str) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut shell = Command::new("sh");
    shell
        .arg("-c")
        .arg(command)
        .env("HOME", home.path())
        .env("CURL_HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .stdin(Stdio::null());
    for proxy in [
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        shell.env_remove(proxy);
    }

    shell.output().unwrap()
}

/// The parts of a request that must match, readable in a failure. cURL
/// leaves out Accept-Encoding, so it prints responses it could not decode
/// as they arrive. A multipart form's boundary is chosen when it is sent,
/// and cURL encodes a few characters of URL-encoded forms differently.
fn comparable(received: &[Received]) -> Vec<String> {
    received
        .iter()
        .map(|request| {
            let mut headers = request.headers.clone();
            let mut body = String::from_utf8_lossy(&request.body).into_owned();

            if request.header("content-type") == Some("application/x-www-form-urlencoded") {
                body = url::form_urlencoded::parse(&request.body)
                    .map(|(name, value)| format!("{name:?} = {value:?}\n"))
                    .collect();
                headers.retain(|(name, _)| name != "content-length");
            }

            let boundary = request
                .header("content-type")
                .and_then(|value| value.split_once("boundary="))
                .map(|(_, boundary)| boundary.to_owned());
            if let Some(boundary) = boundary {
                body = body.replace(&boundary, "BOUNDARY");
                headers.retain(|(name, _)| name != "content-length");
                for (_, value) in &mut headers {
                    *value = value.replace(&boundary, "BOUNDARY");
                }
            }

            headers.retain(|(name, _)| name != "accept-encoding");
            headers.sort();
            let headers: Vec<_> = headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}"))
                .collect();

            format!(
                "{} {} {}\n{}\n\n{body}",
                request.method,
                request.target,
                request.version,
                headers.join("\n")
            )
        })
        .collect()
}

/// Asserts that the command ran and its requests match those sent.
fn assert_same(sent: &Sent) {
    assert!(sent.sent, "Request Eagle did not send {}", sent.command);
    assert!(
        sent.curl.status.success(),
        "{}\n{}",
        sent.command,
        String::from_utf8_lossy(&sent.curl.stderr)
    );
    assert!(!sent.by_request_eagle.is_empty(), "{}", sent.command);
    assert_eq!(
        comparable(&sent.by_request_eagle),
        comparable(&sent.by_curl),
        "{}",
        sent.command
    );
}

fn check(server: &Server, request: HttpRequest, values: &[(&str, &str)]) -> Sent {
    let sent = send(
        server,
        &request,
        values,
        &RequestPreferences::default(),
        None,
    );
    assert_same(&sent);
    sent
}

fn get(path: &str) -> HttpRequest {
    HttpRequest {
        path: path.into(),
        ..HttpRequest::default()
    }
}

#[test]
fn urls_reach_the_same_target() {
    let server = Server::start(ok);
    let disabled = Field {
        enabled: false,
        ..Field::new("off", "1")
    };

    check(
        &server,
        HttpRequest {
            query: vec![
                Field::new("q", "fish & chips"),
                Field::new("empty", ""),
                Field::new("q", "{{term}}"),
                Field::new("ünï", "çødé"),
                disabled,
            ],
            ..get("{{server}}/search?lang=en&lang=fr#results")
        },
        &[("term", "a+b=c")],
    );
    check(
        &server,
        HttpRequest {
            path_variables: vec![
                ("user".into(), "{{user}}".into()),
                ("file".into(), "a b#c".into()),
            ],
            ..get("{{server}}/users/:user/files/:file")
        },
        &[("user", "42")],
    );
    for path in [
        "{{server}}/a path/ünïcode?filter[name]=Rex&sort={desc}",
        "{{server}}/it's \"quoted\"?a='b'",
        "{{server}}/?q=hello world&x={{term}}",
        "{{server}}",
    ] {
        check(&server, get(path), &[("term", "a&b c")]);
    }
}

#[test]
fn headers_and_the_client_name_match() {
    let server = Server::start(ok);

    let sent = check(
        &server,
        HttpRequest {
            headers: vec![
                Field::new("X-Empty", ""),
                Field::new("X-Twice", "1"),
                Field::new("X-Twice", "2"),
                Field::new("X-Quote", "it's {{name}}"),
                Field::new("Accept", "application/json"),
                Field {
                    enabled: false,
                    ..Field::new("X-Off", "1")
                },
            ],
            ..get("{{server}}/headers")
        },
        &[("name", "Rex")],
    );
    // cURL would name itself, which some servers turn away.
    assert_eq!(
        sent.by_curl[0].header("user-agent"),
        Some(request::USER_AGENT)
    );

    let sent = check(
        &server,
        HttpRequest {
            headers: vec![
                Field::new("user-agent", "Mozilla/5.0"),
                Field::new("Host", "virtual.example"),
            ],
            ..get("{{server}}/headers")
        },
        &[],
    );
    assert_eq!(sent.by_curl[0].header("user-agent"), Some("Mozilla/5.0"));
}

#[test]
fn every_method_is_sent_with_and_without_a_body() {
    let server = Server::start(ok);
    let methods = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    for method in methods {
        for body in [None, Some(Body::json(r#"{"name": "Rex"}"#))] {
            check(
                &server,
                HttpRequest {
                    method,
                    body,
                    ..get("{{server}}/items")
                },
                &[],
            );
        }
    }
}

#[test]
fn every_body_is_sent_as_the_same_bytes() {
    let server = Server::start(ok);
    let files = tempfile::tempdir().unwrap();
    let file = |name: &str, contents: &[u8]| {
        let path = files.path().join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().into_owned()
    };
    let image = file("eagle's \"nest\";1.png", b"\x89PNG\r\n\x1a\n\0\xff");
    let json = file("data.json", br#"{"a": 1}"#);
    let unknown = file("data.unknownext", b"bytes");

    let bodies = [
        Body::json("{\n  \"name\": \"{{name}}\",\n  \"quote\": \"it's\"\n}"),
        Body::Raw {
            language: RawLanguage::Text,
            text: "@not-a-file\nline two\r\nline three".into(),
        },
        Body::Raw {
            language: RawLanguage::Xml,
            text: "<pet name=\"{{name}}\">ünï</pet>".into(),
        },
        Body::Raw {
            language: RawLanguage::Text,
            text: String::new(),
        },
        Body::UrlEncoded {
            fields: vec![
                ("pet name".into(), "{{name}}".into()),
                ("symbols".into(), "~!*()'@#$&+/=?%".into()),
                ("".into(), "nameless".into()),
                ("ünï".into(), "çødé".into()),
                ("@at".into(), "@value".into()),
            ],
        },
        Body::UrlEncoded { fields: Vec::new() },
        Body::Multipart {
            parts: vec![
                FormPart {
                    name: "title".into(),
                    value: "@{{name}};type=text/html,x".into(),
                    file: false,
                },
                FormPart {
                    name: "image".into(),
                    value: image.clone(),
                    file: true,
                },
                FormPart {
                    name: "json".into(),
                    value: json.clone(),
                    file: true,
                },
                FormPart {
                    name: "unknown".into(),
                    value: unknown.clone(),
                    file: true,
                },
            ],
        },
        Body::Binary {
            file: image.clone().into(),
        },
        Body::Binary {
            file: json.clone().into(),
        },
        Body::Binary {
            file: unknown.into(),
        },
    ];

    for body in bodies {
        for method in [Method::Post, Method::Put] {
            check(
                &server,
                HttpRequest {
                    method,
                    body: Some(body.clone()),
                    ..get("{{server}}/upload")
                },
                &[("name", "Rex & \"co\"")],
            );
        }
    }

    // A multipart type written without its boundary gets it.
    check(
        &server,
        HttpRequest {
            method: Method::Post,
            headers: vec![Field::new("Content-Type", "multipart/form-data")],
            body: Some(Body::Multipart {
                parts: vec![FormPart {
                    name: "title".into(),
                    value: "Rex".into(),
                    file: false,
                }],
            }),
            ..get("{{server}}/upload")
        },
        &[],
    );

    // A file's own Content-Type takes precedence.
    check(
        &server,
        HttpRequest {
            method: Method::Post,
            headers: vec![Field::new("Content-Type", "application/octet-stream")],
            body: Some(Body::Binary {
                file: Path::new(&json).into(),
            }),
            ..get("{{server}}/upload")
        },
        &[],
    );
}

#[test]
fn authorization_is_sent_the_same_way() {
    let server = Server::start(ok);
    let auths = [
        Auth::Basic(PasswordAuth {
            username: "{{user}}".into(),
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
        Auth::ApiKey(ApiKeyAuth {
            key: "api key".into(),
            value: "{{token}}".into(),
            add_to: AuthLocation::Query,
        }),
        Auth::OAuth2(Box::new(OAuth2Auth {
            access_token: "{{token}}".into(),
            header_prefix: "Bearer".into(),
            ..OAuth2Auth::default()
        })),
        Auth::OAuth2(Box::new(OAuth2Auth {
            access_token: "{{token}}".into(),
            add_to: AuthLocation::Query,
            ..OAuth2Auth::default()
        })),
        Auth::Jwt(Box::new(JwtAuth {
            secret: "{{token}}".into(),
            payload: r#"{"sub": "{{user}}"}"#.into(),
            ..JwtAuth::default()
        })),
    ];

    for auth in auths {
        check(
            &server,
            HttpRequest {
                method: Method::Post,
                body: Some(Body::json("{}")),
                auth,
                ..get("{{server}}/private?page=1")
            },
            &[("user", "eagle"), ("token", "s3cr3t+/=")],
        );
    }

    // Credentials in the URL.
    let url = server.url.replace("://", "://us%40er:p%3Ass@");
    check(&server, get(&format!("{url}/private")), &[]);
}

#[test]
fn signed_authorization_differs_only_in_what_each_send_signs() {
    let server = Server::start(ok);
    let aws = |service: &str, add_to| {
        Auth::AwsSignature(Box::new(AwsSignatureAuth {
            access_key: "AKID".into(),
            secret_key: "{{token}}".into(),
            session_token: "session".into(),
            region: "us-east-1".into(),
            service: service.into(),
            add_to,
        }))
    };
    let oauth1 = |add_to| {
        Auth::OAuth1(Box::new(OAuth1Auth {
            consumer_key: "key".into(),
            consumer_secret: "{{token}}".into(),
            access_token: "token".into(),
            token_secret: "token secret".into(),
            add_to,
            ..OAuth1Auth::default()
        }))
    };
    // Each signature covers its time and a nonce. cURL signs AWS requests
    // itself, also with the headers it adds and the form as it encodes it.
    let unsigned = |received: &[Received]| {
        let mut received = received.to_vec();
        for request in &mut received {
            let (path, query) = request.target.split_once('?').unwrap();
            let query: Vec<_> = query
                .split('&')
                .filter(|pair| {
                    let name = pair.split('=').next().unwrap();
                    !["oauth_nonce", "oauth_timestamp", "oauth_signature"].contains(&name)
                        && !["X-Amz-Date", "X-Amz-Signature"].contains(&name)
                })
                .collect();
            request.target = format!("{path}?{}", query.join("&"));

            request.headers.retain(|(name, _)| {
                !["x-amz-date", "x-amz-content-sha256"].contains(&name.as_str())
            });
            for (name, value) in &mut request.headers {
                if name == "authorization" {
                    // The scheme, and the credentials that AWS signs with.
                    *value = value
                        .split([',', ' '])
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if value.starts_with("OAuth") {
                        *value = "OAuth".into();
                    }
                }
            }
        }
        comparable(&received)
    };

    for auth in [
        aws("execute-api", AuthLocation::Header),
        aws("s3", AuthLocation::Header),
        aws("execute-api", AuthLocation::Query),
        oauth1(AuthLocation::Header),
        oauth1(AuthLocation::Query),
    ] {
        for body in [
            None,
            Some(Body::json(r#"{"a": 1}"#)),
            Some(Body::UrlEncoded {
                fields: vec![("a b".into(), "c~d*".into())],
            }),
        ] {
            let request = HttpRequest {
                method: Method::Post,
                body,
                auth: auth.clone(),
                ..get("{{server}}/private?page=1")
            };
            let sent = send(
                &server,
                &request,
                &[("token", "secret")],
                &RequestPreferences::default(),
                None,
            );

            assert!(sent.sent && sent.curl.status.success(), "{}", sent.command);
            assert_eq!(
                unsigned(&sent.by_request_eagle),
                unsigned(&sent.by_curl),
                "{}",
                sent.command
            );
        }
    }
}

/// Asks for Digest credentials, then accepts any.
fn digest_challenge(request: &Received) -> String {
    if request.header("authorization").is_none() {
        "HTTP/1.1 401 Unauthorized\r\n\
         WWW-Authenticate: Digest realm=\"eagle\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", qop=\"auth\", algorithm=MD5\r\n\
         Content-Length: 0\r\n\r\n"
            .into()
    } else {
        ok(request)
    }
}

#[test]
fn digest_authorization_answers_the_challenge() {
    let server = Server::start(digest_challenge);
    let request = HttpRequest {
        auth: Auth::Digest(PasswordAuth {
            username: "eagle".into(),
            password: "secret".into(),
        }),
        ..get("{{server}}/private?page=1")
    };
    let sent = send(&server, &request, &[], &RequestPreferences::default(), None);
    assert!(sent.curl.status.success(), "{}", sent.command);

    // Each client chooses its own nonce, so each answer is checked.
    for requests in [&sent.by_request_eagle, &sent.by_curl] {
        assert_eq!(requests.len(), 2, "{}", sent.command);
        let answer = requests[1].header("authorization").unwrap();
        let field = |name: &str| {
            answer
                .split(", ")
                .find_map(|field| field.trim_start_matches("Digest ").strip_prefix(name))
                .and_then(|value| value.strip_prefix('='))
                .map(|value| value.trim_matches('"'))
                .unwrap_or_else(|| panic!("{name} is missing from {answer}"))
        };
        let md5 = |text: String| format!("{:x}", Md5::digest(text));
        let first = md5("eagle:eagle:secret".to_owned());
        let second = md5(format!("GET:{}", field("uri")));
        let expected = md5(format!(
            "{first}:{}:{}:{}:auth:{second}",
            field("nonce"),
            field("nc"),
            field("cnonce")
        ));

        assert_eq!(field("uri"), "/private?page=1");
        assert_eq!(field("response"), expected, "{answer}");
    }
}

/// Sets cookies at `/login`.
fn log_in(request: &Received) -> String {
    if request.target == "/login" {
        "HTTP/1.1 200 OK\r\n\
         Set-Cookie: sid=abc; Path=/\r\n\
         Set-Cookie: theme=dark; Path=/private\r\n\
         Content-Length: 0\r\n\r\n"
            .into()
    } else {
        ok(request)
    }
}

#[test]
fn the_jar_cookies_join_the_request_cookies() {
    let server = Server::start(log_in);
    let jar = CookieJar::new();
    let executor = RequestExecutor::new(&RequestPreferences::default())
        .unwrap()
        .with_cookie_jar(jar.clone());
    smol::block_on(executor.execute(
        get(&format!("{}/login", server.url)),
        RequestVariables::new(HashMap::new(), None),
    ))
    .unwrap();
    assert_eq!(jar.cookies().len(), 2);
    server.take();

    for headers in [vec![], vec![Field::new("Cookie", "own=typed")]] {
        for send_cookies in [true, false] {
            let request = HttpRequest {
                headers: headers.clone(),
                settings: HttpSettings {
                    send_cookies,
                    ..HttpSettings::default()
                },
                ..get("{{server}}/private/page")
            };
            let sent = send(
                &server,
                &request,
                &[],
                &RequestPreferences::default(),
                Some(&jar),
            );
            assert_same(&sent);
        }
    }
}

/// Sends `/from/{status}` to `/to`.
fn redirect(request: &Received) -> String {
    match request.target.strip_prefix("/from/") {
        Some(status) => format!(
            "HTTP/1.1 {status} Moved\r\nLocation: /to?from={status}\r\nContent-Length: 0\r\n\r\n"
        ),
        None => ok(request),
    }
}

#[test]
fn redirects_are_followed_the_same_way() {
    let server = Server::start(redirect);
    let without_referer = |mut sent: Sent| {
        // Sending names the page it was sent from, as browsers do.
        for request in &mut sent.by_request_eagle {
            request.headers.retain(|(name, _)| name != "referer");
        }
        // cURL keeps the headers it was given when a redirect leaves out
        // the body.
        for request in sent.by_curl.iter_mut().skip(1) {
            if request.body.is_empty() {
                request.headers.retain(|(name, _)| name != "content-type");
            }
        }
        assert_same(&sent);
    };

    for status in [301, 302, 303, 307, 308] {
        for (method, body) in [
            (Method::Get, None),
            (Method::Post, Some(Body::json(r#"{"a": 1}"#))),
        ] {
            let request = HttpRequest {
                method,
                body,
                ..get(&format!("{{{{server}}}}/from/{status}"))
            };
            without_referer(send(
                &server,
                &request,
                &[],
                &RequestPreferences::default(),
                None,
            ));
        }
    }

    // Unless the request or the preferences say otherwise.
    let request = get("{{server}}/from/302");
    let not_following = RequestPreferences {
        follow_all_redirects: false,
        ..RequestPreferences::default()
    };
    let sent = send(&server, &request, &[], &not_following, None);
    assert_same(&sent);
    assert_eq!(sent.by_curl.len(), 1);

    let request = HttpRequest {
        settings: HttpSettings {
            follow_redirects: Some(true),
            ..HttpSettings::default()
        },
        ..request
    };
    without_referer(send(&server, &request, &[], &not_following, None));
}

#[test]
fn certificate_checks_follow_the_settings_and_preferences() {
    let server = Server::start_tls(ok);
    let request = get("{{server}}/secure");
    let unchecked = RequestPreferences {
        ssl_certificate_verification: false,
        ..RequestPreferences::default()
    };

    // Nobody trusts the server's certificate.
    let sent = send(&server, &request, &[], &RequestPreferences::default(), None);
    assert!(
        !sent.sent && !sent.curl.status.success(),
        "{}",
        sent.command
    );

    let sent = send(&server, &request, &[], &unchecked, None);
    assert_same(&sent);

    let request = HttpRequest {
        settings: HttpSettings {
            verify_certificates: Some(false),
            ..HttpSettings::default()
        },
        ..request
    };
    let sent = send(&server, &request, &[], &RequestPreferences::default(), None);
    assert_same(&sent);
}

#[test]
fn the_http_version_follows_the_preferences() {
    let server = Server::start(ok);
    let request = get("{{server}}/version");

    for http_version in [HttpVersion::Auto, HttpVersion::Http1_1] {
        let preferences = RequestPreferences {
            http_version,
            ..RequestPreferences::default()
        };
        let sent = send(&server, &request, &[], &preferences, None);
        assert_same(&sent);
    }

    // HTTP/2 starts without asking, which this server does not answer.
    let preferences = RequestPreferences {
        http_version: HttpVersion::Http2,
        ..RequestPreferences::default()
    };
    let sent = send(&server, &request, &[], &preferences, None);
    assert_eq!(
        comparable(&sent.by_request_eagle),
        comparable(&sent.by_curl),
        "{}",
        sent.command
    );
    assert_eq!(sent.by_curl[0].version, "HTTP/2.0", "{}", sent.command);
}

/// Answers after the clients have given up.
fn slow(request: &Received) -> String {
    thread::sleep(Duration::from_secs(2));
    ok(request)
}

#[test]
fn timeouts_follow_the_settings_and_preferences() {
    let server = Server::start(slow);
    let request = get("{{server}}/slow");
    let impatient = RequestPreferences {
        timeout_ms: 300,
        ..RequestPreferences::default()
    };

    let sent = send(&server, &request, &[], &impatient, None);
    assert!(
        !sent.sent && !sent.curl.status.success(),
        "{}",
        sent.command
    );

    let request = HttpRequest {
        settings: HttpSettings {
            timeout_ms: Some(0),
            ..HttpSettings::default()
        },
        ..request
    };
    let sent = send(&server, &request, &[], &impatient, None);
    assert_same(&sent);
}
