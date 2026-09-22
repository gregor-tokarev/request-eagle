use std::io::Write;

use flate2::{Compression, write::GzEncoder};
use request::{
    Execution, ExecutionError, HttpError, HttpRequest, HttpVersion, Method, RequestExecutor,
    RequestPreferences, Response,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn gzip(body: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    encoder.finish().unwrap()
}

async fn execute_response(
    status: u16,
    encoding: &str,
    payload: &[u8],
    method: Method,
    limit_mb: u64,
) -> (Result<Execution, ExecutionError>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut response = format!(
        "HTTP/1.1 {status} Response\r\nContent-Encoding: {encoding}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )
    .into_bytes();
    response.extend_from_slice(payload);
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();

        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
            assert!(head.len() < 16 * 1024);
        }

        let _ = stream.write_all(&response).await;

        String::from_utf8(head).unwrap()
    });
    let result = RequestExecutor::new(&RequestPreferences {
        http_version: HttpVersion::Http1_1,
        timeout_ms: 2_000,
        max_response_size_mb: limit_mb,
        ..RequestPreferences::default()
    })
    .unwrap()
    .execute(HttpRequest {
        method,
        path: url,
        ..HttpRequest::default()
    })
    .await;

    (result, server.await)
}

#[test]
fn decodes_gzip_json_and_preserves_received_headers_and_download_size() {
    smol::block_on(async {
        let json = br#"{"message":"readable JSON"}"#;
        let compressed = gzip(json);
        let (result, head) = execute_response(200, "GZip", &compressed, Method::Get, 1).await;
        let Response::Http(response) = result.unwrap().response;

        assert_eq!(response.body, json);
        assert_eq!(response.headers["content-encoding"], "GZip");
        assert_eq!(
            response.headers["content-length"],
            compressed.len().to_string()
        );
        assert_eq!(
            response.metrics.encoded_response_body_bytes,
            Some(compressed.len())
        );
        assert!(
            head.to_ascii_lowercase()
                .contains("accept-encoding: gzip\r\n")
        );
    });
}

#[test]
fn rejects_malformed_truncated_and_corrupt_gzip_bodies() {
    smol::block_on(async {
        let mut truncated = gzip(b"readable JSON");
        truncated.truncate(truncated.len() - 4);
        let mut corrupt = gzip(b"readable JSON");
        let checksum = corrupt.len() - 8;
        corrupt[checksum] ^= 1;

        for payload in [b"not gzip".to_vec(), Vec::new(), truncated, corrupt] {
            let (result, _) = execute_response(200, "gzip", &payload, Method::Get, 1).await;
            assert!(matches!(
                result,
                Err(ExecutionError::Http(HttpError::DecodeBody(_)))
            ));
        }
    });
}

#[test]
fn limits_uncompressed_size_and_accepts_an_exact_limit() {
    smol::block_on(async {
        let limit = 1_048_576;

        for size in [limit, limit + 1] {
            let compressed = gzip(&vec![b'x'; size]);
            assert!(compressed.len() < limit);
            let (result, _) = execute_response(200, "gzip", &compressed, Method::Get, 1).await;

            if size == limit {
                let Response::Http(response) = result.unwrap().response;
                assert_eq!(response.body, vec![b'x'; limit]);
            } else {
                assert!(matches!(
                    result,
                    Err(ExecutionError::ResponseTooLarge {
                        limit_bytes: 1_048_576
                    })
                ));
            }
        }
    });
}

#[test]
fn zero_response_limit_allows_larger_decoded_bodies() {
    smol::block_on(async {
        let json = vec![b'x'; 1_048_577];
        let (result, _) = execute_response(200, "gzip", &gzip(&json), Method::Get, 0).await;
        let Response::Http(response) = result.unwrap().response;

        assert_eq!(response.body, json);
    });
}

#[test]
fn decodes_concatenated_members_and_stacked_gzip_encodings() {
    smol::block_on(async {
        let mut members = gzip(b"{\"ok\":");
        members.extend(gzip(b"true}"));
        let json = b"{\"ok\":true}";

        for (encoding, payload) in [
            ("gzip", members),
            ("gzip, identity, GZIP", gzip(&gzip(json))),
            ("gzip\r\nContent-Encoding: gzip", gzip(&gzip(json))),
        ] {
            let (result, _) = execute_response(200, encoding, &payload, Method::Get, 1).await;
            let Response::Http(response) = result.unwrap().response;
            assert_eq!(response.body, json);
            assert_eq!(
                response.metrics.encoded_response_body_bytes,
                Some(payload.len())
            );
        }
    });
}

#[test]
fn leaves_unsupported_encodings_and_unencoded_bytes_unchanged() {
    smol::block_on(async {
        for encoding in ["identity", "br", "gzip, br"] {
            let payload = b"opaque body bytes";
            let (result, _) = execute_response(200, encoding, payload, Method::Get, 1).await;
            let Response::Http(response) = result.unwrap().response;

            assert_eq!(response.body, payload);
            assert_eq!(response.metrics.encoded_response_body_bytes, None);
        }
    });
}

#[test]
fn bodyless_responses_do_not_attempt_to_decode_gzip_metadata() {
    smol::block_on(async {
        for (status, method) in [
            (200, Method::Head),
            (204, Method::Get),
            (205, Method::Get),
            (304, Method::Get),
        ] {
            let (result, _) = execute_response(status, "gzip", b"", method, 1).await;
            let Response::Http(response) = result.unwrap().response;

            assert!(response.body.is_empty());
            assert_eq!(response.headers["content-encoding"], "gzip");
        }
    });
}
