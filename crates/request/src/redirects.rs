use bytes::Bytes;
use http_client::{
    AsyncBody, HttpClient, Method, RedirectPolicy, Request, Response, Url,
    http::header::{
        AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, LOCATION,
        PROXY_AUTHORIZATION, REFERER, TRANSFER_ENCODING, WWW_AUTHENTICATE,
    },
};

use crate::HttpError;

const REDIRECT_LIMIT: u32 = 100;

pub(crate) async fn send(
    client: &reqwest_client::ReqwestClient,
    request: Request<Option<Vec<u8>>>,
    mut url: Url,
    follow: bool,
) -> Result<Response<AsyncBody>, HttpError> {
    let (mut parts, body) = request.into_parts();
    let mut body = body.map(Bytes::from);
    let mut redirects = 0;

    loop {
        // The transport cannot change an explicit Host during a redirect.
        // Handle those hops here, then delegate once the override is gone.
        let follow_in_transport = follow && !parts.headers.contains_key(HOST);
        parts.extensions.insert(if follow_in_transport {
            RedirectPolicy::FollowLimit(REDIRECT_LIMIT - redirects)
        } else {
            RedirectPolicy::NoFollow
        });

        let response = client
            .send(Request::from_parts(parts.clone(), body.clone().into()))
            .await
            .map_err(HttpError::Transport)?;

        if !follow || follow_in_transport {
            return Ok(response);
        }

        let status = response.status().as_u16();

        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }

        let next = response.headers().get(LOCATION).and_then(|location| {
            url.join(std::str::from_utf8(location.as_bytes()).ok()?)
                .ok()
        });
        let Some(mut next) = next else {
            return Ok(response);
        };
        next.set_fragment(None);

        let Ok(uri) = next.as_str().parse() else {
            return Ok(response);
        };

        redirects += 1;

        if redirects >= REDIRECT_LIMIT {
            return Err(HttpError::Transport(anyhow::anyhow!("too many redirects")));
        }

        if !matches!(next.scheme(), "http" | "https") {
            return Err(HttpError::UnsupportedScheme(next.scheme().to_owned()));
        }

        if matches!(status, 301..=303) {
            body = None;

            for header in [
                TRANSFER_ENCODING,
                CONTENT_ENCODING,
                CONTENT_TYPE,
                CONTENT_LENGTH,
            ] {
                parts.headers.remove(header);
            }

            if !matches!(parts.method, Method::GET | Method::HEAD) {
                parts.method = Method::GET;
            }
        }

        if next.host_str() != url.host_str()
            || next.port_or_known_default() != url.port_or_known_default()
        {
            for header in [
                HOST,
                AUTHORIZATION,
                COOKIE,
                PROXY_AUTHORIZATION,
                WWW_AUTHENTICATE,
            ] {
                parts.headers.remove(header);
            }

            parts.headers.remove("cookie2");
        }

        // Match the transport's Referer behavior without including URL credentials.
        if !(url.scheme() == "https" && next.scheme() == "http") {
            let _ = url.set_username("");
            let _ = url.set_password(None);

            if let Ok(referer) = url.as_str().parse() {
                parts.headers.insert(REFERER, referer);
            }
        }

        parts.uri = uri;
        url = next;
    }
}
