use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn help_and_version_use_stdout_and_exit_successfully() {
    for args in [
        vec![],
        vec!["--help"],
        vec!["-h"],
        vec!["help"],
        vec!["call", "--help"],
        vec!["help", "call"],
        vec!["schema", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("Usage: request-eagle-cli"), "{help}");
        assert!(help.contains("--data-dir") && help.contains("--collections-dir"));
        if args.contains(&"call") {
            assert!(help.contains("<JSON>") && help.contains("stdin"));
        }
    }

    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let version = String::from_utf8(output.stdout).unwrap();
        assert!(version.starts_with("request-eagle-cli "));
        assert!(version.ends_with(" (format 1)\n"), "{version}");
    }
}

#[test]
fn invalid_arguments_return_json_without_touching_storage() {
    let directory = tempdir().unwrap();
    let data = directory.path().join("untouched");
    let create = r#"{"command":"collections.create"}"#;

    for args in [
        vec![],
        vec!["--unknown"],
        vec!["--collections-dir"],
        vec!["unknown"],
        vec!["schema", "extra"],
        vec!["call"],
        vec!["call", create, "extra"],
        vec!["call", create, "--unknown"],
        vec!["call", create, "--collections-dir"],
        vec!["call", "{"],
        vec!["call", "{}"],
        vec!["--data-dir", "duplicate", "call", create],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
            .arg("--data-dir")
            .arg(&data)
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["version"], 1);
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "invalid_input");
        assert!(!result["error"]["message"].as_str().unwrap().is_empty());
        assert!(!data.exists(), "{args:?} changed storage");
    }
}

#[test]
fn directory_options_work_before_and_after_the_subcommand() {
    for placement in 0..3 {
        let directory = tempdir().unwrap();
        let data = directory.path().join("data with spaces");
        let collections = directory.path().join("selected collections");
        let ignored = directory.path().join("ignored");
        let options = [
            format!("--data-dir={}", data.display()),
            format!("--collections-dir={}", collections.display()),
        ];
        let mut args = vec![
            "call".to_owned(),
            r#"{"command":"collections.create"}"#.to_owned(),
        ];
        args.splice(placement..placement, options);

        let output = Command::new(env!("CARGO_BIN_EXE_request-eagle-cli"))
            .args(&args)
            .env("REQUEST_EAGLE_COLLECTIONS_DIR", &ignored)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(
            result["result"]["path"],
            collections.join("New Collection").to_str().unwrap()
        );
        assert!(collections.join("New Collection").is_dir());
        assert!(!ignored.exists());
        assert!(!data.join("collections").exists());
    }
}
