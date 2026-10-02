use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ring::signature::{self, KeyPair as _, RsaKeyPair, UnparsedPublicKey};
use rustls::pki_types::{PrivateKeyDer, pem::PemObject};
use url::Url;

use super::credentials::{Credential, Outgoing};
use super::*;

/// A 2048-bit RSA key made for these tests.
const RSA_KEY: &str = include_str!("test_rsa_key.pem");

fn outgoing<'a>(method: &'a str, url: &'a Url) -> Outgoing<'a> {
    Outgoing {
        method,
        url,
        body: &[],
        form: &[],
    }
}

fn header(credentials: &[Credential], name: &str) -> String {
    credentials
        .iter()
        .find_map(|credential| match credential {
            Credential::Header(header, value) if header == name => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {name} header in {credentials:?}"))
}

fn jwt_parts(token: &str) -> (serde_json::Value, serde_json::Value, Vec<u8>, String) {
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "{token}");
    let decode = |part: &str| URL_SAFE_NO_PAD.decode(part).unwrap();

    (
        serde_json::from_slice(&decode(parts[0])).unwrap(),
        serde_json::from_slice(&decode(parts[1])).unwrap(),
        decode(parts[2]),
        format!("{}.{}", parts[0], parts[1]),
    )
}

#[test]
fn simple_credentials_go_where_they_are_configured() {
    let url = Url::parse("https://example.com/").unwrap();
    let request = outgoing("GET", &url);
    let credentials = |auth: Auth| auth.credentials(Some(&request), SystemTime::now()).unwrap();

    assert_eq!(
        credentials(Auth::ApiKey(ApiKeyAuth {
            key: "X-Api-Key".into(),
            value: "secret".into(),
            add_to: AuthLocation::Header,
        })),
        [Credential::Header("X-Api-Key".into(), "secret".into())]
    );
    assert_eq!(
        credentials(Auth::ApiKey(ApiKeyAuth {
            key: "api_key".into(),
            value: "secret".into(),
            add_to: AuthLocation::Query,
        })),
        [Credential::Query("api_key".into(), "secret".into())]
    );
    assert_eq!(
        credentials(Auth::Bearer(BearerAuth {
            token: "abc".into()
        })),
        [Credential::Header(
            "Authorization".into(),
            "Bearer abc".into()
        )]
    );
    assert_eq!(
        credentials(Auth::Basic(PasswordAuth {
            username: "Aladdin".into(),
            password: "open sesame".into(),
        })),
        [Credential::Header(
            "Authorization".into(),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==".into()
        )]
    );
    assert_eq!(
        credentials(Auth::OAuth2(OAuth2Auth {
            access_token: "token".into(),
            header_prefix: "Token".into(),
            ..OAuth2Auth::default()
        })),
        [Credential::Header(
            "Authorization".into(),
            "Token token".into()
        )]
    );
    assert_eq!(
        credentials(Auth::OAuth2(OAuth2Auth {
            access_token: "token".into(),
            add_to: AuthLocation::Query,
            ..OAuth2Auth::default()
        })),
        [Credential::Query("access_token".into(), "token".into())]
    );

    // Empty credentials send nothing, as in Postman.
    for auth in [
        Auth::Inherit,
        Auth::None,
        AuthKind::ApiKey.new_auth(),
        AuthKind::Bearer.new_auth(),
        AuthKind::Basic.new_auth(),
        AuthKind::OAuth2.new_auth(),
        // Digest waits for the server's challenge.
        Auth::Digest(PasswordAuth {
            username: "user".into(),
            password: "pass".into(),
        }),
    ] {
        assert_eq!(credentials(auth), []);
    }
}

#[test]
fn a_header_or_parameter_the_request_sets_itself_takes_precedence() {
    let bearer = Auth::Bearer(BearerAuth {
        token: "from-auth".into(),
    });
    let mut query = Vec::new();
    let mut headers = vec![("authorization".to_owned(), "Bearer own".to_owned())];
    authorize(
        &bearer,
        "GET",
        "https://example.com/",
        &mut query,
        &mut headers,
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(
        headers,
        [("authorization".to_owned(), "Bearer own".to_owned())]
    );

    let api_key = Auth::ApiKey(ApiKeyAuth {
        key: "key".into(),
        value: "from-auth".into(),
        add_to: AuthLocation::Query,
    });
    let mut headers = Vec::new();
    authorize(
        &api_key,
        "GET",
        "https://example.com/?key=own",
        &mut query,
        &mut headers,
        &[],
        &[],
    )
    .unwrap();
    assert!(query.is_empty());

    authorize(
        &api_key,
        "GET",
        "https://example.com/?other=1",
        &mut query,
        &mut headers,
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(query, [("key".to_owned(), "from-auth".to_owned())]);
}

#[test]
fn signing_kinds_cannot_authorize_grpc_calls() {
    for kind in [AuthKind::Digest, AuthKind::OAuth1, AuthKind::AwsSignature] {
        assert!(!kind.supports_grpc());
    }

    let error = AuthKind::OAuth1
        .new_auth()
        .credentials(None, SystemTime::now())
        .unwrap_err();
    assert!(
        error.contains("OAuth 1.0 cannot authorize gRPC calls"),
        "{error}"
    );

    // Digest sends nothing before a challenge, which a call never gets.
    assert!(AuthKind::Bearer.supports_grpc() && AuthKind::Jwt.supports_grpc());
}

#[test]
fn oauth1_signs_the_query_form_and_protocol_parameters() {
    // Twitter's documented example of a signed request.
    let url = Url::parse("https://api.twitter.com/1.1/statuses/update.json?include_entities=true")
        .unwrap();
    let form = [(
        "status".to_owned(),
        "Hello Ladies + Gentlemen, a signed OAuth request!".to_owned(),
    )];
    let request = Outgoing {
        method: "post",
        url: &url,
        body: b"",
        form: &form,
    };
    let auth = OAuth1Auth {
        consumer_key: "xvz1evFS4wEEPTGEFPHBog".into(),
        consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw".into(),
        access_token: "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb".into(),
        token_secret: "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE".into(),
        ..OAuth1Auth::default()
    };

    let credentials = oauth1::sign_with(
        &auth,
        &request,
        "1318622958",
        "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
    )
    .unwrap();
    let authorization = header(&credentials, "Authorization");
    assert!(authorization.starts_with("OAuth oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\", "));
    assert!(
        authorization.contains("oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\""),
        "{authorization}"
    );

    // In the query, the same parameters are sent unencoded, since the URL
    // encodes them.
    let credentials = oauth1::sign_with(
        &OAuth1Auth {
            add_to: AuthLocation::Query,
            realm: "Example".into(),
            ..auth.clone()
        },
        &request,
        "1318622958",
        "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
    )
    .unwrap();
    assert!(credentials.contains(&Credential::Query(
        "oauth_signature".into(),
        "hCtSmYh+iHYCEqBWrE7C7hYmtUk=".into()
    )));
    assert!(credentials.iter().all(
        |credential| matches!(credential, Credential::Query(name, _) if name.starts_with("oauth_"))
    ));

    // PLAINTEXT sends the key itself, and a realm comes first.
    let credentials = oauth1::sign_with(
        &OAuth1Auth {
            signature_method: OAuth1Signature::Plaintext,
            realm: "Photos".into(),
            ..auth
        },
        &request,
        "1",
        "n",
    )
    .unwrap();
    let authorization = header(&credentials, "Authorization");
    assert!(authorization.starts_with("OAuth realm=\"Photos\", "));
    assert!(authorization.contains(
        "oauth_signature=\"kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw%26LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE\""
    ));
}

#[test]
fn oauth1_rsa_signatures_verify_with_the_public_key() {
    let url = Url::parse("https://example.com/photos?size=original").unwrap();
    let auth = OAuth1Auth {
        signature_method: OAuth1Signature::RsaSha256,
        consumer_key: "key".into(),
        private_key: RSA_KEY.into(),
        ..OAuth1Auth::default()
    };

    let credentials =
        oauth1::sign_with(&auth, &outgoing("GET", &url), "1318622958", "nonce").unwrap();
    let authorization = header(&credentials, "Authorization");
    let signature = authorization
        .split("oauth_signature=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    let signature = STANDARD
        .decode(
            percent_encoding::percent_decode_str(signature)
                .decode_utf8()
                .unwrap()
                .as_ref(),
        )
        .unwrap();

    let base = "GET&https%3A%2F%2Fexample.com%2Fphotos&oauth_consumer_key%3Dkey%26oauth_nonce%3Dnonce%26oauth_signature_method%3DRSA-SHA256%26oauth_timestamp%3D1318622958%26oauth_version%3D1.0%26size%3Doriginal";
    rsa_public_key()
        .verify(base.as_bytes(), &signature)
        .expect("a valid signature of the base string");

    let error = oauth1::sign_with(
        &OAuth1Auth {
            private_key: String::new(),
            ..auth
        },
        &outgoing("GET", &url),
        "1",
        "n",
    )
    .unwrap_err();
    assert!(error.contains("enter a private key"), "{error}");
}

fn rsa_public_key() -> UnparsedPublicKey<Vec<u8>> {
    let PrivateKeyDer::Pkcs8(key) = PrivateKeyDer::from_pem_slice(RSA_KEY.as_bytes()).unwrap()
    else {
        panic!("the test key is PKCS #8");
    };
    let key = RsaKeyPair::from_pkcs8(key.secret_pkcs8_der()).unwrap();

    UnparsedPublicKey::new(
        &signature::RSA_PKCS1_2048_8192_SHA256,
        key.public_key().as_ref().to_vec(),
    )
}

#[test]
fn jwt_tokens_match_the_reference_and_keep_the_written_claim_order() {
    let auth = JwtAuth {
        secret: "your-256-bit-secret".into(),
        payload:
            "{\n  \"sub\": \"1234567890\",\n  \"name\": \"John Doe\",\n  \"iat\": 1516239022\n}"
                .into(),
        ..JwtAuth::default()
    };
    let credentials = Auth::Jwt(auth.clone())
        .credentials(None, SystemTime::now())
        .unwrap();

    // The example on jwt.io.
    assert_eq!(
        credentials,
        [Credential::Header(
            "Authorization".into(),
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c".into()
        )]
    );

    // Headers join alg and typ; the secret may be Base64; the token may go
    // in the query.
    let credentials = Auth::Jwt(JwtAuth {
        algorithm: JwtAlgorithm::Hs512,
        secret: STANDARD.encode("secret"),
        secret_base64: true,
        headers: r#"{"kid": "key-1", "typ": "at+jwt", "alg": "none"}"#.into(),
        add_to: AuthLocation::Query,
        query_param: "jwt".into(),
        ..auth.clone()
    })
    .credentials(None, SystemTime::now())
    .unwrap();
    let [Credential::Query(name, token)] = credentials.as_slice() else {
        panic!("{credentials:?}");
    };
    assert_eq!(name, "jwt");
    let (header, _, signature, input) = jwt_parts(token);
    assert_eq!(
        header,
        serde_json::json!({"alg": "HS512", "typ": "at+jwt", "kid": "key-1"})
    );
    assert_eq!(
        signature,
        crypto::hmac(ring::hmac::HMAC_SHA512, b"secret", input.as_bytes())
    );

    let error = Auth::Jwt(JwtAuth {
        payload: "{".into(),
        ..auth
    })
    .credentials(None, SystemTime::now())
    .unwrap_err();
    assert!(
        error.starts_with("The JWT payload is not valid JSON"),
        "{error}"
    );
}

#[test]
fn jwt_signs_with_rsa_and_elliptic_curve_keys() {
    let auth = JwtAuth {
        algorithm: JwtAlgorithm::Rs256,
        private_key: RSA_KEY.into(),
        payload: r#"{"sub":"me"}"#.into(),
        header_prefix: String::new(),
        ..JwtAuth::default()
    };
    let token = header(
        &Auth::Jwt(auth.clone())
            .credentials(None, SystemTime::now())
            .unwrap(),
        "Authorization",
    );
    let (header, payload, signature, input) = jwt_parts(&token);
    assert_eq!(header["alg"], "RS256");
    assert_eq!(payload, serde_json::json!({"sub": "me"}));
    rsa_public_key()
        .verify(input.as_bytes(), &signature)
        .unwrap();

    let key = rcgen::KeyPair::generate().unwrap();
    let token = header_value(
        Auth::Jwt(JwtAuth {
            algorithm: JwtAlgorithm::Es256,
            private_key: key.serialize_pem(),
            ..auth.clone()
        }),
        "Authorization",
    );
    let (_, _, signature, input) = jwt_parts(&token);
    UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, key.public_key_raw())
        .verify(input.as_bytes(), &signature)
        .unwrap();

    // An RSA key cannot sign ES256.
    let error = Auth::Jwt(JwtAuth {
        algorithm: JwtAlgorithm::Es256,
        ..auth
    })
    .credentials(None, SystemTime::now())
    .unwrap_err();
    assert!(error.starts_with("Could not sign the JWT"), "{error}");
}

fn header_value(auth: Auth, name: &str) -> String {
    header(&auth.credentials(None, SystemTime::now()).unwrap(), name)
}

#[test]
fn digest_answers_match_the_rfc_examples() {
    let url = Url::parse("http://www.example.com/dir/index.html").unwrap();

    // RFC 2617, section 3.5.
    let auth = PasswordAuth {
        username: "Mufasa".into(),
        password: "Circle Of Life".into(),
    };
    let challenge = r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
    let answer = auth
        .answer_challenge(challenge, "GET", &url, &[], "0a4f113b")
        .unwrap();
    assert_eq!(
        answer,
        r#"Digest username="Mufasa", realm="testrealm@host.com", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", uri="/dir/index.html", qop=auth, nc=00000001, cnonce="0a4f113b", response="6629fae49393a05397450978507c4ef1", opaque="5ccc069c403ebaf9f0171e9517f40e41""#
    );

    // RFC 7616, section 3.9.1, with SHA-256 offered after another scheme.
    let auth = PasswordAuth {
        username: "Mufasa".into(),
        password: "Circle of Life".into(),
    };
    let challenge = r#"Basic realm="other", Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=SHA-256, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#;
    let answer = auth
        .answer_challenge(
            challenge,
            "GET",
            &url,
            &[],
            "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ",
        )
        .unwrap();
    assert!(answer.contains("algorithm=SHA-256"), "{answer}");
    assert!(
        answer.contains(
            r#"response="753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1""#
        ),
        "{answer}"
    );

    // Without qop, the response leaves out the client nonce.
    let answer = auth
        .answer_challenge(r#"Digest realm="r", nonce="n""#, "GET", &url, &[], "c")
        .unwrap();
    assert!(!answer.contains("cnonce"), "{answer}");

    // Other schemes and unknown algorithms are not answered.
    assert_eq!(
        auth.answer_challenge(r#"Basic realm="r""#, "GET", &url, &[], "c"),
        None
    );
    assert_eq!(
        auth.answer_challenge(
            r#"Digest realm="r", nonce="n", algorithm=SHA-1"#,
            "GET",
            &url,
            &[],
            "c"
        ),
        None
    );
}

#[test]
fn aws_signatures_match_the_signature_v4_test_suite() {
    // The get-vanilla request of AWS's Signature Version 4 test suite.
    let url = Url::parse("https://example.amazonaws.com/").unwrap();
    let time = UNIX_EPOCH + Duration::from_secs(1_440_938_160);
    let auth = AwsSignatureAuth {
        access_key: "AKIDEXAMPLE".into(),
        secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
        region: "us-east-1".into(),
        service: "service".into(),
        ..AwsSignatureAuth::default()
    };

    let credentials = Auth::AwsSignature(auth.clone())
        .credentials(Some(&outgoing("GET", &url)), time)
        .unwrap();
    assert_eq!(header(&credentials, "X-Amz-Date"), "20150830T123600Z");
    assert_eq!(
        header(&credentials, "Authorization"),
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );

    // A presigned URL carries the same in its query, and S3 signs the
    // payload's hash header.
    let credentials = Auth::AwsSignature(AwsSignatureAuth {
        add_to: AuthLocation::Query,
        session_token: "session".into(),
        ..auth.clone()
    })
    .credentials(Some(&outgoing("GET", &url)), time)
    .unwrap();
    let names: Vec<&str> = credentials
        .iter()
        .map(|credential| match credential {
            Credential::Query(name, _) => name.as_str(),
            Credential::Header(name, _) => panic!("header {name} in a presigned URL"),
        })
        .collect();
    assert_eq!(
        names,
        [
            "X-Amz-Algorithm",
            "X-Amz-Credential",
            "X-Amz-Date",
            "X-Amz-Security-Token",
            "X-Amz-SignedHeaders",
            "X-Amz-Signature"
        ]
    );

    let credentials = Auth::AwsSignature(AwsSignatureAuth {
        service: "s3".into(),
        ..auth.clone()
    })
    .credentials(Some(&outgoing("GET", &url)), time)
    .unwrap();
    assert_eq!(
        header(&credentials, "X-Amz-Content-Sha256"),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert!(
        header(&credentials, "Authorization")
            .contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date,")
    );

    let error = Auth::AwsSignature(AwsSignatureAuth {
        region: " ".into(),
        ..auth
    })
    .credentials(Some(&outgoing("GET", &url)), time)
    .unwrap_err();
    assert_eq!(error, "Enter the AWS region in the Auth tab");
}

#[test]
fn previews_show_known_values_and_leave_the_rest_for_sending() {
    assert_eq!(
        Auth::Basic(PasswordAuth {
            username: "user".into(),
            password: "pass".into(),
        })
        .preview_headers(),
        [(
            "Authorization".to_owned(),
            Some("Basic dXNlcjpwYXNz".to_owned())
        )]
    );
    assert_eq!(
        Auth::Bearer(BearerAuth {
            token: "{{token}}".into()
        })
        .preview_headers(),
        [("Authorization".to_owned(), None)]
    );
    assert_eq!(
        AuthKind::Jwt.new_auth().preview_headers(),
        [("Authorization".to_owned(), None)]
    );
    assert_eq!(
        Auth::ApiKey(ApiKeyAuth {
            key: "key".into(),
            value: "value".into(),
            add_to: AuthLocation::Query,
        })
        .preview_headers(),
        []
    );
}

#[test]
fn saved_requests_keep_only_an_authorization_they_choose() {
    let request = crate::HttpRequest {
        path: "https://example.com".into(),
        ..Default::default()
    };
    assert!(!toml::to_string(&request).unwrap().contains("auth"));

    let request = crate::HttpRequest {
        auth: Auth::Bearer(BearerAuth {
            token: "{{token}}".into(),
        }),
        ..request
    };
    let saved = toml::to_string(&request).unwrap();
    assert!(
        saved.contains("[auth]\ntype = \"bearer\"\ntoken = \"{{token}}\""),
        "{saved}"
    );
    assert_eq!(
        toml::from_str::<crate::HttpRequest>(&saved).unwrap(),
        request
    );

    let none: crate::WebSocketRequest =
        toml::from_str("url = \"wss://example.com\"\n[auth]\ntype = \"none\"").unwrap();
    assert_eq!(none.auth, Auth::None);

    // Fields left out take their defaults.
    let oauth: crate::GrpcRequest = toml::from_str(
        "url = \"localhost:50051\"\n[auth]\ntype = \"oauth2\"\naccess_token = \"t\"",
    )
    .unwrap();
    assert_eq!(
        oauth.auth,
        Auth::OAuth2(OAuth2Auth {
            access_token: "t".into(),
            ..OAuth2Auth::default()
        })
    );
}
