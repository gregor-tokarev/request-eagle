use std::{
    fmt::Display,
    fs, io,
    path::{Path, PathBuf},
};

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

pub fn file() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join(".request-eagle/logs/request-eagle.log"))
}

/// Apps opened from Finder or a desktop launcher write stderr to /dev/null,
/// and Windows release builds have no stderr at all. Send it to the log file
/// instead, so `eprintln!`, log records and panic messages are kept. A
/// terminal or a redirection chosen by whoever started the app keeps
/// receiving stderr.
pub fn init() {
    if stderr_is_discarded()
        && let Some(path) = file()
        && let Ok(log) = open(&path)
    {
        redirect_stderr(log);
    }

    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }

    let report_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("{} Request Eagle panicked:", timestamp());
        report_panic(info);
    }));

    eprintln!(
        "{} Request Eagle {} started on {} {}",
        timestamp(),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

/// Opens the log for appending. A log over 5 MB replaces the previous
/// `request-eagle.log.1` first, so at most one old log is kept.
pub(crate) fn open(path: &Path) -> io::Result<fs::File> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }

    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_LOG_SIZE) {
        fs::rename(path, path.with_extension("log.1"))?;
    }

    fs::OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(unix)]
fn stderr_is_discarded() -> bool {
    let (Ok(stderr), Ok(null)) = (
        rustix::fs::fstat(io::stderr()),
        rustix::fs::stat("/dev/null"),
    ) else {
        return false;
    };

    (stderr.st_dev, stderr.st_ino) == (null.st_dev, null.st_ino)
}

#[cfg(unix)]
fn redirect_stderr(log: fs::File) {
    // Without stderr there is nowhere to report a failure.
    let _ = rustix::stdio::dup2_stderr(log);
}

/// Release builds open without a console, so they start without stderr.
#[cfg(windows)]
fn stderr_is_discarded() -> bool {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{GetStdHandle, STD_ERROR_HANDLE},
    };

    // SAFETY: GetStdHandle only reads the process's standard handles.
    let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };

    stderr.is_null() || stderr == INVALID_HANDLE_VALUE
}

/// Rust looks up the standard handle on every write, so replacing it is
/// enough.
#[cfg(windows)]
fn redirect_stderr(log: fs::File) {
    use std::os::windows::io::IntoRawHandle as _;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};

    // SAFETY: The handle stays open for the rest of the process.
    unsafe {
        SetStdHandle(STD_ERROR_HANDLE, log.into_raw_handle());
    }
}

fn timestamp() -> impl Display {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z")
}

/// Writes warnings and errors, including GPUI's, to stderr.
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{} {} {}: {}",
                timestamp(),
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}
