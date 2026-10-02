use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use gpui_kit::http_client::{AsyncBody, FakeHttpClient, Response};

use super::{PreparedUpdate, download_update, remove_previous_version, verify_sha256};

/// What the installer left after replacing an app marked "old" with one
/// marked "new".
struct Installation {
    _root: tempfile::TempDir,
    app: PathBuf,
    work_dir: PathBuf,
    opened: Vec<PathBuf>,
}

impl Installation {
    fn version(&self) -> String {
        fs::read_to_string(self.app.join("version")).unwrap()
    }
}

/// Runs the installer as if Request Eagle had just quit. The fake `open`
/// records each bundle it opens, then runs `on_open` with the bundle as `$1`.
/// `commands` replace other tools the installer uses.
fn install(on_open: &str, commands: &[(&str, &str)]) -> Installation {
    let root = tempfile::tempdir().unwrap();
    // The installer must escape this path for pkill's regular expression.
    let app = root.path().join("Apps (1)/Request Eagle.app");
    let work_dir = tempfile::tempdir_in(root.path()).unwrap();
    let new_app = work_dir.path().join("Request Eagle.app");
    let bin = root.path().join("bin");
    let opened = root.path().join("opened");

    for (bundle, version) in [(&app, "old"), (&new_app, "new")] {
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("version"), version).unwrap();
        executable(
            &bundle.join("Contents/MacOS/request-eagle"),
            "while :; do sleep 1; done",
        );
    }

    fs::create_dir(&bin).unwrap();
    executable(
        &bin.join("open"),
        &format!("echo \"$1\" >> '{}'\n{on_open}", opened.display()),
    );

    for (name, script) in commands {
        executable(&bin.join(name), script);
    }

    // A process that already quit stands in for the running app.
    let mut quit = Command::new("true").spawn().unwrap();
    quit.wait().unwrap();

    let update = PreparedUpdate {
        current_app: app.clone(),
        new_app,
        work_dir,
    };
    let work_dir = update.work_dir.path().to_owned();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    update
        .installer(quit.id(), Duration::from_secs(1))
        .env("PATH", path)
        .status()
        .unwrap();

    let opened = fs::read_to_string(opened)
        .unwrap()
        .lines()
        .map(PathBuf::from)
        .collect();

    Installation {
        _root: root,
        app,
        work_dir,
        opened,
    }
}

fn executable(path: &Path, script: &str) {
    fs::write(path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_new_version_that_confirms_its_start_stays_installed() {
    // Confirming removes the backup, as `confirm_startup` does.
    let installation = install(r#"rm -rf "$1.previous""#, &[]);

    assert_eq!(installation.version(), "new");
    assert_eq!(installation.opened, [installation.app.as_path()]);
    assert!(!installation.app.with_extension("app.previous").exists());
    assert!(!installation.work_dir.exists());
}

#[test]
fn a_new_version_that_hangs_at_startup_is_stopped_and_rolled_back() {
    let installation = install(
        r#"
if [ "$(cat "$1/version")" = new ]; then
  "$1/Contents/MacOS/request-eagle" &
  echo $! > "$1.pid"
fi"#,
        &[],
    );

    assert_eq!(installation.version(), "old");
    assert_eq!(installation.opened, [installation.app.as_path(); 2]);
    assert!(!installation.app.with_extension("app.previous").exists());
    assert!(!installation.work_dir.exists());

    let hung = fs::read_to_string(installation.app.with_extension("app.pid")).unwrap();
    let state = Command::new("ps")
        .args(["-o", "stat=", "-p", hung.trim()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&state.stdout);
    // A stopped process stays a zombie where nothing reaps orphans, such as
    // in a container without an init process.
    assert!(
        state.trim().is_empty() || state.trim().starts_with('Z'),
        "The hung version must be stopped, but it is in state {state}"
    );
}

#[test]
fn an_app_that_cannot_be_moved_aside_stays_installed() {
    let installation = install("", &[("mv", "exit 1")]);

    assert_eq!(installation.version(), "old");
    assert_eq!(installation.opened, [installation.app.as_path()]);
    assert!(!installation.work_dir.exists());
}

#[test]
fn starting_removes_the_previous_version_without_leftovers() {
    let directory = tempfile::tempdir().unwrap();
    let app = directory.path().join("Request Eagle.app");
    let backup = directory.path().join("Request Eagle.app.previous");
    fs::create_dir(&app).unwrap();
    fs::create_dir_all(backup.join("Contents/MacOS")).unwrap();
    fs::write(backup.join("Contents/MacOS/request-eagle"), "previous").unwrap();

    remove_previous_version(&app).unwrap();
    // Later launches have nothing to confirm.
    remove_previous_version(&app).unwrap();

    let entries: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, ["Request Eagle.app"]);
}

#[test]
fn downloads_report_bytes_with_or_without_a_content_length() {
    smol::block_on(async {
        let archive = vec![42; 150_000];

        for content_length in [None, Some("150000"), Some("0"), Some("invalid")] {
            let http = FakeHttpClient::create({
                let archive = archive.clone();

                move |_| {
                    let mut response = Response::builder().status(200);

                    if let Some(length) = content_length {
                        response = response.header("content-length", length);
                    }

                    let body = AsyncBody::from_reader(smol::io::Cursor::new(archive.clone()));

                    async move { Ok(response.body(body).unwrap()) }
                }
            });
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("update.zip");
            let mut progress = Vec::new();

            download_update(
                "https://example.test/update.zip",
                &destination,
                http,
                |bytes, total| {
                    progress.push((bytes, total));
                },
            )
            .await
            .unwrap();

            let total = (content_length == Some("150000")).then_some(150_000);

            assert_eq!(progress.first(), Some(&(0, total)));
            assert_eq!(progress.last(), Some(&(150_000, total)));
            assert!(progress.windows(2).all(|pair| pair[0].0 <= pair[1].0));
            assert_eq!(std::fs::read(destination).unwrap(), archive);
        }
    });
}

#[test]
fn truncated_downloads_are_rejected() {
    smol::block_on(async {
        let http = FakeHttpClient::create(|_| async {
            Ok(Response::builder()
                .status(200)
                .header("content-length", "1000")
                .body("partial archive".into())
                .unwrap())
        });
        let directory = tempfile::tempdir().unwrap();

        let error = download_update(
            "https://example.test/update.zip",
            &directory.path().join("update.zip"),
            http,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(error.contains("incomplete"));
    });
}

#[test]
fn failed_downloads_preserve_the_server_error_without_writing_an_archive() {
    smol::block_on(async {
        let http = FakeHttpClient::create(|_| async {
            Ok(Response::builder()
                .status(503)
                .body("Release service unavailable".into())
                .unwrap())
        });
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("update.zip");

        let error = download_update(
            "https://example.test/update.zip",
            &destination,
            http,
            |_, _| {
                panic!("A failed response must not report download progress");
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error, "Release service unavailable");
        assert!(!destination.exists());
    });
}

#[test]
fn checksum_must_match_before_an_update_can_be_prepared() {
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("update.zip");
    std::fs::write(&archive, b"abc").unwrap();

    assert!(
        verify_sha256(
            &archive,
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
        )
        .is_ok()
    );
    assert!(verify_sha256(&archive, "invalid").is_err());
}
