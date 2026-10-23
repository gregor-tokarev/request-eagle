//! How each platform's release package is found and recognized.

/// Only the macOS app replaces itself. The Windows installer and the Linux
/// packages are downloaded in the browser and installed by the system.
pub const INSTALLS_IN_APP: bool = cfg!(target_os = "macos");

/// The macOS installer keeps the previous version until the new one confirms
/// that it started. Elsewhere the system installer has already finished.
#[cfg(not(target_os = "macos"))]
pub fn confirm_startup() {}

/// The release file that describes this copy's package. macOS keeps the name
/// that versions before update tracks read.
#[cfg(target_os = "macos")]
pub(super) fn manifest() -> &'static str {
    "request-eagle-update.json"
}

#[cfg(windows)]
pub(super) fn manifest() -> &'static str {
    "request-eagle-update-windows.json"
}

/// Linux has several packages. A copy that no release package replaces,
/// because it was built from source, is offered the Debian package when
/// asked, as before the other packages existed.
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn manifest() -> &'static str {
    match linux_package() {
        Some(LinuxPackage::AppImage) => "request-eagle-update-appimage.json",
        Some(LinuxPackage::Rpm) => "request-eagle-update-rpm.json",
        Some(LinuxPackage::Debian) | None => "request-eagle-update-debian.json",
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
    linux_package().is_some()
}

#[cfg(not(any(target_os = "macos", windows)))]
#[derive(Clone, Copy)]
enum LinuxPackage {
    AppImage,
    Debian,
    Rpm,
}

/// The AppImage runtime tells the app where its image is. The Debian and RPM
/// packages install the app at one path and name themselves in a marker
/// file, which older Debian packages did not leave. A package manager that
/// delivers updates itself, such as pacman for the AUR package, names itself
/// there too, and the app leaves updates to it.
#[cfg(not(any(target_os = "macos", windows)))]
fn linux_package() -> Option<LinuxPackage> {
    if std::env::var_os("APPIMAGE").is_some() {
        return Some(LinuxPackage::AppImage);
    }

    let packaged = std::env::current_exe()
        .is_ok_and(|executable| executable == std::path::Path::new("/usr/bin/request-eagle"));

    if !packaged {
        return None;
    }

    match std::fs::read_to_string("/usr/share/request-eagle/package") {
        Ok(marker) => match marker.trim() {
            "debian" => Some(LinuxPackage::Debian),
            "rpm" => Some(LinuxPackage::Rpm),
            _ => None,
        },
        Err(_) => Some(LinuxPackage::Debian),
    }
}
