//! Requests that authorize themselves, against local servers.

use std::collections::HashMap;

use md5::{Digest as _, Md5};
use request::{
    ApiKeyAuth, Auth, AuthKind, AuthLocation, AwsSignatureAuth, BearerAuth, Body, HttpRequest,
    Method, OAuth2Auth, OAuth2ClientAuthentication, OAuth2Grant, PasswordAuth, RequestExecutor,
    RequestPreferences, RequestVariables, Response,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct Received {
    head: String,
    body: Vec<u8>,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }

    fn target(&self) -> &str {
        self.head.split_whitespace().nth(1).unwrap()
    }
}

async fn read_request(stream: &mut TcpStream) -> Received {
    let mut head = Vec::new();

    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }

    let head = String::from_utf8(head).unwrap();
    let received = Received {
        head,
        body: Vec::new(),
    };
    let length = received
        .header("content-length")
        .map_or(0, |length| length.parse().unwrap());
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();

    Received { body, ..received }
}

/// Answer each request on its own connection with the next response, and
/// return what the server received.
async fn serve(responses: Vec<&'static str>) -> (String, smol::Task<Vec<Received>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let mut received = Vec::new();

        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            received.push(read_request(&mut stream).await);
            stream.write_all(response.as_bytes()).await.unwrap();
        }

        received
    });

    (url, server)
}

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

fn executor() -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences {
        timeout_ms: 5_000,
        ..RequestPreferences::default()
    })
    .unwrap()
}

fn variables(values: &[(&str, &str)]) -> RequestVariables {
    RequestVariables::new(
        values
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<HashMap<_, _>>(),
        None,
    )
}

fn status(response: &Response) -> u16 {
    let Response::Http(response) = response;
    response.status.as_u16()
}

#[test]
fn requests_inherit_the_collection_authorization_until_they_choose_their_own() {
    smol::block_on(async {
        let (url, server) = serve(vec![OK, OK, OK]).await;
        let collection = Auth::Bearer(BearerAuth {
            token: "{{token}}".into(),
        });
        let send = |auth: Auth| {
            let request = HttpRequest {
                path: url.clone(),
                auth,
                ..HttpRequest::default()
            };
            let variables =
                variables(&[("token", "secret")]).with_collection_auth(collection.clone());

            executor().execute(request, variables)
        };

        send(Auth::Inherit).await.unwrap();
        send(Auth::None).await.unwrap();
        send(Auth::ApiKey(ApiKeyAuth {
            key: "api_key".into(),
            value: "{{token}}".into(),
            add_to: AuthLocation::Query,
        }))
        .await
        .unwrap();

        let received = server.await;
        assert_eq!(received[0].header("authorization"), Some("Bearer secret"));
        assert_eq!(received[1].header("authorization"), None);
        assert_eq!(received[2].header("authorization"), None);
        assert_eq!(received[2].target(), "/?api_key=secret");
    });
}

#[test]
fn a_header_set_in_the_request_replaces_the_authorizations() {
    smol::block_on(async {
        let (url, server) = serve(vec![OK]).await;
        let request = HttpRequest {
            path: url,
            headers: vec![("Authorization".into(), "Token own".into())],
            auth: Auth::Basic(PasswordAuth {
                username: "user".into(),
                password: "pass".into(),
            }),
            ..HttpRequest::default()
        };

        executor().execute(request, variables(&[])).await.unwrap();

        let received = server.await;
        assert_eq!(received[0].header("authorization"), Some("Token own"));
    });
}

#[test]
fn digest_requests_answer_the_challenge_and_send_their_body_again() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"eagle\", nonce=\"n0nce\", qop=\"auth\", opaque=\"op\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            OK,
        ])
        .await;
        let request = HttpRequest {
            method: Method::Post,
            path: format!("{url}/upload?part=1"),
            body: Some(Body::json("{\"name\": \"eagle\"}")),
            auth: Auth::Digest(PasswordAuth {
                username: "{{user}}".into(),
                password: "secret".into(),
            }),
            ..HttpRequest::default()
        };

        let execution = executor()
            .execute(request, variables(&[("user", "mufasa")]))
            .await
            .unwrap();
        assert_eq!(status(&execution.response), 200);

        let received = server.await;
        assert_eq!(received[0].header("authorization"), None);
        assert_eq!(received[1].body, b"{\"name\": \"eagle\"}");

        let answer = received[1].header("authorization").unwrap();
        let cnonce = answer
            .split("cnonce=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        let md5 = |text: String| {
            Md5::digest(text.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let ha1 = md5("mufasa:eagle:secret".into());
        let ha2 = md5("POST:/upload?part=1".into());
        let response = md5(format!("{ha1}:n0nce:00000001:{cnonce}:auth:{ha2}"));
        assert_eq!(
            answer,
            format!(
                "Digest username=\"mufasa\", realm=\"eagle\", nonce=\"n0nce\", uri=\"/upload?part=1\", qop=auth, nc=00000001, cnonce=\"{cnonce}\", response=\"{response}\", opaque=\"op\""
            )
        );
    });
}

#[test]
fn aws_signatures_cover_the_request_as_it_is_sent() {
    smol::block_on(async {
        let (url, server) = serve(vec![OK]).await;
        let request = HttpRequest {
            method: Method::Put,
            path: format!("{url}/bucket/key.txt"),
            body: Some(Body::Raw {
                language: request::RawLanguage::Text,
                text: "contents".into(),
            }),
            auth: Auth::AwsSignature(AwsSignatureAuth {
                access_key: "AKID".into(),
                secret_key: "{{secret}}".into(),
                session_token: "session".into(),
                region: "eu-west-1".into(),
                service: "s3".into(),
                add_to: AuthLocation::Header,
            }),
            ..HttpRequest::default()
        };

        executor()
            .execute(request, variables(&[("secret", "s3cret")]))
            .await
            .unwrap();

        let received = server.await;
        let authorization = received[0].header("authorization").unwrap();
        assert!(
            authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKID/"),
            "{authorization}"
        );
        assert!(authorization.contains(
            "/eu-west-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token, Signature="
        ));
        // The hash of "contents".
        assert_eq!(
            received[0].header("x-amz-content-sha256"),
            Some("d1b2a59fbea7e20077af9f91b27e95e865061b270be03ff539ab3b73587882e8")
        );
        assert_eq!(received[0].header("x-amz-security-token"), Some("session"));
        assert!(received[0].header("x-amz-date").is_some());
    });
}

#[test]
fn signing_failures_stop_the_request_before_it_goes_out() {
    smol::block_on(async {
        let request = HttpRequest {
            path: "http://127.0.0.1:9/".into(),
            auth: AuthKind::AwsSignature.new_auth(),
            ..HttpRequest::default()
        };

        let error = executor()
            .execute(request, variables(&[]))
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Enter the AWS access key in the Auth tab"
        );
    });
}

#[test]
fn client_credentials_tokens_come_from_the_token_endpoint() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 44\r\nConnection: close\r\n\r\n{\"access_token\":\"t0ken\",\"expires_in\":\"3600\"}",
        ])
        .await;
        let auth = OAuth2Auth {
            grant_type: OAuth2Grant::ClientCredentials,
            token_url: format!("{url}/token"),
            client_id: "{{client}}".into(),
            client_secret: "secret".into(),
            scope: "read write".into(),
            ..OAuth2Auth::default()
        };

        let request = executor()
            .oauth2_token(&auth, &variables(&[("client", "app")]))
            .unwrap();
        assert_eq!(request.browser_url, None);
        let token = request.token.await.unwrap();
        assert_eq!(token.access_token, "t0ken");
        assert_eq!(token.expires_in, Some(3600));

        let received = server.await;
        assert!(received[0].head.starts_with("POST /token "));
        assert_eq!(
            received[0].header("authorization"),
            Some("Basic YXBwOnNlY3JldA==")
        );
        assert_eq!(received[0].header("accept"), Some("application/json"));
        assert_eq!(
            String::from_utf8_lossy(&received[0].body),
            "grant_type=client_credentials&scope=read+write"
        );
    });
}

#[test]
fn token_endpoint_errors_explain_why() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 62\r\nConnection: close\r\n\r\n{\"error\":\"invalid_grant\",\"error_description\":\"Wrong password\"}",
        ])
        .await;
        let auth = OAuth2Auth {
            grant_type: OAuth2Grant::Password,
            token_url: url,
            client_id: "app".into(),
            username: "me".into(),
            password: "pw".into(),
            client_authentication: OAuth2ClientAuthentication::Body,
            ..OAuth2Auth::default()
        };

        let error = executor()
            .oauth2_token(&auth, &variables(&[]))
            .unwrap()
            .token
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "The authorization server refused the token request (invalid_grant): Wrong password"
        );

        let received = server.await;
        assert_eq!(received[0].header("authorization"), None);
        assert_eq!(
            String::from_utf8_lossy(&received[0].body),
            "grant_type=password&username=me&password=pw&client_id=app"
        );
    });
}

#[test]
fn authorization_codes_return_to_the_callback_and_are_exchanged_with_pkce() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 30\r\nConnection: close\r\n\r\naccess_token=c0de&token_type=x",
        ])
        .await;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let callback = format!("http://127.0.0.1:{port}/callback");
        let auth = OAuth2Auth {
            auth_url: "https://example.com/authorize?audience=api".into(),
            token_url: format!("{url}/token"),
            callback_url: callback.clone(),
            client_id: "app".into(),
            scope: "openid".into(),
            ..OAuth2Auth::default()
        };

        let request = executor().oauth2_token(&auth, &variables(&[])).unwrap();
        let browser = url::Url::parse(request.browser_url.as_deref().unwrap()).unwrap();
        let query: HashMap<_, _> = browser.query_pairs().into_owned().collect();
        assert_eq!(browser.path(), "/authorize");
        assert_eq!(query["audience"], "api");
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["client_id"], "app");
        assert_eq!(query["redirect_uri"], callback);
        assert_eq!(query["scope"], "openid");
        assert_eq!(query["code_challenge_method"], "S256");

        // The browser returns after signing in, first asking for an icon.
        let state = query["state"].clone();
        let browser = smol::spawn(async move {
            let get = |target: String| async move {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                stream
                    .write_all(
                        format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).await.unwrap();
                response
            };

            assert!(get("/favicon.ico".into()).await.starts_with("HTTP/1.1 404"));
            get(format!("/callback?code=abc&state={state}")).await
        });

        let token = request.token.await.unwrap();
        assert_eq!(token.access_token, "c0de");
        assert!(browser.await.contains("Signed in"));

        let received = server.await;
        let fields: HashMap<_, _> = url::form_urlencoded::parse(&received[0].body)
            .into_owned()
            .collect();
        assert_eq!(fields["grant_type"], "authorization_code");
        assert_eq!(fields["code"], "abc");
        assert_eq!(fields["redirect_uri"], callback);
        assert_eq!(fields["client_id"], "app");

        use base64::Engine as _;
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            ring::digest::digest(&ring::digest::SHA256, fields["code_verifier"].as_bytes()),
        );
        assert_eq!(query["code_challenge"], challenge);
    });
}

#[test]
fn callbacks_must_return_to_this_computer() {
    let auth = OAuth2Auth {
        auth_url: "https://example.com/authorize".into(),
        token_url: "https://example.com/token".into(),
        callback_url: "https://oauth.example.com/callback".into(),
        client_id: "app".into(),
        ..OAuth2Auth::default()
    };

    let Err(error) = executor().oauth2_token(&auth, &variables(&[])) else {
        panic!("expected an error");
    };
    assert!(
        error.starts_with("The callback URL must be a loopback address"),
        "{error}"
    );
}

#[test]
fn curl_snippets_carry_the_authorization() {
    let request = HttpRequest {
        path: "https://example.com/items".into(),
        auth: Auth::Basic(PasswordAuth {
            username: "{{user}}".into(),
            password: "{{unknown}}".into(),
        }),
        ..HttpRequest::default()
    };
    let values = HashMap::from([("user".to_owned(), "eagle".to_owned())]);
    let command = request.curl_command(&values, None);
    assert!(
        command.ends_with("\\\n--user 'eagle:{{unknown}}'"),
        "{command}"
    );

    let command = HttpRequest {
        auth: Auth::ApiKey(ApiKeyAuth {
            key: "key".into(),
            value: "{{user}}".into(),
            add_to: AuthLocation::Query,
        }),
        ..request.clone()
    }
    .curl_command(&values, None);
    assert!(
        command.contains("'https://example.com/items?key=eagle'"),
        "{command}"
    );

    let command = HttpRequest {
        auth: AuthKind::Digest.new_auth(),
        ..request.clone()
    }
    .curl_command(&values, None);
    assert!(
        command.ends_with("\\\n--digest \\\n--user ':'"),
        "{command}"
    );

    let command = HttpRequest {
        auth: Auth::AwsSignature(AwsSignatureAuth {
            access_key: "AKID".into(),
            secret_key: "secret".into(),
            region: "us-east-1".into(),
            service: "execute-api".into(),
            ..AwsSignatureAuth::default()
        }),
        ..request
    }
    .curl_command(&values, None);
    assert!(
        command
            .ends_with("\\\n--aws-sigv4 'aws:amz:us-east-1:execute-api' \\\n--user 'AKID:secret'"),
        "{command}"
    );
}
