//! How each platform's release package is found and recognized.

/// Only the macOS app replaces itself. The Windows installer and the Linux
/// packages are downloaded in the browser and installed by the system.
pub const INSTALLS_IN_APP: bool = cfg!(target_os = "macos");

/// The macOS installer keeps the previous version until the new one confirms
/// that it started. Elsewhere the system installer has already finished.
#[cfg(not(target_os = "macos"))]
pub fn confirm_startup() {}

/// The release file that describes the package this copy updates to, or
/// `None` when a package manager delivers its updates. macOS keeps the name
/// that versions before update tracks read.
#[cfg(target_os = "macos")]
pub(super) fn manifest() -> Option<&'static str> {
    Some("request-eagle-update.json")
}

#[cfg(windows)]
pub(super) fn manifest() -> Option<&'static str> {
    Some("request-eagle-update-windows.json")
}

/// A copy built from source is offered the Debian package when asked, as
/// before the other packages existed.
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn manifest() -> Option<&'static str> {
    match linux_package() {
        Some(LinuxPackage::AppImage) => Some("request-eagle-update-appimage.json"),
        Some(LinuxPackage::Rpm) => Some("request-eagle-update-rpm.json"),
        Some(LinuxPackage::Debian) | None => Some("request-eagle-update-debian.json"),
        Some(LinuxPackage::Managed) => None,
    }
}

/// Whether this copy was installed from a release package rather than built
/// from source, so it looks for updates on its own.
#[cfg(target_os = "macos")]
pub(super) fn installed() -> bool {
    super::install::current_app_bundle().is_ok()
}

/// The installer leaves its uninstaller beside the app.
#[cfg(windows)]
pub(super) fn installed() -> bool {
    std::env::current_exe()
        .is_ok_and(|executable| executable.with_file_name("unins000.exe").is_file())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn installed() -> bool {
    matches!(
        linux_package(),
        Some(LinuxPackage::AppImage | LinuxPackage::Debian | LinuxPackage::Rpm)
    )
}

#[cfg(not(any(target_os = "macos", windows)))]
#[derive(Clone, Copy)]
enum LinuxPackage {
    AppImage,
    Debian,
    Rpm,
    /// Installed by a package manager that delivers updates itself, such as
    /// pacman for the AUR package.
    Managed,
}

/// The package is recognized once. A package installed over this copy while
/// it runs renames its executable, which must not change the answer.
#[cfg(not(any(target_os = "macos", windows)))]
fn linux_package() -> Option<LinuxPackage> {
    static PACKAGE: std::sync::OnceLock<Option<LinuxPackage>> = std::sync::OnceLock::new();

    *PACKAGE.get_or_init(recognize_linux_package)
}

/// The AppImage runtime tells the app where its image is mounted. An
/// AppImage terminal passes the same variables to the apps it starts, so the
/// executable has to run from that mount. The Debian and RPM packages
/// install the app at one path and name themselves in a marker file, which
/// older Debian packages did not leave; a package manager names itself there
/// too.
#[cfg(not(any(target_os = "macos", windows)))]
fn recognize_linux_package() -> Option<LinuxPackage> {
    let executable = std::env::current_exe().ok()?;

    if let Some(mount) = std::env::var_os("APPDIR")
        && std::env::var_os("APPIMAGE").is_some()
        && executable.starts_with(&mount)
    {
        return Some(LinuxPackage::AppImage);
    }

    if executable != std::path::Path::new("/usr/bin/request-eagle") {
        return None;
    }

    match std::fs::read_to_string("/usr/share/request-eagle/package") {
        Ok(marker) => match marker.trim() {
            "debian" => Some(LinuxPackage::Debian),
            "rpm" => Some(LinuxPackage::Rpm),
            _ => Some(LinuxPackage::Managed),
        },
        Err(_) => Some(LinuxPackage::Debian),
    }
}
