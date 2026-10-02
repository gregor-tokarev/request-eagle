//! How each platform's release package is found and recognized.

/// The release file that describes this platform's package. macOS keeps the
/// name that versions before update tracks read.
#[cfg(target_os = "macos")]
pub(super) const MANIFEST: &str = "request-eagle-update.json";
#[cfg(windows)]
pub(super) const MANIFEST: &str = "request-eagle-update-windows.json";
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) const MANIFEST: &str = "request-eagle-update-debian.json";

/// Only the macOS app replaces itself. The Windows installer and the Debian
/// package are downloaded in the browser and installed by the system.
pub const INSTALLS_IN_APP: bool = cfg!(target_os = "macos");

/// Whether this copy was installed from a release package rather than built
/// from source, so it looks for updates on its own.
pub(super) fn installed() -> bool {
    #[cfg(target_os = "macos")]
    return super::install::current_app_bundle().is_ok();

    // The installer leaves its uninstaller beside the app.
    #[cfg(windows)]
    return std::env::current_exe()
        .is_ok_and(|executable| executable.with_file_name("unins000.exe").is_file());

    // The Debian package installs the app here.
    #[cfg(not(any(target_os = "macos", windows)))]
    return std::env::current_exe()
        .is_ok_and(|executable| executable == std::path::Path::new("/usr/bin/request-eagle"));
}
