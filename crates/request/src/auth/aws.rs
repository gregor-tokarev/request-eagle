//! AWS Signature Version 4.

use std::time::SystemTime;

use chrono::{DateTime, Utc};
use percent_encoding::percent_decode_str;
use ring::hmac::HMAC_SHA256;

use super::credentials::{Credential, Outgoing};
use super::crypto::{encode, hex, hmac, sha256};
use super::{AuthLocation, AwsSignatureAuth};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// How long a presigned S3 URL stays valid, in seconds.
const PRESIGNED_EXPIRY: &str = "86400";

pub(super) fn sign(
    auth: &AwsSignatureAuth,
    request: &Outgoing,
    now: SystemTime,
) -> Result<Vec<Credential>, String> {
    let region = auth.region.trim();
    let service = auth.service.trim();
    for (value, name) in [
        (auth.access_key.trim(), "access key"),
        (auth.secret_key.trim(), "secret key"),
        (region, "region"),
        (service, "service name"),
    ] {
        if value.is_empty() {
            return Err(format!("Enter the AWS {name} in the Auth tab"));
        }
    }

    // The request's own x-amz-* headers are signed too, and one it sets
    // itself replaces the one signing would add.
    let own: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .filter(|(name, _)| name.starts_with("x-amz-"))
        .collect();
    let own_value = |name: &str| {
        own.iter()
            .find(|(own, _)| own == name)
            .map(|(_, value)| value.clone())
    };

    let amz_date = own_value("x-amz-date").unwrap_or_else(|| {
        DateTime::<Utc>::from(now)
            .format("%Y%m%dT%H%M%SZ")
            .to_string()
    });
    let date: String = amz_date.chars().take(8).collect();
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let credential = format!("{}/{scope}", auth.access_key.trim());
    let s3 = service == "s3";

    let url = request.url;
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_owned(),
    };
    let mut query: Vec<(String, String)> = url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    let mut headers = own.clone();
    headers.push(("host".into(), host));
    let mut added = Vec::new();

    let payload = match auth.add_to {
        AuthLocation::Header => {
            let payload =
                own_value("x-amz-content-sha256").unwrap_or_else(|| hex(&sha256(request.body)));
            let mut generated = vec![("X-Amz-Date", amz_date.clone())];
            if s3 {
                generated.push(("X-Amz-Content-Sha256", payload.clone()));
            }
            if !auth.session_token.is_empty() {
                generated.push(("X-Amz-Security-Token", auth.session_token.clone()));
            }

            for (name, value) in generated {
                let lowercase = name.to_ascii_lowercase();
                if own_value(&lowercase).is_none() {
                    headers.push((lowercase, value.clone()));
                    added.push(Credential::Header(name.to_owned(), value));
                }
            }

            payload
        }
        AuthLocation::Query => {
            let mut signed: Vec<&str> = headers.iter().map(|(name, _)| name.as_str()).collect();
            signed.sort();
            let mut parameters = vec![
                ("X-Amz-Algorithm", ALGORITHM.to_owned()),
                ("X-Amz-Credential", credential.clone()),
                ("X-Amz-Date", amz_date.clone()),
            ];
            if s3 {
                parameters.push(("X-Amz-Expires", PRESIGNED_EXPIRY.to_owned()));
            }
            if !auth.session_token.is_empty() {
                parameters.push(("X-Amz-Security-Token", auth.session_token.clone()));
            }
            parameters.push(("X-Amz-SignedHeaders", signed.join(";")));
            query.extend(
                parameters
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), value.clone())),
            );
            added.extend(
                parameters
                    .into_iter()
                    .map(|(name, value)| Credential::Query(name.to_owned(), value)),
            );

            // S3 cannot know the body of a presigned URL in advance.
            if s3 {
                "UNSIGNED-PAYLOAD".to_owned()
            } else {
                hex(&sha256(request.body))
            }
        }
    };

    headers.sort();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| {
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("{name}:{value}\n")
        })
        .collect();

    let mut query: Vec<(String, String)> = query
        .iter()
        .map(|(name, value)| (encode(name), encode(value)))
        .collect();
    query.sort();
    let canonical_query = query
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_request = format!(
        "{}\n{}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload}",
        request.method.to_ascii_uppercase(),
        canonical_uri(url.path(), s3),
    );
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex(&sha256(canonical_request.as_bytes()))
    );

    let key = [date.as_str(), region, service, "aws4_request"]
        .into_iter()
        .fold(
            format!("AWS4{}", auth.secret_key.trim()).into_bytes(),
            |key, part| hmac(HMAC_SHA256, &key, part.as_bytes()),
        );
    let signature = hex(&hmac(HMAC_SHA256, &key, string_to_sign.as_bytes()));

    added.push(match auth.add_to {
        AuthLocation::Header => Credential::Header(
            "Authorization".into(),
            format!(
                "{ALGORITHM} Credential={credential}, SignedHeaders={signed_headers}, Signature={signature}"
            ),
        ),
        AuthLocation::Query => Credential::Query("X-Amz-Signature".into(), signature),
    });

    Ok(added)
}

/// The path as AWS signs it. S3 encodes each segment of the decoded path.
/// Other services encode the path as it is sent, so its own escapes are
/// encoded again, and leave out empty segments.
pub(super) fn canonical_uri(path: &str, s3: bool) -> String {
    if s3 {
        let path = if path.is_empty() { "/" } else { path };

        return path
            .split('/')
            .map(|segment| encode(&percent_decode_str(segment).decode_utf8_lossy()))
            .collect::<Vec<_>>()
            .join("/");
    }

    let segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(encode)
        .collect();
    let mut uri = format!("/{}", segments.join("/"));
    if path.ends_with('/') && !segments.is_empty() {
        uri.push('/');
    }

    uri
}
