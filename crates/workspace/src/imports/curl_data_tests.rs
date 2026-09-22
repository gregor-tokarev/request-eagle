use request::{FormBody, Method, MultipartField};

use super::parse_import;

#[test]
fn curl_data_empty_arguments_match_native_accumulated_body_rules() {
    // Verified with cURL 8.7.1 on a local HTTP server for every flag pairing.
    for first in [
        "-d",
        "--data-raw",
        "--data-binary",
        "--data-ascii",
        "--data-urlencode",
    ] {
        for subsequent in [
            "-d",
            "--data-raw",
            "--data-binary",
            "--data-ascii",
            "--data-urlencode",
        ] {
            for (values, expected) in [
                (vec!["", "a"], "a"),
                (vec!["a", ""], "a&"),
                (vec!["", ""], ""),
                (vec!["", "", "a"], "a"),
                (vec!["a", "", "b"], "a&&b"),
                (vec!["a", "", ""], "a&&"),
                (vec!["", "a", ""], "a&"),
            ] {
                let mut command = String::from("curl https://example.test");

                for (index, value) in values.iter().enumerate() {
                    let flag = if index == 0 { first } else { subsequent };
                    command.push_str(&format!(" {flag} '{value}'"));
                }

                let imported = parse_import(&command).unwrap();
                assert_eq!(imported[0].request.method, Method::Post);
                assert_eq!(
                    imported[0].request.body.as_deref(),
                    Some(expected.as_bytes()),
                    "{command}"
                );
            }
        }
    }

    let imported = parse_import(
        "curl -d '' -d '{\"a\":1}' -H 'Content-Type: application/json' https://example.test",
    )
    .unwrap();
    assert_eq!(
        imported[0].request.body.as_deref(),
        Some(b"{\"a\":1}".as_slice())
    );
}

#[test]
fn curl_json_empty_arguments_concatenate_without_separators() {
    for flags in [
        "--json '' --json '{\"a\":1}'",
        "--json '{\"a\":1}' --json ''",
        "--json '' --json '{\"a\":' --json '' --json '1}' --json ''",
    ] {
        let imported = parse_import(&format!("curl https://example.test {flags}")).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(b"{\"a\":1}".as_slice())
        );
    }

    let imported = parse_import("curl https://example.test --json '' --json ''").unwrap();
    assert_eq!(imported[0].request.method, Method::Post);
    assert_eq!(imported[0].request.body.as_deref(), Some(b"".as_slice()));
}

#[test]
fn curl_empty_json_arguments_do_not_hide_unsupported_mixed_data_modes() {
    for other in ["-d", "--data-raw", "--data-binary", "--data-urlencode"] {
        for flags in [
            format!("--json '' {other} 'a'"),
            format!("{other} '' --json 'a'"),
        ] {
            let error = parse_import(&format!("curl https://example.test {flags}")).unwrap_err();
            assert!(error.contains("cannot mix cURL --json"));
        }
    }
}

#[test]
fn curl_multipart_trims_only_its_six_ascii_whitespace_characters() {
    for whitespace in [' ', '\t', '\n', '\r', '\u{000b}', '\u{000c}'] {
        for (value, expected) in [
            (format!("{whitespace}hello{whitespace}"), "hello"),
            (whitespace.to_string(), ""),
        ] {
            let imported =
                parse_import(&format!("curl https://example.test -F 'field={value}'")).unwrap();
            assert_eq!(
                imported[0].request.form,
                Some(FormBody::Multipart(vec![MultipartField::Text {
                    name: "field".into(),
                    value: expected.into()
                }]))
            );
        }
    }
}

#[test]
fn curl_multipart_preserves_unicode_whitespace_in_text_and_upload_paths() {
    for whitespace in ['\u{00a0}', '\u{2003}', '\u{202f}', '\u{3000}'] {
        for value in [
            format!("{whitespace}hello{whitespace}"),
            whitespace.to_string(),
        ] {
            let imported = parse_import(&format!(
                "curl https://example.test -F 'field= \t{value}\r '"
            ))
            .unwrap();
            assert_eq!(
                imported[0].request.form,
                Some(FormBody::Multipart(vec![MultipartField::Text {
                    name: "field".into(),
                    value
                }]))
            );
        }

        let path = format!("/nonexistent/upload.txt{whitespace}");
        let imported =
            parse_import(&format!("curl https://example.test -F 'file=@{path}'")).unwrap();
        assert_eq!(
            imported[0].request.form,
            Some(FormBody::Multipart(vec![MultipartField::File {
                name: "file".into(),
                path: path.into()
            }]))
        );
    }
}

#[test]
fn curl_form_string_preserves_ascii_and_unicode_whitespace() {
    let value = " \t\n\r\u{000b}\u{000c}\u{00a0}hello\u{2003} ";
    let imported = parse_import(&format!(
        "curl https://example.test --form-string 'field={value}'"
    ))
    .unwrap();
    assert_eq!(
        imported[0].request.form,
        Some(FormBody::Multipart(vec![MultipartField::Text {
            name: "field".into(),
            value: value.into()
        }]))
    );
}

#[test]
fn curl_shell_keeps_non_posix_whitespace_inside_unquoted_arguments() {
    for whitespace in [
        '\r', '\u{000b}', '\u{000c}', '\u{00a0}', '\u{2003}', '\u{202f}', '\u{3000}',
    ] {
        for value in [
            format!("a{whitespace}--data-raw{whitespace}b"),
            format!("payload{whitespace}"),
        ] {
            let imported =
                parse_import(&format!("curl https://example.test --data-raw {value}")).unwrap();
            assert_eq!(imported[0].request.body.as_deref(), Some(value.as_bytes()));
        }
    }
}

#[test]
fn curl_shell_splits_only_space_tab_and_newline_without_affecting_quoted_whitespace() {
    assert_eq!(
        super::shell::words("curl\t--data-raw\n' a\t\nb ' https://example.test").unwrap(),
        ["curl", "--data-raw", " a\t\nb ", "https://example.test"]
    );

    let json = "\r\n{\"item\":[{\"request\":\"https://example.test\"}]}\r\n";
    assert!(parse_import(json).is_ok());
}

#[test]
fn curl_header_values_preserve_unicode_whitespace_and_trim_only_leading_http_ows() {
    for whitespace in [
        '\u{00a0}', '\u{0085}', '\u{2003}', '\u{202f}', '\u{3000}', '\u{feff}',
    ] {
        for value in [
            format!("{whitespace}credential{whitespace}"),
            whitespace.to_string(),
        ] {
            let imported =
                parse_import(&format!("curl https://example.test -H 'X-Key: \t{value}'")).unwrap();
            assert_eq!(imported[0].request.headers, [("X-Key".into(), value)]);
        }
    }
}
