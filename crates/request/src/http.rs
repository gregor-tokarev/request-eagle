use std::sync::Arc;

use http_client::{AsyncBody, HttpClient, HttpRequestExt, RedirectPolicy, Request, Url};
use smol::io::AsyncReadExt;

use crate::{
    ExecutionError, HttpError, HttpRequest, HttpResponse, HttpVersion, RequestPreferences,
};

#[derive(Clone)]
pub(crate) struct HttpExecutor {
    client: Arc<reqwest_client::ReqwestClient>,
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

        let mut builder = reqwest::Client::builder()
            .use_rustls_tls()
            .danger_accept_invalid_certs(!preferences.ssl_certificate_verification)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();

        builder = match preferences.http_version {
            HttpVersion::Auto => builder,
            HttpVersion::Http1_1 => builder.http1_only(),
            HttpVersion::Http2 => builder.http2_prior_knowledge(),
        };

        let client = builder.build().map_err(HttpError::Client)?;

        Ok(Self {
            client: Arc::new(client.into()),
            max_response_bytes,
        })
    }

    pub(crate) async fn execute(
        &self,
        request: HttpRequest,
    ) -> Result<HttpResponse, ExecutionError> {
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
            .uri(url.as_str())
            // Expose redirect responses just like other HTTP statuses.
            .follow_redirects(RedirectPolicy::NoFollow);

        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }

        let request = builder
            .body(request.body.map(AsyncBody::from).unwrap_or_default())
            .map_err(HttpError::InvalidRequest)?;
        let response = self
            .client
            .send(request)
            .await
            .map_err(HttpError::Transport)?;
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

        Ok(HttpResponse {
            status: parts.status,
            version: parts.version,
            headers: parts.headers,
            body,
        })
    }
}
