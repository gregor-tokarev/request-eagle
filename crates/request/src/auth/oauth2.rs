//! New OAuth 2.0 access tokens (RFC 6749) from an authorization server. The
//! authorization code grant sends the user to sign in with their browser,
//! which then returns to a loopback address that the app listens on.

use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    pin::Pin,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bytes::Bytes;
use serde_json::Value;
use smol::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
};
use url::{Host, Url};

use super::credentials::basic;
use super::crypto::{random_token, sha256};
use super::{Auth, OAuth2Auth, OAuth2ClientAuthentication, OAuth2Grant};
use crate::{Body, Field, HttpRequest, Method, RequestExecutor, RequestVariables, StatusCode};

/// How long to wait for the browser to return after signing in.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// An access token the authorization server issued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuth2Token {
    pub access_token: String,
    /// Seconds until it expires, when the server says.
    pub expires_in: Option<u64>,
}

/// A request for a new access token. Dropping it stops waiting.
pub struct OAuth2TokenRequest {
    /// For the authorization code grant, the page to sign in at, which the
    /// caller opens in the browser.
    pub browser_url: Option<String>,
    pub token: Pin<Box<dyn Future<Output = Result<OAuth2Token, String>> + Send>>,
}

impl RequestExecutor {
    /// Ask the authorization server for a new access token with the auth's
    /// grant, after resolving its `{{variables}}`. The authorization code
    /// grant listens on the callback URL before this returns, so a port in
    /// use fails here.
    pub fn oauth2_token(
        &self,
        auth: &OAuth2Auth,
        variables: &RequestVariables,
    ) -> Result<OAuth2TokenRequest, String> {
        let Auth::OAuth2(auth) = variables.resolve_auth(&Auth::OAuth2(auth.clone()))? else {
            unreachable!("resolving keeps the kind of authorization");
        };
        if auth.token_url.trim().is_empty() {
            return Err("Enter the access token URL".into());
        }

        let executor = self.clone();
        let fields = match auth.grant_type {
            OAuth2Grant::ClientCredentials => vec![("grant_type", "client_credentials".to_owned())],
            OAuth2Grant::Password => vec![
                ("grant_type", "password".to_owned()),
                ("username", auth.username.clone()),
                ("password", auth.password.clone()),
            ],
            OAuth2Grant::AuthorizationCode => return authorization_code(executor, auth),
        };

        Ok(OAuth2TokenRequest {
            browser_url: None,
            token: Box::pin(async move { request_token(&executor, &auth, fields, true).await }),
        })
    }
}

fn authorization_code(
    executor: RequestExecutor,
    auth: OAuth2Auth,
) -> Result<OAuth2TokenRequest, String> {
    if auth.client_id.trim().is_empty() {
        return Err("Enter the client ID".into());
    }

    let callback = Url::parse(auth.callback_url.trim())
        .map_err(|_| "Enter a callback URL such as http://localhost:7777/callback".to_owned())?;
    let listeners = listen(&callback)?;

    let mut browser_url = Url::parse(auth.auth_url.trim())
        .map_err(|_| "Enter the authorization URL where you sign in".to_owned())?;
    let state = random_token(16);
    let verifier = random_token(32);
    {
        let mut query = browser_url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", auth.client_id.trim())
            .append_pair("redirect_uri", callback.as_str())
            .append_pair("state", &state);
        if !auth.scope.trim().is_empty() {
            query.append_pair("scope", auth.scope.trim());
        }
        if auth.pkce {
            query
                .append_pair(
                    "code_challenge",
                    &URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes())),
                )
                .append_pair("code_challenge_method", "S256");
        }
    }

    let token = async move {
        let code = smol::future::or(receive_code(&listeners, callback.path(), &state), async {
            smol::Timer::after(SIGN_IN_TIMEOUT).await;
            Err("Timed out waiting for the browser to return after signing in".to_owned())
        })
        .await?;
        drop(listeners);

        let mut fields = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code),
            ("redirect_uri", callback.to_string()),
        ];
        if auth.pkce {
            fields.push(("code_verifier", verifier));
        }

        request_token(&executor, &auth, fields, false).await
    };

    Ok(OAuth2TokenRequest {
        browser_url: Some(browser_url.into()),
        token: Box::pin(token),
    })
}

/// Post the grant's fields to the token endpoint, with the client's
/// credentials. A client without a secret sends its ID with the fields.
async fn request_token(
    executor: &RequestExecutor,
    auth: &OAuth2Auth,
    fields: Vec<(&str, String)>,
    scoped: bool,
) -> Result<OAuth2Token, String> {
    let mut fields: Vec<(String, String)> = fields
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    let mut headers = vec![Field::new("Accept", "application/json")];
    let client_id = auth.client_id.trim();

    if scoped && !auth.scope.trim().is_empty() {
        fields.push(("scope".into(), auth.scope.trim().to_owned()));
    }
    if auth.client_authentication == OAuth2ClientAuthentication::Header
        && !auth.client_secret.is_empty()
    {
        headers.push(Field::new(
            "Authorization",
            basic(client_id, &auth.client_secret),
        ));
    } else {
        if !client_id.is_empty() {
            fields.push(("client_id".into(), client_id.to_owned()));
        }
        if !auth.client_secret.is_empty() {
            fields.push(("client_secret".into(), auth.client_secret.clone()));
        }
    }

    let mut request = HttpRequest {
        method: Method::Post,
        path: auth.token_url.trim().to_owned(),
        headers,
        body: Some(Body::UrlEncoded { fields }),
        ..HttpRequest::default()
    }
    .prepare_for_send();
    let body = request
        .encode_body()
        .map_err(|error| error.to_string())?
        .map(Bytes::from);

    let send = executor.http.execute(&request, body, None);
    let sent = match executor.timeout {
        Some(timeout) => {
            smol::future::or(async { Some(send.await) }, async {
                smol::Timer::after(timeout).await;
                None
            })
            .await
        }
        None => Some(send.await),
    };
    let (response, _) = sent
        .ok_or_else(|| "The token endpoint did not answer in time".to_owned())?
        .map_err(|error| {
            format!(
                "Could not reach the token endpoint: {}",
                error.message_without_url()
            )
        })?;

    parse_token(response.status, &response.body)
}

/// The token in a JSON response, or the URL-encoded form some servers
/// still answer with.
fn parse_token(status: StatusCode, body: &[u8]) -> Result<OAuth2Token, String> {
    let json = serde_json::from_slice::<Value>(body).ok();
    let form: HashMap<String, String> = url::form_urlencoded::parse(body).into_owned().collect();
    let field = |name: &str| match &json {
        Some(json) => match &json[name] {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        },
        None => form.get(name).cloned(),
    };

    if status.is_success()
        && let Some(access_token) = field("access_token").filter(|token| !token.is_empty())
    {
        return Ok(OAuth2Token {
            access_token,
            expires_in: field("expires_in").and_then(|seconds| seconds.parse().ok()),
        });
    }

    Err(match field("error") {
        Some(error) => {
            let description = field("error_description")
                .map(|description| format!(": {description}"))
                .unwrap_or_default();
            format!("The authorization server refused the token request ({error}){description}")
        }
        None => format!("The authorization server answered {status} without an access token"),
    })
}

/// Listen where the browser returns: a loopback address, since only this
/// computer's browser goes there.
fn listen(callback: &Url) -> Result<Vec<TcpListener>, String> {
    let addresses: Vec<IpAddr> = match callback.host() {
        _ if callback.scheme() != "http" => Vec::new(),
        // Browsers may reach `localhost` over either protocol.
        Some(Host::Domain(domain)) if domain.eq_ignore_ascii_case("localhost") => vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ],
        Some(Host::Ipv4(address)) if address.is_loopback() => vec![IpAddr::V4(address)],
        Some(Host::Ipv6(address)) if address.is_loopback() => vec![IpAddr::V6(address)],
        _ => Vec::new(),
    };
    if addresses.is_empty() {
        return Err(
            "The callback URL must be a loopback address such as http://localhost:7777/callback, where Request Eagle receives the sign-in. Register the same URL with the authorization server"
                .into(),
        );
    }

    let port = callback.port_or_known_default().unwrap_or(80);
    let mut listeners = Vec::new();
    let mut failure = None;

    for address in addresses {
        match std::net::TcpListener::bind((address, port)).and_then(TcpListener::try_from) {
            Ok(listener) => listeners.push(listener),
            Err(error) => failure = Some(error),
        }
    }

    match failure {
        // One of the two `localhost` addresses is enough.
        Some(error) if listeners.is_empty() || error.kind() == std::io::ErrorKind::AddrInUse => {
            Err(format!("Could not listen on {callback}: {error}"))
        }
        _ => Ok(listeners),
    }
}

/// The authorization code the browser brings back to `path`. Other
/// requests, such as for an icon, are answered and ignored.
async fn receive_code(
    listeners: &[TcpListener],
    path: &str,
    state: &str,
) -> Result<String, String> {
    loop {
        let accepted = match listeners {
            [listener] => listener.accept().await,
            [first, second, ..] => smol::future::or(first.accept(), second.accept()).await,
            [] => unreachable!("listening on at least one address"),
        };
        let Ok((mut stream, _)) = accepted else {
            continue;
        };
        let Some(target) = read_target(&mut stream).await else {
            continue;
        };
        let Ok(url) = Url::parse("http://localhost").and_then(|base| base.join(&target)) else {
            continue;
        };

        if url.path() != path {
            respond(&mut stream, "404 Not Found", "Not found").await;
            continue;
        }

        let parameters: HashMap<String, String> = url.query_pairs().into_owned().collect();
        if let Some(error) = parameters.get("error") {
            let description = parameters
                .get("error_description")
                .map(|description| format!(": {description}"))
                .unwrap_or_default();
            respond(
                &mut stream,
                "200 OK",
                "Sign-in failed. Return to Request Eagle for details.",
            )
            .await;
            return Err(format!(
                "The authorization server refused to sign in ({error}){description}"
            ));
        }

        let Some(code) = parameters.get("code") else {
            respond(
                &mut stream,
                "400 Bad Request",
                "The authorization code is missing.",
            )
            .await;
            continue;
        };
        if parameters.get("state").map(String::as_str) != Some(state) {
            respond(
                &mut stream,
                "400 Bad Request",
                "This sign-in was not started by Request Eagle.",
            )
            .await;
            continue;
        }

        respond(
            &mut stream,
            "200 OK",
            "Signed in. You can close this tab and return to Request Eagle.",
        )
        .await;
        return Ok(code.clone());
    }
}

/// The request target of an HTTP request's first line, after reading its
/// head.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buffer = [0; 1024];

    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = smol::future::or(async { stream.read(&mut buffer).await.ok() }, async {
            smol::Timer::after(Duration::from_secs(10)).await;
            None
        })
        .await?;
        if read == 0 || head.len() > 16 * 1024 {
            return None;
        }
        head.extend_from_slice(&buffer[..read]);
    }

    let line = String::from_utf8_lossy(&head);
    let mut parts = line.lines().next()?.split_whitespace();
    parts.next()?;

    parts.next().map(str::to_owned)
}

async fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let page = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Request Eagle</title>\
         <body style=\"font-family: system-ui, sans-serif; margin: 4rem; text-align: center\">\
         <p>{message}</p></body>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );

    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}
