use gpui_kit::{
    Context, Task,
    http_client::{AsyncBody, HttpClient},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use smol::{future::FutureExt, io::AsyncReadExt};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const RELEASES: &str = "https://github.com/gregor-tokarev/request-eagle/releases/download";
const MAX_BINARY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum CliStatus {
    NotInstalled,
    Installing,
    Installed { version: String, path: PathBuf },
    Error(String),
}

pub struct CliInstaller {
    version: String,
    status: CliStatus,
    task: Option<Task<()>>,
}

#[derive(Deserialize, Serialize)]
struct Manifest {
    version: String,
    sha256: String,
    target: String,
}

pub fn cli_path() -> Result<PathBuf, String> {
    std::env::home_dir()
        .map(|home| home.join(".request-eagle/bin/request-eagle-cli"))
        .ok_or_else(|| "Cannot locate the home directory".into())
}

pub fn cli_target() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        _ => {
            Err("No prebuilt CLI for this platform. See the CLI source installation guide.".into())
        }
    }
}

impl CliInstaller {
    pub fn new(version: &str, cx: &mut Context<Self>) -> Self {
        let version = version.to_owned();
        let task = cx.background_executor().spawn(async { installed_status() });
        let task = cx.spawn(async move |this, cx| {
            let status = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = status;
                this.task = None;
                cx.notify();
            });
        });
        Self {
            version,
            status: CliStatus::NotInstalled,
            task: Some(task),
        }
    }

    pub fn status(&self) -> &CliStatus {
        &self.status
    }

    pub fn install(&mut self, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }
        self.status = CliStatus::Installing;
        let version = self.version.clone();
        let http = cx.http_client();
        let task = cx.background_executor().spawn(async move {
            download(&version, http)
                .or(async {
                    smol::Timer::after(Duration::from_secs(120)).await;
                    Err("CLI download timed out. Try again.".into())
                })
                .await
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let status = match task.await {
                Ok((version, path)) => CliStatus::Installed { version, path },
                Err(error) => CliStatus::Error(error),
            };
            let _ = this.update(cx, |this, cx| {
                this.status = status;
                this.task = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn installed_status() -> CliStatus {
    let result = (|| -> Result<CliStatus, String> {
        let path = cli_path()?;
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Ok(CliStatus::NotInstalled);
        }
        let manifest = managed_install(&path)?;
        Ok(CliStatus::Installed {
            version: manifest.version,
            path,
        })
    })();
    result.unwrap_or_else(CliStatus::Error)
}

fn managed_install(path: &Path) -> Result<Manifest, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BINARY_BYTES {
        return Err(
            "The CLI destination is not a managed executable. Move it before installing.".into(),
        );
    }
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(path.with_extension("json")).map_err(
            |_| "The CLI destination has no installation receipt. Move it before installing.",
        )?)
        .map_err(|e| e.to_string())?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    verify(&manifest, &bytes)?;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err("Installed CLI is not executable. Restore its execute permission.".into());
    }
    Ok(manifest)
}

async fn fetch(url: &str, max: u64, http: &Arc<dyn HttpClient>) -> Result<Vec<u8>, String> {
    let mut response = http
        .get(url, AsyncBody::empty(), true)
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "CLI download returned {}. A CLI release must exist for this app version.",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("CLI download exceeds the size limit".into());
    }
    Ok(bytes)
}

async fn download(version: &str, http: Arc<dyn HttpClient>) -> Result<(String, PathBuf), String> {
    download_to(version, http, cli_path()?).await
}

async fn download_to(
    version: &str,
    http: Arc<dyn HttpClient>,
    path: PathBuf,
) -> Result<(String, PathBuf), String> {
    semver::Version::parse(version).map_err(|e| e.to_string())?;
    let target = cli_target()?;
    let base = format!("{RELEASES}/v{version}/request-eagle-cli-{target}");
    let manifest_bytes = fetch(&format!("{base}.json"), 4096, &http).await?;
    let signature = fetch(&format!("{base}.sig"), 1024, &http).await?;
    verify_manifest(&manifest_bytes, &signature)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
    if manifest.target != target {
        return Err("CLI release target does not match this platform".into());
    }
    if manifest.version != version {
        return Err("CLI release version does not match this app".into());
    }
    let bytes = fetch(&base, MAX_BINARY_BYTES, &http).await?;
    verify(&manifest, &bytes)?;
    install_bytes(&path, &manifest, &bytes)?;
    Ok((version.into(), path))
}

fn verify_manifest(bytes: &[u8], signature: &[u8]) -> Result<(), String> {
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        include_bytes!("cli/signing-key.der"),
    )
    .verify(bytes, signature)
    .map_err(|_| "CLI release signature verification failed; nothing was installed".into())
}

fn verify(manifest: &Manifest, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty()
        || format!("{:x}", Sha256::digest(bytes)) != manifest.sha256.to_ascii_lowercase()
    {
        return Err("CLI checksum verification failed; nothing was installed".into());
    }
    Ok(())
}

fn install_bytes(path: &Path, manifest: &Manifest, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("CLI destination has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Do not follow a user-created symlink or replace an unrelated binary.
    match fs::symlink_metadata(path) {
        Ok(_) => {
            managed_install(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut binary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    binary.write_all(bytes).map_err(|e| e.to_string())?;
    binary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    binary.as_file().sync_all().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    super::install::verify_developer_signature(binary.path())?;

    let mut receipt = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    receipt
        .write_all(&serde_json::to_vec(manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    receipt.as_file().sync_all().map_err(|e| e.to_string())?;
    binary.persist(path).map_err(|e| e.to_string())?;
    receipt
        .persist(path.with_extension("json"))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("cli/fixtures/manifest.json");
    const SIGNATURE: &[u8] = include_bytes!("cli/fixtures/manifest.sig");

    #[test]
    fn authenticates_manifest_before_trusting_its_checksum() {
        verify_manifest(FIXTURE, SIGNATURE).unwrap();
        let mut tampered: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        tampered["sha256"] = serde_json::json!(format!("{:x}", Sha256::digest(b"attacker binary")));
        assert!(verify_manifest(&serde_json::to_vec(&tampered).unwrap(), SIGNATURE).is_err());
        assert!(verify_manifest(FIXTURE, &[0; 384]).is_err());
        assert!(verify_manifest(FIXTURE, &[]).is_err());
    }

    // Synthetic executable fixtures cannot carry an Apple Developer signature.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn download_checks_signature_version_target_and_hash_before_installing() {
        use gpui_kit::http_client::{FakeHttpClient, Response};

        smol::block_on(async {
            for scenario in ["valid", "version", "manifest", "signature", "binary"] {
                let requested_version = if scenario == "version" {
                    "2.0.0"
                } else {
                    "1.2.3"
                };
                let http = FakeHttpClient::create(move |request| {
                    let body = if request.uri().path().ends_with(".json") {
                        if scenario == "manifest" {
                            b"{}".to_vec()
                        } else {
                            FIXTURE.to_vec()
                        }
                    } else if request.uri().path().ends_with(".sig") {
                        if scenario == "signature" {
                            vec![0; 384]
                        } else {
                            SIGNATURE.to_vec()
                        }
                    } else if scenario == "binary" {
                        b"tampered executable".to_vec()
                    } else {
                        b"downloaded CLI fixture".to_vec()
                    };
                    async move { Ok(Response::builder().status(200).body(body.into()).unwrap()) }
                });
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("cli");
                let result = download_to(requested_version, http, path.clone()).await;
                assert_eq!(
                    result.is_ok(),
                    scenario == "valid",
                    "{scenario}: {result:?}"
                );
                assert_eq!(path.exists(), scenario == "valid");
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn verifies_installs_and_preserves_unmanaged_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cli");
        let bytes = b"executable fixture";
        let manifest = Manifest {
            version: "1.2.3".into(),
            target: cli_target().unwrap().into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        assert!(verify(&manifest, b"tampered").is_err());
        install_bytes(&path, &manifest, bytes).unwrap();
        assert_eq!(managed_install(&path).unwrap().version, "1.2.3");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fs::write(&path, b"user binary").unwrap();
        assert!(install_bytes(&path, &manifest, bytes).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"user binary");
    }
}
