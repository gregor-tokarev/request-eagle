use request::Method;

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
