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
fn curl_shell_splits_space_and_tab_without_affecting_quoted_whitespace() {
    assert_eq!(
        super::shell::words("curl\t--data-raw ' a\t\nb ' https://example.test").unwrap(),
        ["curl", "--data-raw", " a\t\nb ", "https://example.test"]
    );

    let json = "\r\n{\"item\":[{\"request\":\"https://example.test\"}]}\r\n";
    assert!(parse_import(json).is_ok());
}

#[test]
fn curl_shell_rejects_unescaped_internal_command_newlines() {
    for command in [
        "curl https://example.test\n--data-raw payload",
        "curl https://example.test \t\n--data-raw payload",
        "curl https://example.test\n\n--data-raw payload",
        "curl\nhttps://example.test",
    ] {
        let error = parse_import(command).unwrap_err();
        assert!(error.contains("backslash before a newline"), "{error}");
    }
}

#[test]
fn curl_shell_keeps_outer_whitespace_continuations_and_quoted_newlines() {
    let imported = parse_import(" \t\ncurl https://example.test\n\t ").unwrap();
    assert_eq!(imported[0].request.method, Method::Get);

    let imported = parse_import("curl https://example.test \\\n--data-raw pay\\\nload").unwrap();
    assert_eq!(
        imported[0].request.body.as_deref(),
        Some(b"payload".as_slice())
    );

    for value in ["'pay\nload'", "\"pay\nload\""] {
        let imported =
            parse_import(&format!("curl https://example.test --data-raw {value}")).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(b"pay\nload".as_slice())
        );
    }
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

#[test]
fn curl_explicit_methods_reject_spellings_that_would_change_on_send() {
    for method in ["get", "post", "DeLeTe", "pAtCh", "Head", "options"] {
        for flag in ["-X ", "--request=", "-X"] {
            let error =
                parse_import(&format!("curl https://example.test {flag}{method}")).unwrap_err();
            assert!(error.contains("canonical uppercase methods"), "{error}");
        }
    }

    for method in ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"] {
        let imported = parse_import(&format!("curl https://example.test -X {method}")).unwrap();
        assert_eq!(imported[0].request.method.as_str(), method);
    }

    assert!(parse_import("curl https://example.test -X CUSTOM").is_err());
}

#[test]
fn curl_rejects_paths_that_would_be_normalized_differently_on_send() {
    for path in ["/a/%2e/b", "/a/.%2E/b", "/a/%2e./b", "/a/%2E%2e/b"] {
        for host in ["https://example.test", "example.test", "{{base_url}}"] {
            let error = parse_import(&format!("curl '{host}{path}'")).unwrap_err();
            assert!(error.contains("encoded dot segments"), "{error}");
        }
    }

    for host in ["https://example.test", "example.test", "{{base_url}}"] {
        let error = parse_import(&format!("curl '{host}/a\\b'")).unwrap_err();
        assert!(error.contains("backslashes"), "{error}");
    }
}

#[test]
fn curl_retains_supported_path_and_query_spelling() {
    for path in [
        "/a/../b",
        "/a/./b",
        "/a/%2ejson/b",
        "/a/%2e%2e%2e/b",
        "/a/%252e%252e/b",
        "/a/%5cb",
        "/a?path=/%2e%2e/b\\c",
        "/a#/%2e%2e/b\\c",
    ] {
        let url = format!("https://example.test{path}");
        let imported = parse_import(&format!("curl '{url}'")).unwrap();
        assert_eq!(imported[0].request.path, url);
    }

    // cURL's default behavior also removes ordinary literal dot segments.
    let imported = parse_import("curl 'https://example.test/a/../b'").unwrap();
    assert_eq!(
        url::Url::parse(&imported[0].request.path).unwrap().path(),
        "/b"
    );
}

#[test]
fn curl_shell_rejects_unquoted_leading_tilde_expansion() {
    for value in [
        "~",
        "~/documents",
        "~root/documents",
        "~+/documents",
        "~-/documents",
    ] {
        let error =
            parse_import(&format!("curl https://example.test --data-raw {value}")).unwrap_err();
        assert!(error.contains("tilde expansion"), "{error}");
    }
}

#[test]
fn curl_shell_preserves_quoted_escaped_and_nonleading_literal_tildes() {
    for value in [
        "'~/documents'",
        "\"~/documents\"",
        "\\~/documents",
        "''~/documents",
        "'~'/documents",
    ] {
        let imported =
            parse_import(&format!("curl https://example.test --data-raw {value}")).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(b"~/documents".as_slice())
        );
    }

    let imported = parse_import("curl https://example.test --data-raw documents/~name").unwrap();
    assert_eq!(
        imported[0].request.body.as_deref(),
        Some(b"documents/~name".as_slice())
    );
}

#[test]
fn curl_shell_rejects_unquoted_filename_expansion() {
    for value in ["*.txt", "file?.txt", "[ab].txt", "'prefix'*.txt"] {
        for flags in ["", "--globoff "] {
            let error = parse_import(&format!(
                "curl {flags}https://example.test --data-raw {value}"
            ))
            .unwrap_err();
            assert!(error.contains("Shell filename expansion"), "{error}");
        }
    }
}

#[test]
fn curl_shell_preserves_quoted_and_escaped_literal_wildcards() {
    for value in ["'*?[ab].txt'", "\"*?[ab].txt\"", "\\*\\?\\[ab].txt"] {
        let imported =
            parse_import(&format!("curl https://example.test --data-raw {value}")).unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(b"*?[ab].txt".as_slice())
        );
    }

    let imported = parse_import("curl 'https://example.test/?q=*'").unwrap();
    assert_eq!(imported[0].request.path, "https://example.test/?q=*");
}

#[test]
fn curl_data_urlencode_matches_native_bytes_for_raw_text() {
    // Captured with native cURL 8.7.1: every printable ASCII punctuation mark,
    // spaces, and multibyte UTF-8, with an explicit non-form Content-Type.
    let value = "AZaz09-._~ !\"#$%&'()*+,/:;<=>?@[\\]^`{|} é漢🙂";
    let encoded = "AZaz09-._~+%21%22%23%24%25%26%27%28%29%2A%2B%2C%2F%3A%3B%3C%3D%3E%3F%40%5B%5C%5D%5E%60%7B%7C%7D+%C3%A9%E6%BC%A2%F0%9F%99%82";

    for (argument, expected) in [
        (format!("={value}"), encoded.to_string()),
        (
            format!("encoded%20name[]={value}"),
            format!("encoded%20name[]={encoded}"),
        ),
        (
            value.replace(['=', '@'], ""),
            "AZaz09-._~+%21%22%23%24%25%26%27%28%29%2A%2B%2C%2F%3A%3B%3C%3E%3F%5B%5C%5D%5E%60%7B%7C%7D+%C3%A9%E6%BC%A2%F0%9F%99%82".into(),
        ),
    ] {
        let quoted_argument = argument.replace('\'', "'\"'\"'");
        let imported = parse_import(&format!(
            "curl https://example.test --data-urlencode '{quoted_argument}' -H 'Content-Type: text/plain'"
        ))
        .unwrap();
        assert_eq!(
            imported[0].request.body.as_deref(),
            Some(expected.as_bytes())
        );
        assert_eq!(
            imported[0].request.headers,
            [("Content-Type".into(), "text/plain".into())]
        );
        assert!(imported[0].request.form.is_none());
    }
}

#[test]
fn curl_multipart_rejects_nameless_text_and_file_parts() {
    for flag in ["-F", "--form", "--form-string"] {
        for value in ["=hello", "=", "=@/tmp/upload.txt", "=</tmp/input.txt"] {
            let error =
                parse_import(&format!("curl https://example.test {flag} '{value}'")).unwrap_err();
            assert!(error.contains("Nameless cURL multipart fields"), "{error}");
        }
    }
}

#[test]
fn curl_multipart_retains_named_text_and_file_parts() {
    for flag in ["-F", "--form", "--form-string"] {
        for value in ["hello", ""] {
            let imported =
                parse_import(&format!("curl https://example.test {flag} 'named={value}'")).unwrap();
            assert_eq!(
                imported[0].request.form,
                Some(FormBody::Multipart(vec![MultipartField::Text {
                    name: "named".into(),
                    value: value.into(),
                }]))
            );
        }
    }

    let imported = parse_import("curl https://example.test -F 'file=@/tmp/upload.txt'").unwrap();
    assert_eq!(
        imported[0].request.form,
        Some(FormBody::Multipart(vec![MultipartField::File {
            name: "file".into(),
            path: "/tmp/upload.txt".into(),
        }]))
    );
}

#[test]
fn curl_get_reuses_only_existing_query_separators() {
    // Native cURL appends after an empty query or trailing '&', but a '?' in
    // an existing query value or an '&' in the path is not a separator.
    for (path, expected) in [
        ("/", "/?x=1"),
        ("/?", "/?x=1"),
        ("/?q=1", "/?q=1&x=1"),
        ("/?q=1&", "/?q=1&x=1"),
        ("/?q=?", "/?q=?&x=1"),
        ("/path&", "/path&?x=1"),
    ] {
        for fragment in ["", "#details"] {
            let imported = parse_import(&format!(
                "curl 'https://example.test{path}{fragment}' -G -d x=1"
            ))
            .unwrap();
            assert_eq!(
                imported[0].request.path,
                format!("https://example.test{expected}{fragment}")
            );
            assert_eq!(imported[0].request.method, Method::Get);
            assert!(imported[0].request.body.is_none());
        }
    }
}

#[test]
fn curl_multipart_rejects_name_characters_with_different_native_escaping() {
    for character in (0_u8..=31)
        .chain([127, b'\\'])
        .map(char::from)
        .filter(|character| !matches!(character, '\r' | '\n'))
    {
        for flag in ["-F", "--form", "--form-string"] {
            let error = parse_import(&format!(
                "curl https://example.test {flag} 'prefix{character}suffix=hello'"
            ))
            .unwrap_err();
            assert!(error.contains("names and filenames"), "{error}");
        }
    }
}

#[test]
fn curl_multipart_rejects_filename_characters_with_different_native_escaping() {
    for character in (0_u8..=31)
        .chain([127, b'\\'])
        .map(char::from)
        .filter(|character| !matches!(character, '\r' | '\n'))
    {
        let error = parse_import(&format!(
            "curl https://example.test -F 'file=@/tmp/prefix{character}suffix.txt'"
        ))
        .unwrap_err();
        assert!(error.contains("names and filenames"), "{error}");
    }
}

#[test]
fn curl_multipart_retains_matching_parameter_escaping_and_literal_values() {
    // Native cURL uses the same quote/CR/LF escaping and preserves UTF-8.
    let name = "UTF-8 é漢🙂 \"\r\n";
    let value = "literal\t\\value";

    for flag in ["-F", "--form-string"] {
        let imported = parse_import(&format!(
            "curl https://example.test {flag} '{name}={value}'"
        ))
        .unwrap();
        assert_eq!(
            imported[0].request.form,
            Some(FormBody::Multipart(vec![MultipartField::Text {
                name: name.into(),
                value: value.into(),
            }]))
        );
    }

    for path in [
        "/tmp/UTF-8 é漢🙂 \r\nfile.txt",
        "/tmp/folder\t\\name/upload-é.txt",
    ] {
        let imported =
            parse_import(&format!("curl https://example.test -F 'file=@{path}'")).unwrap();
        assert_eq!(
            imported[0].request.form,
            Some(FormBody::Multipart(vec![MultipartField::File {
                name: "file".into(),
                path: path.into(),
            }]))
        );
    }
}
