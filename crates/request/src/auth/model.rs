use serde::{Deserialize, Serialize};

/// How a request proves who sends it. A request inherits its collection's
/// authorization until it chooses its own.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    /// Use the collection's authorization. A collection itself has none.
    #[default]
    Inherit,
    None,
    ApiKey(ApiKeyAuth),
    Bearer(BearerAuth),
    Basic(PasswordAuth),
    /// Answers the server's Digest challenge, so the request is sent twice.
    Digest(PasswordAuth),
    #[serde(rename = "oauth1")]
    OAuth1(OAuth1Auth),
    #[serde(rename = "oauth2")]
    OAuth2(OAuth2Auth),
    Jwt(JwtAuth),
    #[serde(rename = "aws_signature")]
    AwsSignature(AwsSignatureAuth),
}

/// The kinds of authorization, in the order the Auth tab lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthKind {
    Inherit,
    None,
    ApiKey,
    Bearer,
    Basic,
    Digest,
    OAuth1,
    OAuth2,
    Jwt,
    AwsSignature,
}

/// Where a credential goes in the request.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthLocation {
    #[default]
    Header,
    Query,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
    /// gRPC calls send it as metadata either way.
    pub add_to: AuthLocation,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct BearerAuth {
    pub token: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PasswordAuth {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct OAuth1Auth {
    pub signature_method: OAuth1Signature,
    pub consumer_key: String,
    pub consumer_secret: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub access_token: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token_secret: String,
    /// A PEM key for the RSA signature methods.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub private_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub callback_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub verifier: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub realm: String,
    pub add_to: AuthLocation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum OAuth1Signature {
    #[default]
    #[serde(rename = "HMAC-SHA1")]
    HmacSha1,
    #[serde(rename = "HMAC-SHA256")]
    HmacSha256,
    #[serde(rename = "HMAC-SHA512")]
    HmacSha512,
    #[serde(rename = "RSA-SHA256")]
    RsaSha256,
    #[serde(rename = "RSA-SHA512")]
    RsaSha512,
    #[serde(rename = "PLAINTEXT")]
    Plaintext,
}

/// An access token, and how to get a new one from the authorization server.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct OAuth2Auth {
    /// The token sent with each request. Empty sends none.
    pub access_token: String,
    /// Written before the token in the Authorization header.
    pub header_prefix: String,
    /// In the query, the token is the `access_token` parameter.
    pub add_to: AuthLocation,
    pub grant_type: OAuth2Grant,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub auth_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token_url: String,
    /// Where the browser returns after signing in. Request Eagle listens on
    /// this loopback address while it waits.
    pub callback_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub client_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope: String,
    /// The resource owner's, for the password grant.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// Proof Key for Code Exchange, for the authorization code grant.
    pub pkce: bool,
    pub client_authentication: OAuth2ClientAuthentication,
}

impl Default for OAuth2Auth {
    fn default() -> Self {
        Self {
            access_token: String::new(),
            header_prefix: "Bearer".into(),
            add_to: AuthLocation::Header,
            grant_type: OAuth2Grant::AuthorizationCode,
            auth_url: String::new(),
            token_url: String::new(),
            callback_url: "http://localhost:7777/callback".into(),
            client_id: String::new(),
            client_secret: String::new(),
            scope: String::new(),
            username: String::new(),
            password: String::new(),
            pkce: true,
            client_authentication: OAuth2ClientAuthentication::Header,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2Grant {
    #[default]
    AuthorizationCode,
    ClientCredentials,
    Password,
}

/// How the client sends its ID and secret to the token endpoint.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2ClientAuthentication {
    /// As a Basic Authorization header.
    #[default]
    Header,
    /// As form fields of the token request.
    Body,
}

/// A JSON Web Token signed when the request is sent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct JwtAuth {
    pub algorithm: JwtAlgorithm,
    /// The HMAC secret of the HS algorithms.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub secret: String,
    pub secret_base64: bool,
    /// A PEM key for the RS, PS and ES algorithms.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub private_key: String,
    /// The claims, as JSON.
    pub payload: String,
    /// Header fields added to `alg` and `typ`, as a JSON object.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub headers: String,
    pub add_to: AuthLocation,
    pub header_prefix: String,
    pub query_param: String,
}

impl Default for JwtAuth {
    fn default() -> Self {
        Self {
            algorithm: JwtAlgorithm::Hs256,
            secret: String::new(),
            secret_base64: false,
            private_key: String::new(),
            payload: "{}".into(),
            headers: String::new(),
            add_to: AuthLocation::Header,
            header_prefix: "Bearer".into(),
            query_param: "token".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum JwtAlgorithm {
    #[default]
    Hs256,
    Hs384,
    Hs512,
    Rs256,
    Rs384,
    Rs512,
    Ps256,
    Ps384,
    Ps512,
    Es256,
    Es384,
}

/// AWS Signature Version 4.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct AwsSignatureAuth {
    pub access_key: String,
    pub secret_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session_token: String,
    pub region: String,
    pub service: String,
    /// In the query, the request is presigned.
    pub add_to: AuthLocation,
}

impl Auth {
    pub fn kind(&self) -> AuthKind {
        match self {
            Self::Inherit => AuthKind::Inherit,
            Self::None => AuthKind::None,
            Self::ApiKey(_) => AuthKind::ApiKey,
            Self::Bearer(_) => AuthKind::Bearer,
            Self::Basic(_) => AuthKind::Basic,
            Self::Digest(_) => AuthKind::Digest,
            Self::OAuth1(_) => AuthKind::OAuth1,
            Self::OAuth2(_) => AuthKind::OAuth2,
            Self::Jwt(_) => AuthKind::Jwt,
            Self::AwsSignature(_) => AuthKind::AwsSignature,
        }
    }

    pub fn is_inherit(&self) -> bool {
        matches!(self, Self::Inherit)
    }

    /// Whether a collection gives its requests no authorization.
    pub fn is_unset(&self) -> bool {
        matches!(self, Self::Inherit | Self::None)
    }

    /// Every text field, which may hold `{{variables}}`.
    pub(crate) fn texts_mut(&mut self) -> Vec<&mut String> {
        match self {
            Self::Inherit | Self::None => Vec::new(),
            Self::ApiKey(auth) => vec![&mut auth.key, &mut auth.value],
            Self::Bearer(auth) => vec![&mut auth.token],
            Self::Basic(auth) | Self::Digest(auth) => vec![&mut auth.username, &mut auth.password],
            Self::OAuth1(auth) => vec![
                &mut auth.consumer_key,
                &mut auth.consumer_secret,
                &mut auth.access_token,
                &mut auth.token_secret,
                &mut auth.private_key,
                &mut auth.callback_url,
                &mut auth.verifier,
                &mut auth.realm,
            ],
            Self::OAuth2(auth) => vec![
                &mut auth.access_token,
                &mut auth.header_prefix,
                &mut auth.auth_url,
                &mut auth.token_url,
                &mut auth.callback_url,
                &mut auth.client_id,
                &mut auth.client_secret,
                &mut auth.scope,
                &mut auth.username,
                &mut auth.password,
            ],
            Self::Jwt(auth) => vec![
                &mut auth.secret,
                &mut auth.private_key,
                &mut auth.payload,
                &mut auth.headers,
                &mut auth.header_prefix,
                &mut auth.query_param,
            ],
            Self::AwsSignature(auth) => vec![
                &mut auth.access_key,
                &mut auth.secret_key,
                &mut auth.session_token,
                &mut auth.region,
                &mut auth.service,
            ],
        }
    }

    /// The text of every field, for noticing when their variables change.
    pub(crate) fn texts(&self) -> Vec<String> {
        self.clone().texts_mut().into_iter().map(|text| text.clone()).collect()
    }

    /// Replace each `{{variable}}` in the fields.
    pub(crate) fn resolve_with<E>(
        &mut self,
        mut resolve: impl FnMut(&str) -> Result<String, E>,
    ) -> Result<(), E> {
        for text in self.texts_mut() {
            *text = resolve(text)?;
        }

        Ok(())
    }
}

impl AuthKind {
    pub const ALL: [Self; 10] = [
        Self::Inherit,
        Self::None,
        Self::ApiKey,
        Self::Bearer,
        Self::Basic,
        Self::Digest,
        Self::OAuth1,
        Self::OAuth2,
        Self::Jwt,
        Self::AwsSignature,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Inherit => "Inherit auth from collection",
            Self::None => "No Auth",
            Self::ApiKey => "API Key",
            Self::Bearer => "Bearer Token",
            Self::Basic => "Basic Auth",
            Self::Digest => "Digest Auth",
            Self::OAuth1 => "OAuth 1.0",
            Self::OAuth2 => "OAuth 2.0",
            Self::Jwt => "JWT Bearer",
            Self::AwsSignature => "AWS Signature",
        }
    }

    /// A new authorization of this kind, with its defaults.
    pub fn new_auth(self) -> Auth {
        match self {
            Self::Inherit => Auth::Inherit,
            Self::None => Auth::None,
            Self::ApiKey => Auth::ApiKey(ApiKeyAuth::default()),
            Self::Bearer => Auth::Bearer(BearerAuth::default()),
            Self::Basic => Auth::Basic(PasswordAuth::default()),
            Self::Digest => Auth::Digest(PasswordAuth::default()),
            Self::OAuth1 => Auth::OAuth1(OAuth1Auth::default()),
            Self::OAuth2 => Auth::OAuth2(OAuth2Auth::default()),
            Self::Jwt => Auth::Jwt(JwtAuth::default()),
            Self::AwsSignature => Auth::AwsSignature(AwsSignatureAuth::default()),
        }
    }

    /// Whether sending computes its credentials, such as a signature, rather
    /// than sending them as written.
    pub fn computes_credentials(self) -> bool {
        matches!(
            self,
            Self::Digest | Self::OAuth1 | Self::Jwt | Self::AwsSignature
        )
    }

    /// Whether it can authorize gRPC calls. The others sign or answer
    /// challenges for an HTTP request that a call does not make.
    pub fn supports_grpc(self) -> bool {
        !matches!(self, Self::Digest | Self::OAuth1 | Self::AwsSignature)
    }
}

impl OAuth1Signature {
    pub const ALL: [Self; 6] = [
        Self::HmacSha1,
        Self::HmacSha256,
        Self::HmacSha512,
        Self::RsaSha256,
        Self::RsaSha512,
        Self::Plaintext,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HmacSha1 => "HMAC-SHA1",
            Self::HmacSha256 => "HMAC-SHA256",
            Self::HmacSha512 => "HMAC-SHA512",
            Self::RsaSha256 => "RSA-SHA256",
            Self::RsaSha512 => "RSA-SHA512",
            Self::Plaintext => "PLAINTEXT",
        }
    }

    pub fn uses_private_key(self) -> bool {
        matches!(self, Self::RsaSha256 | Self::RsaSha512)
    }
}

impl OAuth2Grant {
    pub const ALL: [Self; 3] = [
        Self::AuthorizationCode,
        Self::ClientCredentials,
        Self::Password,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AuthorizationCode => "Authorization Code",
            Self::ClientCredentials => "Client Credentials",
            Self::Password => "Password Credentials",
        }
    }
}

impl JwtAlgorithm {
    pub const ALL: [Self; 11] = [
        Self::Hs256,
        Self::Hs384,
        Self::Hs512,
        Self::Rs256,
        Self::Rs384,
        Self::Rs512,
        Self::Ps256,
        Self::Ps384,
        Self::Ps512,
        Self::Es256,
        Self::Es384,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hs256 => "HS256",
            Self::Hs384 => "HS384",
            Self::Hs512 => "HS512",
            Self::Rs256 => "RS256",
            Self::Rs384 => "RS384",
            Self::Rs512 => "RS512",
            Self::Ps256 => "PS256",
            Self::Ps384 => "PS384",
            Self::Ps512 => "PS512",
            Self::Es256 => "ES256",
            Self::Es384 => "ES384",
        }
    }

    /// Whether it signs with an HMAC secret rather than a private key.
    pub fn uses_secret(self) -> bool {
        matches!(self, Self::Hs256 | Self::Hs384 | Self::Hs512)
    }
}
