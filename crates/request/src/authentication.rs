use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::HttpError;

/// Credentials saved with a request. Use environment variables for shared files.
#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Authentication {
    #[default]
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        name: String,
        value: String,
        #[serde(default)]
        location: ApiKeyLocation,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

impl std::fmt::Debug for Authentication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Basic { .. } => f.debug_struct("Basic").finish_non_exhaustive(),
            Self::Bearer { .. } => f.debug_struct("Bearer").finish_non_exhaustive(),
            Self::ApiKey { location, .. } => f
                .debug_struct("ApiKey")
                .field("location", location)
                .finish_non_exhaustive(),
        }
    }
}

impl Authentication {
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub(crate) fn header_name(&self) -> Option<&str> {
        match self {
            Self::None
            | Self::ApiKey {
                location: ApiKeyLocation::Query,
                ..
            } => None,
            Self::Basic { .. } | Self::Bearer { .. } => Some("Authorization"),
            Self::ApiKey { name, .. } => Some(name),
        }
    }

    /// Explicit headers and query parameters take precedence over this editor.
    pub(crate) fn apply(
        &self,
        headers: &mut Vec<(String, String)>,
        url: &mut Url,
    ) -> Result<(), HttpError> {
        if let Some(name) = self.header_name()
            && headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            return Ok(());
        }

        match self {
            Self::None => {}
            Self::Basic { username, password } => {
                if username.contains(':') {
                    return Err(HttpError::InvalidAuthentication(
                        "Basic usernames cannot contain a colon",
                    ));
                }

                headers.push((
                    "Authorization".into(),
                    format!(
                        "Basic {}",
                        STANDARD.encode(format!("{username}:{password}"))
                    ),
                ));
            }
            Self::Bearer { token } => {
                if token.trim().is_empty() {
                    return Err(HttpError::InvalidAuthentication("enter a Bearer token"));
                }

                headers.push(("Authorization".into(), format!("Bearer {token}")));
            }
            Self::ApiKey {
                name,
                value,
                location,
            } => {
                if name.trim().is_empty() {
                    return Err(HttpError::InvalidAuthentication("enter an API key name"));
                }

                match location {
                    ApiKeyLocation::Header => headers.push((name.clone(), value.clone())),
                    ApiKeyLocation::Query => {
                        if !url.query_pairs().any(|(key, _)| key == name.as_str()) {
                            url.query_pairs_mut().append_pair(name, value);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn remove_query_credentials(&self, url: &mut Url) {
        if let Self::ApiKey {
            name,
            location: ApiKeyLocation::Query,
            ..
        } = self
        {
            let query = url
                .query_pairs()
                .filter(|(key, _)| key != name.as_str())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect::<Vec<_>>();
            url.set_query(None);

            if !query.is_empty() {
                url.query_pairs_mut().extend_pairs(query);
            }
        }
    }
}
