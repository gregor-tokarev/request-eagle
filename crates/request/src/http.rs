use std::{sync::Arc, time::Instant};

use http_client::http::{HeaderMap, header::HOST, uri::Authority};
use http_client::{Request, Url};
use smol::io::AsyncReadExt;

use crate::{
    ExecutionError, HttpError, HttpMetrics, HttpRequest, HttpResponse, HttpVersion,
    RequestPreferences,
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
        request: HttpRequest,
    ) -> Result<HttpResponse, ExecutionError> {
        let started = Instant::now();
        let request_body_bytes = request.body.as_ref().map_or(0, Vec::len);
        let mut url = Url::parse(&request.path).map_err(HttpError::InvalidUrl)?;

        if !matches!(url.scheme(), "http" | "https") {
            return Err(HttpError::UnsupportedScheme(url.scheme().to_owned()).into());
        }

        // Keep query pairs from the URL, including repeated keys, then append
        // the editor's pairs with URL encoding. Fragments are never sent.
        url.set_fragment(None);

        if let Some(query) = &request.query
            && !query.is_empty()
        {
            url.query_pairs_mut().extend_pairs(query);
        }

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

        for (name, value) in request.headers.into_iter().chain(generated) {
            builder = builder.header(name, value);
        }

        let mut request = builder
            .body(request.body)
            .map_err(HttpError::InvalidRequest)?;

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
                return Err(HttpError::Http2HostOverride.into());
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
                    .map_err(HttpError::ReadBody)?;

                if body.len() as u64 > limit_bytes {
                    return Err(ExecutionError::ResponseTooLarge { limit_bytes });
                }
            }
            None => {
                stream
                    .read_to_end(&mut body)
                    .await
                    .map_err(HttpError::ReadBody)?;
            }
        }

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
            },
        })
    }
}

fn build_client(
    preferences: &RequestPreferences,
    version: HttpVersion,
) -> Result<Arc<reqwest_client::ReqwestClient>, HttpError> {
    let builder = reqwest::Client::builder()
        .use_rustls_tls()
        .danger_accept_invalid_certs(!preferences.ssl_certificate_verification)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();
    let builder = match version {
        HttpVersion::Auto => builder,
        HttpVersion::Http1_1 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
    };
    let client = preferences
        .proxy
        .apply(builder)?
        .build()
        .map_err(HttpError::Client)?;

    Ok(Arc::new(client.into()))
}

fn validate_host(headers: &HeaderMap) -> Result<Option<Authority>, HttpError> {
    let mut hosts = headers.get_all(HOST).iter();
    let Some(value) = hosts.next() else {
        // The transport supplies the URL's host when there is no override.
        return Ok(None);
    };

    if hosts.next().is_some() {
        return Err(HttpError::MultipleHosts);
    }

    let value = value.to_str().map_err(|_| HttpError::InvalidHost)?;
    let authority = value
        .parse::<Authority>()
        .map_err(|_| HttpError::InvalidHost)?;

    // Generic header/authority syntax also accepts userinfo and nonnumeric ports.
    // Host permits only a hostname (or bracketed IPv6 address) and optional port.
    if value.contains('@') || url::Host::parse(authority.host()).is_err() {
        return Err(HttpError::InvalidHost);
    }

    let suffix = &value[authority.host().len()..];

    if !suffix.is_empty()
        && suffix != ":"
        && !(suffix.starts_with(':') && authority.port_u16().is_some())
    {
        return Err(HttpError::InvalidHost);
    }

    Ok(Some(authority))
}
