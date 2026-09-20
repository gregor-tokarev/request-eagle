use gpui_kit::{
    AppContext as _, TestAppContext,
    http_client::{AsyncBody, FakeHttpClient, Response},
};

use super::{PreparedUpdate, download_update, verify_sha256};
use crate::{UpdateStatus, Updater};

#[gpui_kit::test]
fn staged_files_wait_for_an_explicit_relaunch_after_verification(cx: &mut TestAppContext) {
    let work_dir = tempfile::tempdir().unwrap();
    let staged_archive = work_dir.path().join("update.zip");
    std::fs::write(&staged_archive, b"staged update").unwrap();

    let updater = cx.new(|_| Updater {
        current_version: "1.2.3",
        status: UpdateStatus::Verifying("99.0.0".into()),
        prepared_update: Some(PreparedUpdate {
            current_app: work_dir.path().join("installed.app"),
            new_app: work_dir.path().join("new.app"),
            work_dir,
        }),
    });

    updater.update(cx, |updater, cx| {
        updater.relaunch(cx);
        assert!(updater.prepared_update.is_some());

        updater.status = UpdateStatus::Ready("99.0.0".into());
        updater.check(cx);
        updater.download(cx);
    });
    cx.run_until_parked();

    updater.read_with(cx, |updater, _| {
        assert!(matches!(updater.status(), UpdateStatus::Ready(_)));
        assert!(updater.prepared_update.is_some());
        assert!(staged_archive.exists());
    });

    cx.update(|_| drop(updater));

    assert!(
        !staged_archive.exists(),
        "Discarded updates must clean up their staging files"
    );
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
