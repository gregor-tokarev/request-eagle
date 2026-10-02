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

    let time = DateTime::<Utc>::from(now);
    let amz_date = time.format("%Y%m%dT%H%M%SZ").to_string();
    let date = time.format("%Y%m%d").to_string();
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

    let (payload, mut headers, mut added) = match auth.add_to {
        AuthLocation::Header => {
            let payload = hex(&sha256(request.body));
            let mut headers = vec![("x-amz-date", amz_date.clone())];
            let mut added = vec![Credential::Header("X-Amz-Date".into(), amz_date.clone())];
            if s3 {
                headers.push(("x-amz-content-sha256", payload.clone()));
                added.push(Credential::Header(
                    "X-Amz-Content-Sha256".into(),
                    payload.clone(),
                ));
            }
            if !auth.session_token.is_empty() {
                headers.push(("x-amz-security-token", auth.session_token.clone()));
                added.push(Credential::Header(
                    "X-Amz-Security-Token".into(),
                    auth.session_token.clone(),
                ));
            }

            (payload, headers, added)
        }
        AuthLocation::Query => {
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
            parameters.push(("X-Amz-SignedHeaders", "host".to_owned()));
            query.extend(
                parameters
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), value.clone())),
            );

            // S3 cannot know the body of a presigned URL in advance.
            let payload = if s3 {
                "UNSIGNED-PAYLOAD".to_owned()
            } else {
                hex(&sha256(request.body))
            };
            let added = parameters
                .into_iter()
                .map(|(name, value)| Credential::Query(name.to_owned(), value))
                .collect();

            (payload, Vec::new(), added)
        }
    };

    headers.push(("host", host));
    headers.sort();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
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

/// Each segment of the path encoded as AWS expects: twice, except for S3.
fn canonical_uri(path: &str, s3: bool) -> String {
    if path.is_empty() {
        return "/".into();
    }

    path.split('/')
        .map(|segment| {
            let encoded = encode(&percent_decode_str(segment).decode_utf8_lossy());
            if s3 { encoded } else { encode(&encoded) }
        })
        .collect::<Vec<_>>()
        .join("/")
}
