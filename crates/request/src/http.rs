use std::{sync::Arc, time::Instant};

use bytes::Bytes;
use http_client::http::{HeaderMap, header::HOST, uri::Authority};
use http_client::{Request, Url};
use smol::io::AsyncReadExt;

use crate::{
    ExecutionError, HttpMetrics, HttpRequest, HttpResponse, HttpVersion, RequestPreferences,
};

#[derive(Clone)]
pub(crate) struct HttpExecutor {
    client: Arc<reqwest_client::ReqwestClient>,
    host_override_client: Option<Arc<reqwest_client::ReqwestClient>>,
    http_version: HttpVersion,
    follow_all_redirects: bool,
    max_response_bytes: Option<u64>,
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

    pub(crate) fn new(preferences: &RequestPreferences) -> Result<Self, ExecutionError> {
        let max_response_bytes = match preferences.max_response_size_mb {
            0 => None,
            megabytes => Some(
                megabytes
                    .checked_mul(1024 * 1024)
                    .ok_or(ExecutionError::InvalidResponseLimit)?,
            ),
        };

        let client = build_client(preferences, preferences.http_version)?;
        let host_override_client = if preferences.http_version == HttpVersion::Auto {
            Some(build_client(preferences, HttpVersion::Http1_1)?)
        } else {
            None
        };

        Ok(Self {
            client,
            host_override_client,
            http_version: preferences.http_version,
            follow_all_redirects: preferences.follow_all_redirects,
            max_response_bytes,
        })
    }

    pub(crate) async fn execute(
        &self,
        request: &HttpRequest,
        body: Option<Bytes>,
    ) -> Result<HttpResponse, ExecutionError> {
        let started = Instant::now();
        let is_head = request.method.as_str() == "HEAD";
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
        let mut client = &self.client;

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
            } else if let Some(override_client) = &self.host_override_client {
                // This transport derives :authority from the connection URL.
                // Choose HTTP/1.1 before sending so custom Host routing works
                // without changing the destination/TLS name or replaying a request.
                client = override_client;
            } else {
                return Err(ExecutionError::Http2HostOverride);
            }
        }

        let request_header_bytes = request
            .headers()
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
            .sum();

        if generated_host {
            // Let the transport regenerate Host when a redirect changes the URL.
            request.headers_mut().remove(HOST);
        }

        let prepared = Instant::now();
        let response =
            crate::redirects::send(client.as_ref(), request, url, self.follow_all_redirects)
                .await?;
        let received = Instant::now();
        let (parts, mut stream) = response.into_parts();
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

        // HEAD and statuses without a body may describe an encoded representation
        // in their headers, but there are no bytes to pass to a gzip decoder.
        // A 206 body contains a range of the encoded representation, which need
        // not be a complete gzip stream. Keep those bytes and headers intact.
        let (body, encoded_response_body_bytes) =
            if is_head || matches!(parts.status.as_u16(), 204 | 205 | 206 | 304) {
                (body, None)
            } else {
                crate::response_encoding::decode_body(&parts.headers, body, self.max_response_bytes)
                    .await?
            };
        let download = received.elapsed();
        let response_header_bytes = parts
            .headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
            .sum();

        Ok(HttpResponse {
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
        })
    }
}

fn build_client(
    preferences: &RequestPreferences,
    version: HttpVersion,
) -> Result<Arc<reqwest_client::ReqwestClient>, ExecutionError> {
    let builder = client_builder(preferences)?;
    let builder = match version {
        HttpVersion::Auto => builder,
        HttpVersion::Http1_1 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
    };
    let client = builder.build().map_err(ExecutionError::Client)?;

    Ok(Arc::new(client.into()))
}

/// A client with the certificate and proxy preferences, shared by HTTP
/// requests and WebSocket handshakes.
pub(crate) fn client_builder(
    preferences: &RequestPreferences,
) -> Result<reqwest::ClientBuilder, ExecutionError> {
    let builder = reqwest::Client::builder()
        .use_rustls_tls()
        .danger_accept_invalid_certs(!preferences.ssl_certificate_verification)
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
