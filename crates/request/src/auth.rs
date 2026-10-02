//! How requests prove who sends them. `Auth` is saved with each request and
//! collection; a request inherits its collection's until it chooses its own.
//! Sending adds the credentials after variables resolve and pre-request
//! scripts run, unless the request sets the same header or parameter itself.
//! Digest answers the server's challenge by sending the request again. gRPC
//! calls send credentials as metadata, so they cannot use the signing kinds.
//! `RequestExecutor::oauth2_token` gets new OAuth 2.0 access tokens.

mod aws;
mod credentials;
mod crypto;
mod digest;
mod jwt;
mod model;
mod oauth1;
mod oauth2;

#[cfg(test)]
mod tests;

pub(crate) use credentials::{Credential, authorize, sends_own_credential};
pub use model::{
    ApiKeyAuth, Auth, AuthKind, AuthLocation, AwsSignatureAuth, BearerAuth, JwtAlgorithm, JwtAuth,
    OAuth1Auth, OAuth1Signature, OAuth2Auth, OAuth2ClientAuthentication, OAuth2Grant, PasswordAuth,
};
pub use oauth2::{OAuth2Token, OAuth2TokenRequest};
