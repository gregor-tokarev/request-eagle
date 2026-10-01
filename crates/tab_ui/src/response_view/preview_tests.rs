use std::time::Duration;

use gpui_kit::{
    AppContext as _, ClipboardEntry, Entity, ImageFormat, Modifiers, TestAppContext,
    VisualTestContext,
};
use request::{Execution, HeaderMap, HttpResponse, Response, StatusCode, Version};

use super::{
    ResponseContent, ResponseView,
    body::{Body, BodyMode},
    hex::HEX_LIMIT,
};

fn response(body: &[u8], headers: &[(&str, &str)]) -> ResponseContent {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.append(
            name.parse::<request::HeaderName>().unwrap(),
            value.parse().unwrap(),
        );
    }

    ResponseContent::new(Execution {
        scripts: Vec::new(),
        elapsed: Duration::from_millis(1),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers: map,
            body: body.to_vec(),
            metrics: request::HttpMetrics::default(),
        }),
    })
}

fn typed(body: &[u8], content_type: &str) -> ResponseContent {
    response(body, &[("content-type", content_type)])
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

fn open(
    content: ResponseContent,
    cx: &mut TestAppContext,
) -> (Entity<ResponseView>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            view
        });
        view = Some(response.clone());
        gpui_kit::component::Root::new(response, window, cx)
    });
    cx.run_until_parked();

    (view.unwrap(), cx)
}

#[test]
fn bodies_are_recognized_by_media_type_and_signature() {
    use BodyMode::*;

    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    let zip = b"PK\x03\x04\x14\0\0\0\x08\0\xff\xfe";
    let cases: [(ResponseContent, &str, &[BodyMode], BodyMode); 13] = [
        (
            typed(b"{\"a\":1}", "application/json"),
            "JSON",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"<!doctype html><p>Hi</p>", "text/html"),
            "HTML",
            &[Preview, Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"<!DOCTYPE html><p>Hi</p>", "text/plain"),
            "HTML",
            &[Preview, Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"<a><b>1</b></a>", "application/soap+xml"),
            "XML",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"let a = 1;", "text/javascript"),
            "JavaScript",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"a { color: red }", "text/css"),
            "CSS",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"a: 1", "application/yaml"),
            "YAML",
            &[Pretty, Raw, Hex],
            Pretty,
        ),
        (
            typed(b"caf\xe9", "text/plain; charset=iso-8859-1"),
            "Text",
            &[Raw, Hex],
            Raw,
        ),
        (
            typed(png, "application/octet-stream"),
            "PNG",
            &[Preview, Hex],
            Preview,
        ),
        (
            typed(
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
                "image/svg+xml",
            ),
            "SVG",
            &[Preview, Pretty, Raw, Hex],
            Preview,
        ),
        (
            typed(&pdf(1), "application/pdf"),
            "PDF",
            &[Preview, Hex],
            Preview,
        ),
        (
            typed(
                zip,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            "Binary",
            &[Hex],
            Hex,
        ),
        (response(&[0, 1, 2, 255], &[]), "Binary", &[Hex], Hex),
    ];

    for (content, label, modes, default) in cases {
        assert_eq!(content.label(), label);
        assert_eq!(content.modes(), modes, "{label}");
        assert_eq!(content.default_mode(), default, "{label}");
        assert_eq!(content.binary, content.raw.is_empty(), "{label}");
    }

    // Undeclared text stays text, even when it is cut inside a character.
    let text = "é".repeat(5_000);
    assert!(!response(text.as_bytes(), &[]).binary);
    assert_eq!(typed(b"caf\xe9", "text/plain").raw, "caf\u{fffd}");
    // A named charset decodes the text.
    assert_eq!(
        typed(b"caf\xe9", "text/plain; charset=\"ISO-8859-1\"").raw,
        "café"
    );
    assert_eq!(
        typed(
            b"\xcf\xf0\xe8\xe2\xe5\xf2",
            "text/html;charset=windows-1251"
        )
        .raw,
        "Привет"
    );
    assert_eq!(
        typed(b"<a><b x=\"1\">text</b><c/></a>", "text/xml")
            .pretty
            .as_deref(),
        Some("<a>\n  <b x=\"1\">text</b>\n  <c/>\n</a>")
    );
    // Malformed XML is still highlighted as it was received.
    assert_eq!(
        typed(b"<a><b></a>", "application/xml").pretty.as_deref(),
        Some("<a><b></a>")
    );
}

#[test]
fn saved_bodies_are_named_by_the_server_url_or_kind() {
    let named =
        |headers: &[(&str, &str)], url: &str| response(b"{}", headers).named_after(url).file_name();
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
        response(&bmp(1, 1), &[("content-type", "image/bmp")]).file_name(),
        "response.bmp"
    );
    assert_eq!(response(&[0, 1], &[]).file_name(), "response.bin");
}

#[gpui_kit::test]
fn binary_bodies_show_their_bytes_instead_of_text(cx: &mut TestAppContext) {
    let bytes: Vec<u8> = (0..=255).collect();
    let (view, cx) = open(typed(&bytes, "application/octet-stream"), cx);

    cx.read(|cx| {
        let view = view.read(cx);
        assert_eq!(view.mode, BodyMode::Hex);
        let source = view.body.as_ref().unwrap().raw().read(cx).source.clone();
        assert!(source.starts_with(
            "00000000  00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f  ................\n"
        ));
        assert!(source.contains(
            "00000040  40 41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f  @ABCDEFGHIJKLMNO\n"
        ));
        assert_eq!(source.lines().count(), 16);
    });
    assert!(cx.debug_bounds("response-copy").is_none());
    assert!(cx.debug_bounds("response-wrap").is_none());
    assert!(cx.debug_bounds("response-hex-limit").is_none());
    assert!(cx.debug_bounds("response-save").is_some());

    // A large body's dump covers its start.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let large = typed(&vec![0; HEX_LIMIT + 1], "application/octet-stream");
            view.finish(Ok(large), window, cx);
        })
    });
    cx.read(|cx| {
        let source = view
            .read(cx)
            .body
            .as_ref()
            .unwrap()
            .raw()
            .read(cx)
            .source
            .clone();
        assert_eq!(source.lines().count(), HEX_LIMIT / 16);
    });
    assert!(cx.debug_bounds("response-hex-limit").is_some());
}

#[gpui_kit::test]
fn images_preview_and_copy_as_images(cx: &mut TestAppContext) {
    let (view, cx) = open(typed(&bmp(2, 3), "image/bmp"), cx);

    cx.read(|cx| {
        let Some(Body::Image(preview)) = &view.read(cx).body else {
            panic!("expected the image preview");
        };
        let Some(Ok(image)) = &preview.read(cx).decoded else {
            panic!("the image decodes");
        };
        assert_eq!(image.size(0), gpui_kit::size(2.into(), 3.into()));
    });
    assert!(cx.debug_bounds("response-image-content").is_some());

    let copy = cx.debug_bounds("response-copy").unwrap();
    cx.simulate_click(copy.center(), Modifiers::default());
    let Some(ClipboardEntry::Image(image)) =
        cx.read_from_clipboard().unwrap().entries().first().cloned()
    else {
        panic!("the image is copied");
    };
    assert_eq!(image.format, ImageFormat::Bmp);
    assert_eq!(image.bytes, bmp(2, 3));

    // Undecodable bytes explain themselves instead of showing nothing.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.finish(Ok(typed(b"not an image", "image/png")), window, cx);
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("response-image").is_some());
    assert!(cx.debug_bounds("response-image-content").is_none());
    cx.read(|cx| {
        let Some(Body::Image(preview)) = &view.read(cx).body else {
            panic!("expected the image preview");
        };
        assert!(matches!(preview.read(cx).decoded, Some(Err(_))));
    });
}

#[gpui_kit::test]
fn pdf_pages_render_as_they_show(cx: &mut TestAppContext) {
    let (view, cx) = open(typed(&pdf(3), "application/pdf"), cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();

    let preview = cx.read(|cx| match &view.read(cx).body {
        Some(Body::Pdf(preview)) => preview.clone(),
        _ => panic!("expected the PDF preview"),
    });
    cx.read(|cx| {
        assert_eq!(preview.read(cx).page_count(), Some(3));
        assert!(preview.read(cx).rendered_pages() > 0);
    });
    let page = cx.debug_bounds("response-pdf-page-0").unwrap();
    assert!((page.size.width / page.size.height - 2.).abs() < 0.05);

    // Hex still shows the document's bytes.
    cx.update(|window, cx| view.update(cx, |view, cx| view.show(BodyMode::Hex, window, cx)));
    cx.read(|cx| {
        let source = view
            .read(cx)
            .body
            .as_ref()
            .unwrap()
            .raw()
            .read(cx)
            .source
            .clone();
        assert!(source.starts_with("00000000  25 50 44 46 2d 31 2e 34"));
    });

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.finish(Ok(typed(b"%PDF-1.4 broken", "application/pdf")), window, cx);
        })
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let Some(Body::Pdf(preview)) = &view.read(cx).body else {
            panic!("expected the PDF preview");
        };
        assert_eq!(preview.read(cx).page_count(), None);
    });
}

#[gpui_kit::test]
fn html_shows_its_source_and_previews_the_page(cx: &mut TestAppContext) {
    let html = "<!doctype html><h1>Request Eagle</h1><p>Previewed <b>page</b></p>";
    let (view, cx) = open(typed(html.as_bytes(), "text/html; charset=utf-8"), cx);

    cx.read(|cx| {
        let Some(Body::Pretty(editor)) = &view.read(cx).body else {
            panic!("HTML starts as highlighted source");
        };
        assert_eq!(editor.read(cx).0.read(cx).value(), html);
    });

    let format = cx.debug_bounds("response-format").unwrap();
    cx.simulate_click(format.center(), Modifiers::default());
    cx.simulate_keystrokes("down enter");
    cx.read(|cx| assert_eq!(view.read(cx).mode, BodyMode::Preview));
    assert!(cx.debug_bounds("response-html").is_some());
    // Wrapping and search apply to text only.
    assert!(cx.debug_bounds("response-wrap").is_none());
    assert!(cx.debug_bounds("response-copy").is_some());
}

#[gpui_kit::test]
fn saving_writes_the_received_bytes(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("saved.png");
    let bytes: Vec<u8> = (0..=255).rev().collect();
    let (view, cx) = open(typed(&bytes, "application/octet-stream"), cx);

    let save = cx.debug_bounds("response-save").unwrap();
    cx.simulate_click(save.center(), Modifiers::default());
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();

    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    cx.read(|cx| {
        assert_eq!(
            view.read(cx).saved.as_ref().unwrap().as_ref().unwrap(),
            &target
        )
    });
    assert!(cx.debug_bounds("response-saved").is_some());

    // A failed write says why.
    let save = cx.debug_bounds("response-save").unwrap();
    cx.simulate_click(save.center(), Modifiers::default());
    cx.simulate_new_path_selection(|_| Some(directory.path().join("missing/saved.png")));
    cx.run_until_parked();
    assert!(cx.debug_bounds("response-save-error").is_some());

    // A cancelled dialog changes nothing, and a new response clears the result.
    let save = cx.debug_bounds("response-save").unwrap();
    cx.simulate_click(save.center(), Modifiers::default());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert!(cx.debug_bounds("response-save-error").is_some());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.finish(Ok(typed(b"ok", "text/plain")), window, cx)
        })
    });
    assert!(cx.debug_bounds("response-save-error").is_none());
}
