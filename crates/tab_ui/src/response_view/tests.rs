use std::time::Duration;

use request::{Execution, HeaderMap, HttpResponse, Response, StatusCode, Version};

use super::{ResponseContent, body::BodyMode};

fn response(body: &[u8], content_type: &str) -> ResponseContent {
    with_headers(body, &[("content-type", content_type)])
}

fn with_headers(body: &[u8], pairs: &[(&str, &str)]) -> ResponseContent {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.append(
            name.parse::<request::HeaderName>().unwrap(),
            value.parse().unwrap(),
        );
    }

    ResponseContent::new(Execution {
        scripts: Vec::new(),
        elapsed: Duration::from_millis(239),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers,
            body: body.to_vec(),
            metrics: request::HttpMetrics::default(),
        }),
    })
}

#[test]
fn response_formatting_preserves_the_entire_body() {
    let json = response(b"{\"a\":1}", "application/json");
    assert_eq!(json.raw, "{\"a\":1}");
    assert_eq!(json.pretty.as_deref(), Some("{\n  \"a\": 1\n}"));
    assert_eq!(json.language, "json");
    assert_eq!(json.http().body, b"{\"a\":1}");
    assert_eq!(response(b"<html></html>", "text/html").language, "html");
    assert_eq!(response(b"1.2.3.4", "text/plain").language, "text");
    assert!(response(b"{broken", "application/json").pretty.is_none());

    let oversized = "a".repeat(1_048_575) + "é";
    let large = response(oversized.as_bytes(), "text/plain");
    assert_eq!(large.raw.as_ref(), oversized);
    assert_eq!(large.raw.len(), 1_048_577);
    assert_eq!(large.http().body.len(), 1_048_577);
}

#[test]
fn response_size_limits_lock_raw_for_long_lines_and_pretty_expansion() {
    let cases = [
        format!("\"{}\"", "a".repeat(32 * 1024)),
        format!("[\n{}0]", "0,\n".repeat(70_000)),
        format!("[\n{}0]", "0,\n".repeat(90_000)),
    ];
    for raw in cases {
        let content = response(raw.as_bytes(), "application/json");
        assert!(content.raw_only);
        assert!(content.pretty.is_none());
        assert_eq!(content.raw.as_ref(), raw);
    }
    assert!(!response(&vec![b'a'; 32 * 1024], "text/plain").raw_only);
    let at_limit = "1234567\n".repeat(32 * 1024);
    assert!(!response(at_limit.as_bytes(), "text/plain").raw_only);
}

/// A 24-bit bitmap with every pixel grey.
fn bmp(width: u32, height: u32) -> Vec<u8> {
    let row = (width * 3).div_ceil(4) * 4;
    let size = 54 + row * height;
    let mut bmp = b"BM".to_vec();
    bmp.extend(size.to_le_bytes());
    bmp.extend([0; 4]);
    bmp.extend(54u32.to_le_bytes());
    bmp.extend(40u32.to_le_bytes());
    bmp.extend(width.to_le_bytes());
    bmp.extend(height.to_le_bytes());
    bmp.extend(1u16.to_le_bytes());
    bmp.extend(24u16.to_le_bytes());
    bmp.extend([0; 4]);
    bmp.extend((row * height).to_le_bytes());
    bmp.extend([0; 16]);
    bmp.resize(size as usize, 0x80);
    bmp
}

/// A document of blank pages, each 200 by 100 points.
fn pdf(pages: usize) -> Vec<u8> {
    let kids: Vec<_> = (0..pages).map(|page| format!("{} 0 R", page + 3)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        ),
    ];
    objects.extend(
        (0..pages).map(|_| "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".to_owned()),
    );

    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    pdf
}

#[test]
fn bodies_are_recognized_by_media_type_and_signature() {
    use BodyMode::*;

    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    let zip = b"PK\x03\x04\x14\0\0\0\x08\0\xff\xfe";
    let cases: [(ResponseContent, &str, &[BodyMode], BodyMode); 13] = [
        (
            response(b"{\"a\":1}", "application/json"),
            "JSON",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"<!doctype html><p>Hi</p>", "text/html"),
            "HTML",
            &[Preview, Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"<!DOCTYPE html><p>Hi</p>", "text/plain"),
            "HTML",
            &[Preview, Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"<a><b>1</b></a>", "application/soap+xml"),
            "XML",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"let a = 1;", "text/javascript"),
            "JavaScript",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"a { color: red }", "text/css"),
            "CSS",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"a: 1", "application/yaml"),
            "YAML",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            response(b"caf\xe9", "text/plain; charset=iso-8859-1"),
            "Text",
            &[Raw, Hex],
            Raw,
        ),
        (
            response(png, "application/octet-stream"),
            "PNG",
            &[Preview, Hex],
            Preview,
        ),
        (
            response(
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
                "image/svg+xml",
            ),
            "SVG",
            &[Preview, Pretty, Raw, Hex],
            Preview,
        ),
        (
            response(&pdf(1), "application/pdf"),
            "PDF",
            &[Preview, Hex],
            Preview,
        ),
        (
            response(
                zip,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            "Binary",
            &[Hex],
            Hex,
        ),
        (with_headers(&[0, 1, 2, 255], &[]), "Binary", &[Hex], Hex),
    ];

    for (content, label, modes, default) in cases {
        assert_eq!(content.label(), label);
        assert_eq!(content.modes(), modes, "{label}");
        assert_eq!(content.default_mode(), default, "{label}");
        assert_eq!(content.binary, content.raw.is_empty(), "{label}");
    }

    // Undeclared text stays text, even when it is cut inside a character.
    let text = "é".repeat(5_000);
    assert!(!with_headers(text.as_bytes(), &[]).binary);
    assert_eq!(response(b"caf\xe9", "text/plain").raw, "caf\u{fffd}");
    // A named charset decodes the text.
    assert_eq!(
        response(b"caf\xe9", "text/plain; charset=\"ISO-8859-1\"").raw,
        "café"
    );
    assert_eq!(
        response(
            b"\xcf\xf0\xe8\xe2\xe5\xf2",
            "text/html;charset=windows-1251"
        )
        .raw,
        "Привет"
    );
    assert_eq!(
        response(b"<a><b x=\"1\">text</b><c/></a>", "text/xml")
            .pretty
            .as_deref(),
        Some("<a>\n  <b x=\"1\">text</b>\n  <c/>\n</a>")
    );
    // Malformed XML is still highlighted as it was received.
    assert_eq!(
        response(b"<a><b></a>", "application/xml").pretty.as_deref(),
        Some("<a><b></a>")
    );
}

#[test]
fn saved_bodies_are_named_by_the_server_url_or_kind() {
    let named = |headers: &[(&str, &str)], url: &str| {
        with_headers(b"{}", headers).named_after(url).file_name()
    };
    let json = [("content-type", "application/json")];

    assert_eq!(
        named(
            &[(
                "content-disposition",
                "attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf; filename=\"resume.pdf\""
            )],
            "https://example.com/download",
        ),
        "résumé.pdf"
    );
    assert_eq!(
        named(
            &[(
                "content-disposition",
                "attachment; filename=\"../../report 1.csv\""
            )],
            "https://example.com/download",
        ),
        "report 1.csv"
    );
    assert_eq!(
        named(&json, "https://example.com/files/data?page=2"),
        "data.json"
    );
    assert_eq!(named(&json, "{{base}}/users/{{id}}"), "response.json");
    assert_eq!(named(&json, "https://example.com/"), "response.json");
    assert_eq!(named(&json, "https://example.com/a%20b.txt"), "a b.txt");
    assert_eq!(
        response(&bmp(1, 1), "image/bmp").file_name(),
        "response.bmp"
    );
    assert_eq!(with_headers(&[0, 1], &[]).file_name(), "response.bin");
}
