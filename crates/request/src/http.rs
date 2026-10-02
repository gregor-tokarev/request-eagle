use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use bytes::Bytes;
use http_client::http::{
    HeaderMap,
    header::{COOKIE, HOST},
    uri::Authority,
};
use http_client::{Request, Url};
use smol::io::AsyncReadExt;

use crate::{
    CookieJar, EventStream, ExecutionError, HttpMetrics, HttpRequest, HttpResponse, HttpVersion,
    RequestPreferences, event_stream, tls::Tls,
};

type Client = Arc<reqwest_client::ReqwestClient>;

#[derive(Clone)]
pub(crate) struct HttpExecutor {
    clients: Arc<Clients>,
    http_version: HttpVersion,
    follow_all_redirects: bool,
    verify_certificates: bool,
    max_response_bytes: Option<u64>,
    /// Whether the preferences let requests use a cookie jar.
    cookie_jar: bool,
    cookies: Option<CookieJar>,
}

/// A client for each way of connecting that requests need, built when first
/// needed and then reused with its connections.
struct Clients {
    preferences: RequestPreferences,
    built: Mutex<HashMap<ClientKey, Client>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ClientKey {
    verify: bool,
    version: HttpVersion,
    /// Whether the certificate authorities from Settings are trusted.
    ca_certificates: bool,
    /// The ID of the client certificate presented to the server.
    certificate: Option<String>,
}

impl HttpExecutor {
    /// Script subrequests share the transport but cannot allocate unbounded bodies.
    pub(crate) fn with_response_limit(&self, limit: u64) -> Self {
        let mut executor = self.clone();
        executor.max_response_bytes = Some(
            self.max_response_bytes
                .map_or(limit, |configured| configured.min(limit)),
        );
        executor
    }

    /// Store the cookies that responses set in `jar`, and send them with later
    /// requests, unless the preferences turn the cookie jar off.
    pub(crate) fn with_cookie_jar(mut self, jar: CookieJar) -> Self {
        self.cookies = self.cookie_jar.then_some(jar);
        self
    }

    /// The jar requests use, unless the preferences turn it off.
    pub(crate) fn cookie_jar(&self) -> Option<&CookieJar> {
        self.cookies.as_ref()
    }

    pub(crate) fn new(preferences: &RequestPreferences) -> Result<Self, ExecutionError> {
        let max_response_bytes = match preferences.max_response_size_mb {
            0 => None,
            megabytes => Some(
                megabytes
                    .checked_mul(1024 * 1024)
                    .ok_or(ExecutionError::InvalidResponseLimit)?,
            ),
        };

        // Report invalid proxy settings before sending. Certificate files are
        // read when a connection needs them, so one that is missing only
        // fails the requests that use it.
        let _ = preferences.proxy.apply(reqwest::Client::builder())?;

        Ok(Self {
            clients: Arc::new(Clients {
                preferences: preferences.clone(),
                built: Mutex::default(),
            }),
            http_version: preferences.http_version,
            follow_all_redirects: preferences.follow_all_redirects,
            verify_certificates: preferences.ssl_certificate_verification,
            max_response_bytes,
            cookie_jar: preferences.cookie_jar,
            cookies: None,
        })
    }

    /// Read the complete response, and the URL it came from after redirects.
    /// With `events`, an event-stream response reports its events as they
    /// arrive.
    pub(crate) async fn execute(
        &self,
        request: &HttpRequest,
        body: Option<Bytes>,
        events: Option<&mut EventStream>,
    ) -> Result<(HttpResponse, Url), ExecutionError> {
        let started = Instant::now();
        let is_head = request.method.as_str() == "HEAD";
        let verify = request
            .settings
            .verify_certificates
            .unwrap_or(self.verify_certificates);
        let follow = request
            .settings
            .follow_redirects
            .unwrap_or(self.follow_all_redirects);
        let mut url = Url::parse(&request.path).map_err(ExecutionError::InvalidUrl)?;

        if !matches!(url.scheme(), "http" | "https") {
            return Err(ExecutionError::UnsupportedScheme(url.scheme().to_owned()));
        }

        // Keep query pairs from the URL, including repeated keys, then append
        // the editor's pairs with URL encoding. Fragments are never sent.
        url.set_fragment(None);

        if !request.query.is_empty() {
            url.query_pairs_mut().extend_pairs(&request.query);
        }

        let request_body_bytes = body.as_ref().map_or(0, Bytes::len);
        let mut builder = Request::builder()
            .method(request.method.as_str())
            .uri(url.as_str());

        let generated = crate::generated_headers(
            request.method,
            url.as_str(),
            &request.headers,
            request_body_bytes,
        );
        let generated_host = generated.iter().any(|(name, _)| name == "Host");

        for (name, value) in request.headers.iter().chain(&generated) {
            builder = builder.header(name.as_str(), value.as_str());
        }

        let mut request = builder.body(body).map_err(ExecutionError::InvalidRequest)?;

        let host = validate_host(request.headers())?;
        let mut version = self.http_version;

        if self.http_version != HttpVersion::Http1_1
            && let Some(host) = host
        {
            let default_port = if url.scheme() == "https" { 443 } else { 80 };
            let same_host = url.host().is_some_and(|url_host| {
                url::Host::parse(host.host()).is_ok_and(|host| host == url_host)
            });
            let same_port = host.port_u16().unwrap_or(default_port)
                == url.port_or_known_default().unwrap_or(default_port);

            if same_host && same_port {
                // HTTP/2 already conveys this in :authority; some servers reject
                // a redundant Host. HTTP/1.1 will generate Host from the URL.
                request.headers_mut().remove(HOST);
            } else if self.http_version == HttpVersion::Auto {
                // This transport derives :authority from the connection URL.
                // Choose HTTP/1.1 before sending so custom Host routing works
                // without changing the destination/TLS name or replaying a request.
                version = HttpVersion::Http1_1;
            } else {
                return Err(ExecutionError::Http2HostOverride);
            }
        }

        let mut request_header_bytes = request
            .headers()
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
            .sum::<usize>();

        // The jar's cookies join the request's own Cookie header when it is sent.
        if let Some(cookie) = self
            .cookies
            .as_ref()
            .and_then(|jar| jar.request_header(&url, request.headers()))
        {
            let own = request
                .headers()
                .get_all(COOKIE)
                .iter()
                .map(|value| COOKIE.as_str().len() + value.as_bytes().len() + 4)
                .sum::<usize>();
            // The request's own Cookie headers are counted above and replaced.
            request_header_bytes =
                request_header_bytes - own + COOKIE.as_str().len() + cookie.as_bytes().len() + 4;
        }

        if generated_host {
            // Let the transport regenerate Host when a redirect changes the URL.
            request.headers_mut().remove(HOST);
        }

        let prepared = Instant::now();
        if let Some(events) = &events {
            events.dispatch.start();
        }
        // Each hop connects with the client certificate for its host.
        let client = |url: &Url| self.clients.get(url, verify, version);
        let (response, url) =
            crate::redirects::send(client, request, url, follow, self.cookies.as_ref()).await?;
        let received = Instant::now();
        let (parts, mut stream) = response.into_parts();
        // HEAD and statuses without a body may describe an encoded representation
        // in their headers, but there are no bytes to pass to a decoder.
        // A 206 body contains a range of the encoded representation, which need
        // not be a complete encoded stream. Keep those bytes and headers intact.
        let has_body = !is_head && !matches!(parts.status.as_u16(), 204 | 205 | 206 | 304);
        let event_stream = events.filter(|_| has_body).and_then(|events| {
            Some((
                events,
                event_stream::decoder(&parts.headers, self.max_response_bytes)?,
            ))
        });

        let (body, encoded_response_body_bytes) = match event_stream {
            Some((events, decoder)) => {
                events
                    .read(&parts, stream, decoder, self.max_response_bytes)
                    .await?
            }
            None => {
                let mut body = Vec::new();

                match self.max_response_bytes {
                    Some(limit_bytes) => {
                        // Read at most one extra byte to distinguish an exact-limit
                        // response from an oversized stream, even without Content-Length.
                        stream
                            .take(limit_bytes + 1)
                            .read_to_end(&mut body)
                            .await
                            .map_err(ExecutionError::ReadBody)?;

                        if body.len() as u64 > limit_bytes {
                            return Err(ExecutionError::ResponseTooLarge { limit_bytes });
                        }
                    }
                    None => {
                        stream
                            .read_to_end(&mut body)
                            .await
                            .map_err(ExecutionError::ReadBody)?;
                    }
                }

                if has_body {
                    crate::response_encoding::decode_body(
                        &parts.headers,
                        body,
                        self.max_response_bytes,
                    )
                    .await?
                } else {
                    (body, None)
                }
            }
        };
        let download = received.elapsed();
        let response_header_bytes = parts
            .headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
            .sum();

        let response = HttpResponse {
            status: parts.status,
            version: parts.version,
            headers: parts.headers,
            body,
            metrics: HttpMetrics {
                prepare: prepared.duration_since(started),
                waiting: received.duration_since(prepared),
                download,
                request_header_bytes,
                request_body_bytes,
                response_header_bytes,
                encoded_response_body_bytes,
            },
        };

        Ok((response, url))
    }
}

impl Clients {
    /// The client for a connection to `url`. Plain HTTP checks no
    /// certificates, unless its proxy is reached over TLS.
    fn get(&self, url: &Url, verify: bool, version: HttpVersion) -> Result<Client, ExecutionError> {
        let preferences = &self.preferences;
        let secure = url.scheme() == "https";
        let tls_proxy = preferences.proxy.uses_tls(url);
        let client_certificate = secure
            .then(|| {
                crate::certificates::client_certificate(
                    &preferences.client_certificates,
                    url.host_str()?,
                    url.port_or_known_default()?,
                )
            })
            .flatten();

        if let Some(certificate) = client_certificate
            && tls_proxy
        {
            return Err(ExecutionError::Certificate(format!(
                "the client certificate for {} is not sent through a proxy reached over HTTPS, which would also be offered it. Use an HTTP proxy, or bypass the proxy for this host",
                certificate.host
            )));
        }

        let key = ClientKey {
            verify,
            version,
            ca_certificates: verify && (secure || tls_proxy),
            certificate: client_certificate.map(|certificate| certificate.id.clone()),
        };
        if let Some(client) = self.built.lock().unwrap().get(&key) {
            return Ok(client.clone());
        }

        let alpn: &[&[u8]] = match key.version {
            HttpVersion::Auto => &[b"h2", b"http/1.1"],
            HttpVersion::Http1_1 => &[b"http/1.1"],
            HttpVersion::Http2 => &[b"h2"],
        };
        let tls = Tls {
            verify: key.verify,
            server_name: None,
            ca_certificates: preferences
                .ca_certificates
                .as_deref()
                .filter(|_| key.ca_certificates),
            client_certificate,
        }
        .config(alpn)
        .map_err(ExecutionError::Certificate)?;

        let builder = client_builder(preferences, tls)?;
        let builder = match key.version {
            HttpVersion::Auto => builder,
            HttpVersion::Http1_1 => builder.http1_only(),
            HttpVersion::Http2 => builder.http2_prior_knowledge(),
        };
        let client: Client = Arc::new(builder.build().map_err(ExecutionError::Client)?.into());
        self.built.lock().unwrap().insert(key, client.clone());

        Ok(client)
    }
}

/// A client with the TLS configuration and proxy preferences, shared by HTTP
/// requests and WebSocket handshakes.
pub(crate) fn client_builder(
    preferences: &RequestPreferences,
    tls: rustls::ClientConfig,
) -> Result<reqwest::ClientBuilder, ExecutionError> {
    let builder = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        // Decode explicitly so received headers and encoded body sizes survive.
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();

    preferences.proxy.apply(builder)
}

fn validate_host(headers: &HeaderMap) -> Result<Option<Authority>, ExecutionError> {
    let mut hosts = headers.get_all(HOST).iter();
    let Some(value) = hosts.next() else {
        // The transport supplies the URL's host when there is no override.
        return Ok(None);
    };

    if hosts.next().is_some() {
        return Err(ExecutionError::MultipleHosts);
    }

    let value = value.to_str().map_err(|_| ExecutionError::InvalidHost)?;
    let authority = value
        .parse::<Authority>()
        .map_err(|_| ExecutionError::InvalidHost)?;

    // Generic header/authority syntax also accepts userinfo and nonnumeric ports.
    // Host permits only a hostname (or bracketed IPv6 address) and optional port.
    if value.contains('@') || url::Host::parse(authority.host()).is_err() {
        return Err(ExecutionError::InvalidHost);
    }

    let suffix = &value[authority.host().len()..];

    if !suffix.is_empty()
        && suffix != ":"
        && !(suffix.starts_with(':') && authority.port_u16().is_some())
    {
        return Err(ExecutionError::InvalidHost);
    }

    Ok(Some(authority))
}
