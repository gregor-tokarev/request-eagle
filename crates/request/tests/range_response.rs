use std::io::Write;

use flate2::{Compression, write::GzEncoder};
use request::{Execution, HttpRequest, HttpVersion, RequestExecutor, RequestPreferences, Response};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn execute_range(
    headers: Vec<(String, String)>,
    representation: &[u8],
    encoding: Option<&str>,
) -> (Execution, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut response = format!(
        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-14/{}\r\nContent-Length: 12\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n",
        representation.len()
    );

    if let Some(encoding) = encoding {
        response.push_str(&format!("Content-Encoding: {encoding}\r\n"));
    }

    response.push_str("\r\n");
    let mut response = response.into_bytes();
    response.extend_from_slice(&representation[3..15]);

    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();

        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
            assert!(head.len() < 16 * 1024);
        }

        stream.write_all(&response).await.unwrap();

        String::from_utf8(head).unwrap()
    });
    let execution = RequestExecutor::new(&RequestPreferences {
        http_version: HttpVersion::Http1_1,
        timeout_ms: 2_000,
        ..RequestPreferences::default()
    })
    .unwrap()
    .execute(HttpRequest {
        path: url,
        headers,
        ..HttpRequest::default()
    })
    .await
    .unwrap();

    (execution, server.await)
}

#[test]
fn range_requests_do_not_advertise_gzip_and_receive_selected_plain_bytes() {
    smol::block_on(async {
        let representation = b"abcdefghijklmnopqrstuvwxyz";
        let (execution, head) = execute_range(
            vec![("rAnGe".into(), "bytes=3-14".into())],
            representation,
            None,
        )
        .await;
        let head = head.to_ascii_lowercase();
        let Response::Http(response) = execution.response;

        assert!(head.contains("\r\nrange: bytes=3-14\r\n"));
        assert!(!head.contains("\r\naccept-encoding:"));
        assert_eq!(response.status.as_u16(), 206);
        assert_eq!(response.body, &representation[3..15]);
        assert_eq!(response.headers["content-range"], "bytes 3-14/26");
        assert_eq!(response.headers["content-length"], "12");
        assert_eq!(response.metrics.encoded_response_body_bytes, None);
    });
}

#[test]
fn explicit_gzip_range_preserves_partial_encoded_bytes_and_received_headers() {
    smol::block_on(async {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"abcdefghijklmnopqrstuvwxyz").unwrap();
        let representation = encoder.finish().unwrap();
        let (execution, head) = execute_range(
            vec![
                ("Range".into(), "bytes=3-14".into()),
                ("Accept-Encoding".into(), "gzip".into()),
            ],
            &representation,
            Some("gzip"),
        )
        .await;
        let head = head.to_ascii_lowercase();
        let Response::Http(response) = execution.response;

        assert!(head.contains("\r\nrange: bytes=3-14\r\n"));
        assert!(head.contains("\r\naccept-encoding: gzip\r\n"));
        assert_eq!(response.status.as_u16(), 206);
        assert_eq!(response.body, representation[3..15]);
        assert_eq!(response.headers.len(), 5);
        assert_eq!(response.headers["content-encoding"], "gzip");
        assert_eq!(
            response.headers["content-range"],
            format!("bytes 3-14/{}", representation.len())
        );
        assert_eq!(response.headers["content-length"], "12");
        assert_eq!(response.headers["content-type"], "application/octet-stream");
        assert_eq!(response.headers["connection"], "close");
        assert_eq!(response.metrics.encoded_response_body_bytes, None);
    });
}
