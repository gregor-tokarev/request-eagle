use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::http_client::{AsyncBody, HttpClient};
use rustix::fs::{Access, AtFlags, CWD, accessat};
use sha2::{Digest, Sha256};
use smol::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

use super::{UpdateManifest, UpdateStatus};

#[cfg(test)]
mod tests;

const EXPECTED_TEAM_ID: &str = "P2M3JQ4DR5";

pub(super) struct PreparedUpdate {
    current_app: PathBuf,
    new_app: PathBuf,
    work_dir: tempfile::TempDir,
}

impl PreparedUpdate {
    pub(super) fn launch_installer(self) -> Result<(), String> {
        launch_installer(&self.current_app, &self.new_app, self.work_dir.path())?;

        // The installer now owns these files and removes them after relaunch.
        let _ = self.work_dir.keep();

        Ok(())
    }
}

pub(super) async fn download_and_prepare_update(
    manifest: &UpdateManifest,
    http_client: Arc<dyn HttpClient>,
    progress: smol::channel::Sender<UpdateStatus>,
) -> Result<PreparedUpdate, String> {
    let current_app = current_app_bundle()?;
    let install_dir = current_app
        .parent()
        .ok_or_else(|| "The installed app has no parent directory.".to_string())?;

    if accessat(
        CWD,
        install_dir,
        Access::WRITE_OK | Access::EXEC_OK,
        AtFlags::EACCESS,
    )
    .is_err()
    {
        return Err(format!(
            "{} is not writable. Move Request Eagle to a folder owned by your user and try again.",
            install_dir.display()
        ));
    }

    let work_dir = tempfile::Builder::new()
        .prefix("request-eagle-update-")
        .tempdir()
        .map_err(|error| format!("Could not create the update directory: {error}"))?;

    let archive = work_dir.path().join("Request Eagle.zip");
    download_update(
        &manifest.url,
        &archive,
        http_client,
        |downloaded_bytes, total_bytes| {
            let _ = progress.try_send(UpdateStatus::Downloading {
                version: manifest.version.clone(),
                downloaded_bytes,
                total_bytes,
            });
        },
    )
    .await?;

    let _ = progress.try_send(UpdateStatus::Verifying(manifest.version.clone()));

    verify_sha256(&archive, &manifest.sha256)?;

    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&archive)
        .arg(work_dir.path())
        .status()
        .map_err(|error| format!("Could not extract the update: {error}"))?;

    if !status.success() {
        return Err("The update archive could not be extracted.".into());
    }

    let new_app = work_dir.path().join("Request Eagle.app");
    verify_apple_signature(&new_app)?;

    Ok(PreparedUpdate {
        current_app,
        new_app,
        work_dir,
    })
}

async fn download_update(
    url: &str,
    destination: &Path,
    http_client: Arc<dyn HttpClient>,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let mut response = http_client
        .get(url, AsyncBody::empty(), true)
        .await
        .map_err(|error| format!("Could not download the update: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let mut body = Vec::new();
        response
            .body_mut()
            .read_to_end(&mut body)
            .await
            .map_err(|error| format!("Could not read GitHub's error response: {error}"))?;

        let detail = String::from_utf8_lossy(&body).trim().to_string();

        return Err(if detail.is_empty() {
            format!("GitHub returned {status} for the update archive.")
        } else {
            detail
        });
    }

    let total_bytes = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|total| *total > 0);
    let mut downloaded_bytes = 0;
    let mut last_progress = Instant::now();

    on_progress(downloaded_bytes, total_bytes);

    let mut destination = File::create(destination)
        .await
        .map_err(|error| format!("Could not create the update archive: {error}"))?;

    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = response
            .body_mut()
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Could not read the update download: {error}"))?;

        if bytes_read == 0 {
            break;
        }

        destination
            .write_all(&buffer[..bytes_read])
            .await
            .map_err(|error| format!("Could not write the update archive: {error}"))?;

        downloaded_bytes += bytes_read as u64;

        // Keep large downloads responsive without redrawing for every chunk.
        if last_progress.elapsed() >= Duration::from_millis(100) {
            on_progress(downloaded_bytes, total_bytes);
            last_progress = Instant::now();
        }
    }

    destination
        .flush()
        .await
        .map_err(|error| format!("Could not finish writing the update archive: {error}"))?;

    on_progress(downloaded_bytes, total_bytes);

    if let Some(total) = total_bytes
        && downloaded_bytes != total
    {
        return Err("The update download was incomplete. Please try again.".into());
    }

    Ok(())
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("Could not read the update: {error}"))?;
    let actual = format!("{:x}", Sha256::digest(bytes));

    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err("The update checksum does not match the release manifest.".into())
    }
}

fn verify_apple_signature(app: &Path) -> Result<(), String> {
    if !app.join("Contents/MacOS/request-eagle").is_file() {
        return Err("The update does not contain a valid Request Eagle app bundle.".into());
    }

    let verify = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=2"])
        .arg(app)
        .output()
        .map_err(|error| format!("Could not verify the Apple signature: {error}"))?;

    if !verify.status.success() {
        return Err(format!(
            "Apple rejected the update signature: {}",
            String::from_utf8_lossy(&verify.stderr).trim()
        ));
    }

    let details = Command::new("/usr/bin/codesign")
        .args(["--display", "--verbose=4"])
        .arg(app)
        .output()
        .map_err(|error| format!("Could not inspect the Apple signature: {error}"))?;
    let details = String::from_utf8_lossy(&details.stderr);

    if !details.contains(&format!("TeamIdentifier={EXPECTED_TEAM_ID}"))
        || !details.contains("Authority=Developer ID Application:")
    {
        return Err("The update was not signed by the expected Apple Developer team.".into());
    }

    let gatekeeper = Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute", "--verbose=2"])
        .arg(app)
        .output()
        .map_err(|error| format!("Could not ask Gatekeeper to verify the update: {error}"))?;

    if !gatekeeper.status.success() {
        return Err(format!(
            "Gatekeeper rejected the update: {}",
            String::from_utf8_lossy(&gatekeeper.stderr).trim()
        ));
    }

    Ok(())
}

fn launch_installer(current_app: &Path, new_app: &Path, work_dir: &Path) -> Result<(), String> {
    const SCRIPT: &str = r#"
pid="$1"
current_app="$2"
new_app="$3"
work_dir="$4"
backup="${current_app}.previous"

while kill -0 "$pid" 2>/dev/null; do
  sleep 0.2
done

rm -rf "$backup"
if mv "$current_app" "$backup" && mv "$new_app" "$current_app"; then
  open "$current_app"
  rm -rf "$backup" "$work_dir"
else
  rm -rf "$current_app"
  mv "$backup" "$current_app"
  open "$current_app"
fi
"#;

    Command::new("/bin/sh")
        .args(["-c", SCRIPT, "request-eagle-updater"])
        .arg(std::process::id().to_string())
        .arg(current_app)
        .arg(new_app)
        .arg(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start the update installer: {error}"))?;

    Ok(())
}

pub(super) fn current_app_bundle() -> Result<PathBuf, String> {
    let executable =
        env::current_exe().map_err(|error| format!("Could not locate Request Eagle: {error}"))?;

    // Request Eagle.app/Contents/MacOS/request-eagle → Request Eagle.app
    let app = executable
        .ancestors()
        .nth(3)
        .ok_or_else(|| "Request Eagle is not running from an app bundle.".to_string())?;

    if app.extension().and_then(|value| value.to_str()) != Some("app") {
        return Err("Automatic updates are only available from Request Eagle.app.".into());
    }

    Ok(app.to_path_buf())
}
