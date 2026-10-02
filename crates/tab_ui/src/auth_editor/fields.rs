//! The text fields and choices of each kind of authorization, and where
//! their values live in `Auth`.

use request::{
    Auth, AuthKind, AuthLocation, JwtAlgorithm, OAuth1Signature, OAuth2ClientAuthentication,
    OAuth2Grant,
};

/// A text field of an authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Field {
    Key,
    Value,
    Token,
    Username,
    Password,
    ConsumerKey,
    ConsumerSecret,
    AccessToken,
    TokenSecret,
    PrivateKey,
    CallbackUrl,
    Verifier,
    Realm,
    HeaderPrefix,
    AuthUrl,
    TokenUrl,
    ClientId,
    ClientSecret,
    Scope,
    Secret,
    Payload,
    JwtHeaders,
    QueryParam,
    AccessKey,
    SecretKey,
    SessionToken,
    Region,
    Service,
}

/// A choice among a few options, shown as a select.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Choice {
    AddTo,
    SignatureMethod,
    GrantType,
    ClientAuthentication,
    Algorithm,
}

impl Field {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Key => "Key",
            Self::Value => "Value",
            Self::Token => "Token",
            Self::Username => "Username",
            Self::Password => "Password",
            Self::ConsumerKey => "Consumer Key",
            Self::ConsumerSecret => "Consumer Secret",
            Self::AccessToken => "Access Token",
            Self::TokenSecret => "Token Secret",
            Self::PrivateKey => "Private Key",
            Self::CallbackUrl => "Callback URL",
            Self::Verifier => "Verifier",
            Self::Realm => "Realm",
            Self::HeaderPrefix => "Header Prefix",
            Self::AuthUrl => "Auth URL",
            Self::TokenUrl => "Access Token URL",
            Self::ClientId => "Client ID",
            Self::ClientSecret => "Client Secret",
            Self::Scope => "Scope",
            Self::Secret => "Secret",
            Self::Payload => "Payload",
            Self::JwtHeaders => "JWT Headers",
            Self::QueryParam => "Query Param Name",
            Self::AccessKey => "Access Key",
            Self::SecretKey => "Secret Key",
            Self::SessionToken => "Session Token",
            Self::Region => "AWS Region",
            Self::Service => "Service Name",
        }
    }

    pub(super) fn placeholder(self, kind: AuthKind) -> &'static str {
        match self {
            // OAuth 1.0 sends a callback only to get a request token.
            Self::CallbackUrl if kind == AuthKind::OAuth1 => "Optional",
            Self::Key => "X-API-Key",
            Self::Token | Self::AccessToken => "Token",
            Self::PrivateKey => "-----BEGIN PRIVATE KEY-----",
            Self::CallbackUrl => "http://localhost:7777/callback",
            Self::HeaderPrefix => "Bearer",
            Self::AuthUrl => "https://example.com/oauth/authorize",
            Self::TokenUrl => "https://example.com/oauth/token",
            Self::Scope => "read write",
            Self::Payload => "{\"sub\": \"1234567890\"}",
            Self::QueryParam => "token",
            Self::Region => "us-east-1",
            Self::Service => "execute-api",
            Self::Realm | Self::Verifier | Self::SessionToken => "Optional",
            Self::JwtHeaders => "Optional, such as {\"kid\": \"key-1\"}",
            _ => self.label(),
        }
    }

    /// Whether it holds several lines, such as JSON or a PEM key.
    pub(super) fn multiline(self) -> bool {
        matches!(self, Self::PrivateKey | Self::Payload | Self::JwtHeaders)
    }

    /// Whether its value stays hidden until it is shown.
    pub(super) fn secret(self) -> bool {
        matches!(
            self,
            Self::Password
                | Self::ConsumerSecret
                | Self::TokenSecret
                | Self::ClientSecret
                | Self::Secret
                | Self::SecretKey
        )
    }

    /// The field's text in `auth`, when its kind has the field.
    pub(super) fn text_mut(self, auth: &mut Auth) -> Option<&mut String> {
        Some(match (auth, self) {
            (Auth::ApiKey(auth), Self::Key) => &mut auth.key,
            (Auth::ApiKey(auth), Self::Value) => &mut auth.value,
            (Auth::Bearer(auth), Self::Token) => &mut auth.token,
            (Auth::Basic(auth) | Auth::Digest(auth), Self::Username) => &mut auth.username,
            (Auth::Basic(auth) | Auth::Digest(auth), Self::Password) => &mut auth.password,
            (Auth::OAuth1(auth), Self::ConsumerKey) => &mut auth.consumer_key,
            (Auth::OAuth1(auth), Self::ConsumerSecret) => &mut auth.consumer_secret,
            (Auth::OAuth1(auth), Self::AccessToken) => &mut auth.access_token,
            (Auth::OAuth1(auth), Self::TokenSecret) => &mut auth.token_secret,
            (Auth::OAuth1(auth), Self::PrivateKey) => &mut auth.private_key,
            (Auth::OAuth1(auth), Self::CallbackUrl) => &mut auth.callback_url,
            (Auth::OAuth1(auth), Self::Verifier) => &mut auth.verifier,
            (Auth::OAuth1(auth), Self::Realm) => &mut auth.realm,
            (Auth::OAuth2(auth), Self::AccessToken) => &mut auth.access_token,
            (Auth::OAuth2(auth), Self::HeaderPrefix) => &mut auth.header_prefix,
            (Auth::OAuth2(auth), Self::AuthUrl) => &mut auth.auth_url,
            (Auth::OAuth2(auth), Self::TokenUrl) => &mut auth.token_url,
            (Auth::OAuth2(auth), Self::CallbackUrl) => &mut auth.callback_url,
            (Auth::OAuth2(auth), Self::ClientId) => &mut auth.client_id,
            (Auth::OAuth2(auth), Self::ClientSecret) => &mut auth.client_secret,
            (Auth::OAuth2(auth), Self::Scope) => &mut auth.scope,
            (Auth::OAuth2(auth), Self::Username) => &mut auth.username,
            (Auth::OAuth2(auth), Self::Password) => &mut auth.password,
            (Auth::Jwt(auth), Self::Secret) => &mut auth.secret,
            (Auth::Jwt(auth), Self::PrivateKey) => &mut auth.private_key,
            (Auth::Jwt(auth), Self::Payload) => &mut auth.payload,
            (Auth::Jwt(auth), Self::JwtHeaders) => &mut auth.headers,
            (Auth::Jwt(auth), Self::HeaderPrefix) => &mut auth.header_prefix,
            (Auth::Jwt(auth), Self::QueryParam) => &mut auth.query_param,
            (Auth::AwsSignature(auth), Self::AccessKey) => &mut auth.access_key,
            (Auth::AwsSignature(auth), Self::SecretKey) => &mut auth.secret_key,
            (Auth::AwsSignature(auth), Self::SessionToken) => &mut auth.session_token,
            (Auth::AwsSignature(auth), Self::Region) => &mut auth.region,
            (Auth::AwsSignature(auth), Self::Service) => &mut auth.service,
            _ => return None,
        })
    }

    pub(super) fn text(self, auth: &Auth) -> String {
        self.text_mut(&mut auth.clone())
            .map(|text| text.clone())
            .unwrap_or_default()
    }
}

const ADD_TO: [(AuthLocation, &str); 2] = [
    (AuthLocation::Header, "Header"),
    (AuthLocation::Query, "Query Params"),
];

const CLIENT_AUTHENTICATION: [(OAuth2ClientAuthentication, &str); 2] = [
    (
        OAuth2ClientAuthentication::Header,
        "Send as Basic Auth header",
    ),
    (OAuth2ClientAuthentication::Body, "Send credentials in body"),
];

impl Choice {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::AddTo => "Add To",
            Self::SignatureMethod => "Signature Method",
            Self::GrantType => "Grant Type",
            Self::ClientAuthentication => "Client Authentication",
            Self::Algorithm => "Algorithm",
        }
    }

    pub(super) fn options(self) -> Vec<&'static str> {
        match self {
            Self::AddTo => ADD_TO.iter().map(|(_, label)| *label).collect(),
            Self::SignatureMethod => OAuth1Signature::ALL
                .iter()
                .map(|method| method.label())
                .collect(),
            Self::GrantType => OAuth2Grant::ALL.iter().map(|grant| grant.label()).collect(),
            Self::ClientAuthentication => CLIENT_AUTHENTICATION
                .iter()
                .map(|(_, label)| *label)
                .collect(),
            Self::Algorithm => JwtAlgorithm::ALL
                .iter()
                .map(|algorithm| algorithm.label())
                .collect(),
        }
    }

    /// The option `auth` has chosen.
    pub(super) fn selected(self, auth: &Auth) -> Option<&'static str> {
        let location = |location: AuthLocation| {
            ADD_TO
                .iter()
                .find(|(option, _)| *option == location)
                .map(|(_, label)| *label)
        };

        match (auth, self) {
            (Auth::ApiKey(auth), Self::AddTo) => location(auth.add_to),
            (Auth::OAuth1(auth), Self::AddTo) => location(auth.add_to),
            (Auth::OAuth2(auth), Self::AddTo) => location(auth.add_to),
            (Auth::Jwt(auth), Self::AddTo) => location(auth.add_to),
            (Auth::AwsSignature(auth), Self::AddTo) => location(auth.add_to),
            (Auth::OAuth1(auth), Self::SignatureMethod) => Some(auth.signature_method.label()),
            (Auth::OAuth2(auth), Self::GrantType) => Some(auth.grant_type.label()),
            (Auth::OAuth2(auth), Self::ClientAuthentication) => CLIENT_AUTHENTICATION
                .iter()
                .find(|(option, _)| *option == auth.client_authentication)
                .map(|(_, label)| *label),
            (Auth::Jwt(auth), Self::Algorithm) => Some(auth.algorithm.label()),
            _ => None,
        }
    }

    /// Choose the option labelled `label` in `auth`.
    pub(super) fn select(self, auth: &mut Auth, label: &str) {
        let location = ADD_TO
            .iter()
            .find(|(_, option)| *option == label)
            .map(|(location, _)| *location);

        match (auth, self) {
            (Auth::ApiKey(auth), Self::AddTo) => auth.add_to = location.unwrap_or(auth.add_to),
            (Auth::OAuth1(auth), Self::AddTo) => auth.add_to = location.unwrap_or(auth.add_to),
            (Auth::OAuth2(auth), Self::AddTo) => auth.add_to = location.unwrap_or(auth.add_to),
            (Auth::Jwt(auth), Self::AddTo) => auth.add_to = location.unwrap_or(auth.add_to),
            (Auth::AwsSignature(auth), Self::AddTo) => {
                auth.add_to = location.unwrap_or(auth.add_to)
            }
            (Auth::OAuth1(auth), Self::SignatureMethod) => {
                if let Some(method) = OAuth1Signature::ALL
                    .into_iter()
                    .find(|method| method.label() == label)
                {
                    auth.signature_method = method;
                }
            }
            (Auth::OAuth2(auth), Self::GrantType) => {
                if let Some(grant) = OAuth2Grant::ALL
                    .into_iter()
                    .find(|grant| grant.label() == label)
                {
                    auth.grant_type = grant;
                }
            }
            (Auth::OAuth2(auth), Self::ClientAuthentication) => {
                if let Some((option, _)) = CLIENT_AUTHENTICATION
                    .iter()
                    .find(|(_, option)| *option == label)
                {
                    auth.client_authentication = *option;
                }
            }
            (Auth::Jwt(auth), Self::Algorithm) => {
                if let Some(algorithm) = JwtAlgorithm::ALL
                    .into_iter()
                    .find(|algorithm| algorithm.label() == label)
                {
                    auth.algorithm = algorithm;
                }
            }
            _ => {}
        }
    }
}

/// One line of an authorization's form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Text(Field),
    Choice(Choice),
    /// Proof Key for Code Exchange, of the OAuth 2.0 authorization code grant.
    Pkce,
    /// Whether the JWT secret is Base64.
    SecretBase64,
    /// A heading that starts a group of rows.
    Heading(&'static str),
    /// Getting a new OAuth 2.0 access token.
    GetToken,
}

/// The rows of an authorization's form, for requests of a protocol that
/// does (or does not) have a query.
pub(super) fn rows(auth: &Auth, query: bool) -> Vec<Row> {
    use Field::*;
    use Row::{Choice as Pick, Text};

    let add_to = query.then_some(Pick(Choice::AddTo));

    match auth {
        Auth::Inherit | Auth::None => Vec::new(),
        Auth::ApiKey(_) => [Some(Text(Key)), Some(Text(Value)), add_to]
            .into_iter()
            .flatten()
            .collect(),
        Auth::Bearer(_) => vec![Text(Token)],
        Auth::Basic(_) | Auth::Digest(_) => vec![Text(Username), Text(Password)],
        Auth::OAuth1(auth) => {
            let mut rows = vec![Pick(Choice::SignatureMethod), Text(ConsumerKey)];
            if auth.signature_method.uses_private_key() {
                rows.extend([Text(AccessToken), Text(PrivateKey)]);
            } else {
                rows.extend([Text(ConsumerSecret), Text(AccessToken), Text(TokenSecret)]);
            }
            rows.extend(add_to);
            rows.extend([
                Row::Heading("Advanced"),
                Text(CallbackUrl),
                Text(Verifier),
                Text(Realm),
            ]);
            rows
        }
        Auth::OAuth2(auth) => {
            let mut rows = vec![Row::Heading("Current Token"), Text(AccessToken)];
            if auth.add_to == AuthLocation::Header || !query {
                rows.push(Text(HeaderPrefix));
            }
            rows.extend(add_to);
            rows.extend([
                Row::Heading("Get New Access Token"),
                Pick(Choice::GrantType),
            ]);
            match auth.grant_type {
                OAuth2Grant::AuthorizationCode => {
                    rows.extend([Text(CallbackUrl), Text(AuthUrl), Text(TokenUrl)]);
                }
                OAuth2Grant::ClientCredentials => rows.push(Text(TokenUrl)),
                OAuth2Grant::Password => {
                    rows.extend([Text(TokenUrl), Text(Username), Text(Password)]);
                }
            }
            rows.extend([Text(ClientId), Text(ClientSecret), Text(Scope)]);
            if auth.grant_type == OAuth2Grant::AuthorizationCode {
                rows.push(Row::Pkce);
            }
            rows.extend([Pick(Choice::ClientAuthentication), Row::GetToken]);
            rows
        }
        Auth::Jwt(auth) => {
            let mut rows = vec![Pick(Choice::Algorithm)];
            if auth.algorithm.uses_secret() {
                rows.extend([Text(Secret), Row::SecretBase64]);
            } else {
                rows.push(Text(PrivateKey));
            }
            rows.extend([Text(Payload), Text(JwtHeaders)]);
            rows.extend(add_to);
            rows.push(if auth.add_to == AuthLocation::Query && query {
                Text(QueryParam)
            } else {
                Text(HeaderPrefix)
            });
            rows
        }
        Auth::AwsSignature(_) => [
            Some(Text(AccessKey)),
            Some(Text(SecretKey)),
            Some(Text(SessionToken)),
            Some(Text(Region)),
            Some(Text(Service)),
            add_to,
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

/// What each kind of authorization does, shown under its name.
pub(super) fn description(kind: AuthKind, grpc: bool) -> &'static str {
    match kind {
        AuthKind::Inherit => "Use the authorization of the collection this request is saved in.",
        AuthKind::None => "Send no authorization.",
        AuthKind::ApiKey if grpc => "Send a key and value as metadata.",
        AuthKind::ApiKey => "Send a key and value as a header or query parameter.",
        AuthKind::Bearer => "Send a token in the Authorization header.",
        AuthKind::Basic => "Send a username and password in the Authorization header.",
        AuthKind::Digest => {
            "Answer the server's Digest challenge with a username and password. The request is sent again with the answer."
        }
        AuthKind::OAuth1 => "Sign each request with OAuth 1.0 consumer and token credentials.",
        AuthKind::OAuth2 => {
            "Send an OAuth 2.0 access token. Get a new one from the authorization server below."
        }
        AuthKind::Jwt => "Sign a JSON Web Token when sending and send it as a bearer token.",
        AuthKind::AwsSignature => "Sign each request with AWS Signature Version 4.",
    }
}
