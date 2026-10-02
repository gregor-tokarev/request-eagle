use std::time::SystemTime;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use url::Url;

use super::{Auth, AuthLocation};
use crate::Field;

/// A header or query parameter that authorizes a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Credential {
    Header(String, String),
    Query(String, String),
}

/// The request a signature covers, as it is sent.
pub(crate) struct Outgoing<'a> {
    pub method: &'a str,
    /// Including the query that is sent.
    pub url: &'a Url,
    pub body: &'a [u8],
    /// The fields of a URL-encoded form body, which OAuth 1.0 signs.
    pub form: &'a [(String, String)],
}

impl Auth {
    /// The credentials a resolved authorization adds to a request. A gRPC
    /// call makes no HTTP request, so it passes none and cannot use the
    /// kinds that sign one or answer its challenge. Digest adds nothing
    /// until the server challenges the request.
    pub(crate) fn credentials(
        &self,
        request: Option<&Outgoing>,
        now: SystemTime,
    ) -> Result<Vec<Credential>, String> {
        let unsupported = || {
            format!(
                "{} cannot authorize gRPC calls. Choose another authorization in the Auth tab",
                self.kind().label()
            )
        };
        if request.is_none() && !self.kind().supports_grpc() {
            return Err(unsupported());
        }

        Ok(match self {
            Self::Inherit | Self::None | Self::Digest(_) => Vec::new(),
            Self::ApiKey(auth) if auth.key.is_empty() => Vec::new(),
            Self::ApiKey(auth) => vec![match auth.add_to {
                AuthLocation::Header => Credential::Header(auth.key.clone(), auth.value.clone()),
                AuthLocation::Query => Credential::Query(auth.key.clone(), auth.value.clone()),
            }],
            // Like Postman, an empty token or password sends nothing.
            Self::Bearer(auth) if auth.token.is_empty() => Vec::new(),
            Self::Bearer(auth) => vec![authorization(format!("Bearer {}", auth.token))],
            Self::Basic(auth) if auth.username.is_empty() && auth.password.is_empty() => Vec::new(),
            Self::Basic(auth) => vec![authorization(basic(&auth.username, &auth.password))],
            Self::OAuth2(auth) if auth.access_token.is_empty() => Vec::new(),
            Self::OAuth2(auth) => vec![match auth.add_to {
                AuthLocation::Header => {
                    authorization(prefixed(&auth.header_prefix, &auth.access_token))
                }
                AuthLocation::Query => {
                    Credential::Query("access_token".into(), auth.access_token.clone())
                }
            }],
            Self::Jwt(auth) => {
                let token = super::jwt::token(auth)?;

                vec![match auth.add_to {
                    AuthLocation::Header => authorization(prefixed(&auth.header_prefix, &token)),
                    AuthLocation::Query => Credential::Query(auth.query_param.clone(), token),
                }]
            }
            Self::OAuth1(auth) => super::oauth1::sign(auth, request.ok_or_else(unsupported)?, now)?,
            Self::AwsSignature(auth) => {
                super::aws::sign(auth, request.ok_or_else(unsupported)?, now)?
            }
        })
    }

    /// The header or query parameter the authorization sends. A request
    /// that sets it itself sends its own instead.
    pub(crate) fn credential_name(&self) -> Option<(AuthLocation, &str)> {
        let header = (AuthLocation::Header, "Authorization");

        match self {
            Self::Inherit | Self::None => None,
            Self::ApiKey(auth) => Some((auth.add_to, auth.key.as_str())),
            Self::Bearer(_) | Self::Basic(_) | Self::Digest(_) => Some(header),
            Self::OAuth1(auth) => Some(match auth.add_to {
                AuthLocation::Header => header,
                AuthLocation::Query => (AuthLocation::Query, "oauth_signature"),
            }),
            Self::OAuth2(auth) => Some(match auth.add_to {
                AuthLocation::Header => header,
                AuthLocation::Query => (AuthLocation::Query, "access_token"),
            }),
            Self::Jwt(auth) => Some(match auth.add_to {
                AuthLocation::Header => header,
                AuthLocation::Query => (AuthLocation::Query, auth.query_param.as_str()),
            }),
            Self::AwsSignature(auth) => Some(match auth.add_to {
                AuthLocation::Header => header,
                AuthLocation::Query => (AuthLocation::Query, "X-Amz-Signature"),
            }),
        }
    }

    /// The headers that sending adds for this authorization, for showing
    /// before it is sent. A value is `None` when it is only known then: it
    /// has `{{variables}}`, or it is signed or answers a challenge.
    pub fn preview_headers(&self) -> Vec<(String, Option<String>)> {
        let known = |value: String| (!value.contains("{{")).then_some(value);
        let calculated = || vec![("Authorization".to_owned(), None)];

        match self {
            Self::ApiKey(auth) if auth.add_to == AuthLocation::Header && !auth.key.is_empty() => {
                vec![(auth.key.clone(), known(auth.value.clone()))]
            }
            Self::Bearer(auth) if !auth.token.is_empty() => vec![(
                "Authorization".into(),
                known(format!("Bearer {}", auth.token)),
            )],
            Self::Basic(auth) if !auth.username.is_empty() || !auth.password.is_empty() => {
                let templated = auth.username.contains("{{") || auth.password.contains("{{");
                vec![(
                    "Authorization".into(),
                    (!templated).then(|| basic(&auth.username, &auth.password)),
                )]
            }
            Self::OAuth2(auth) if auth.add_to == AuthLocation::Header => {
                if auth.access_token.is_empty() {
                    Vec::new()
                } else {
                    vec![(
                        "Authorization".into(),
                        known(prefixed(&auth.header_prefix, &auth.access_token)),
                    )]
                }
            }
            Self::Digest(_) => calculated(),
            Self::OAuth1(auth) if auth.add_to == AuthLocation::Header => calculated(),
            Self::Jwt(auth) if auth.add_to == AuthLocation::Header => calculated(),
            Self::AwsSignature(auth) if auth.add_to == AuthLocation::Header => {
                let mut headers = calculated();
                headers.push(("X-Amz-Date".into(), None));
                if auth.service.trim() == "s3" {
                    headers.push(("X-Amz-Content-Sha256".into(), None));
                }
                if !auth.session_token.is_empty() {
                    headers.push((
                        "X-Amz-Security-Token".into(),
                        known(auth.session_token.clone()),
                    ));
                }
                headers
            }
            _ => Vec::new(),
        }
    }
}

/// Add a resolved authorization's credentials to a request's headers and
/// query, unless the request already sends the one it would add. A URL
/// that cannot be read is left for sending to report.
pub(crate) fn authorize(
    auth: &Auth,
    method: &str,
    url: &str,
    query: &mut Vec<Field>,
    headers: &mut Vec<Field>,
    body: &[u8],
    form: &[(String, String)],
) -> Result<(), String> {
    let Some((location, name)) = auth.credential_name() else {
        return Ok(());
    };
    let Ok(mut url) = Url::parse(url) else {
        return Ok(());
    };

    url.set_fragment(None);
    let sent = Field::pairs(query);
    if !sent.is_empty() {
        url.query_pairs_mut().extend_pairs(sent);
    }

    let overridden = match location {
        AuthLocation::Header => {
            Field::enabled(headers).any(|(header, _)| header.eq_ignore_ascii_case(name))
        }
        AuthLocation::Query => url.query_pairs().any(|(key, _)| key == name),
    };
    if overridden {
        return Ok(());
    }

    let request = Outgoing {
        method,
        url: &url,
        body,
        form,
    };
    for credential in auth.credentials(Some(&request), SystemTime::now())? {
        match credential {
            Credential::Header(name, value) => headers.push(Field::new(name, value)),
            Credential::Query(name, value) => query.push(Field::new(name, value)),
        }
    }

    Ok(())
}

fn authorization(value: String) -> Credential {
    Credential::Header("Authorization".into(), value)
}

pub(crate) fn basic(username: &str, password: &str) -> String {
    format!(
        "Basic {}",
        STANDARD.encode(format!("{username}:{password}"))
    )
}

/// A token after its scheme, such as `Bearer`. Without one, the token alone.
fn prefixed(prefix: &str, token: &str) -> String {
    match prefix.trim() {
        "" => token.to_owned(),
        prefix => format!("{prefix} {token}"),
    }
}
