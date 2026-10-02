use std::{fs, io, io::Write as _, path::PathBuf};

use gpui_kit::{App, Bounds, DisplayId, Pixels, Window, WindowBounds, point};
use serde::{Deserialize, Serialize};

/// The window, sidebar and tabs as the app last left them, so the next launch
/// opens the same way.
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Session {
    /// The file the session is read from and saved to.
    #[serde(skip)]
    pub(crate) path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) window: Option<SavedWindow>,
    pub(crate) sidebar: SavedSidebar,
    pub(crate) tabs: Vec<SavedTab>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) selected_tab: Option<usize>,
}

impl Session {
    /// A missing or unreadable file gives an empty session, which opens the
    /// app as on its first launch.
    pub fn load(path: PathBuf) -> Self {
        let session = fs::read(&path)
            .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?))
            .unwrap_or_else(|error| {
                if error.kind() != io::ErrorKind::NotFound {
                    eprintln!("Could not read {}: {error}", path.display());
                }
                Self::default()
            });

        Self { path, ..session }
    }

    /// Replace the file whole, so a save that fails midway leaves the
    /// previous session.
    pub(crate) fn save(&self) -> io::Result<()> {
        let directory = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("The session file has no directory"))?;
        fs::create_dir_all(directory)?;

        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.persist(&self.path)?;

        Ok(())
    }

    /// The display the window was on and its bounds there, while that display
    /// is connected.
    pub fn window_placement(&self, cx: &App) -> Option<(DisplayId, WindowBounds)> {
        let window = self.window.as_ref()?;
        let display = cx.displays().into_iter().find(|display| {
            display
                .uuid()
                .is_ok_and(|uuid| uuid.to_string() == window.display)
        })?;

        let bounds = match window.bounds {
            SavedBounds::Windowed(bounds) => {
                WindowBounds::Windowed(fit_bounds(bounds, display.bounds()))
            }
            // As the first window: maximizing a window that already fills the
            // display would restore it to a smaller size on macOS.
            SavedBounds::Maximized => WindowBounds::Maximized(display.default_bounds()),
            SavedBounds::Fullscreen(bounds) => {
                WindowBounds::Fullscreen(fit_bounds(bounds, display.bounds()))
            }
        };

        Some((display.id(), bounds))
    }
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SavedWindow {
    /// The display's identifier, which stays the same across restarts.
    display: String,
    bounds: SavedBounds,
}

impl SavedWindow {
    /// None when the window's display has no lasting identifier.
    pub(crate) fn capture(window: &Window, cx: &App) -> Option<Self> {
        let display = window.display(cx)?.uuid().ok()?.to_string();

        // Some platforms leave the full-screen or maximized state out of the
        // window's bounds.
        let bounds = window.window_bounds().get_bounds();
        let bounds = if window.is_fullscreen() {
            SavedBounds::Fullscreen(bounds)
        } else if window.is_maximized() {
            SavedBounds::Maximized
        } else {
            SavedBounds::Windowed(bounds)
        };

        Some(Self { display, bounds })
    }
}

/// Like `WindowBounds`, which cannot be saved itself. A full-screen window
/// keeps the bounds it returns to. Bounds are relative to the display.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum SavedBounds {
    Windowed(Bounds<Pixels>),
    Maximized,
    Fullscreen(Bounds<Pixels>),
}

/// Keep a window on its display after the display became smaller.
pub(crate) fn fit_bounds(bounds: Bounds<Pixels>, display: Bounds<Pixels>) -> Bounds<Pixels> {
    let size = bounds.size.min(&display.size);
    let origin = point(
        bounds
            .origin
            .x
            .clamp(display.left(), display.right() - size.width),
        bounds
            .origin
            .y
            .clamp(display.top(), display.bottom() - size.height),
    );

    Bounds::new(origin, size)
}

/// Which parts of the sidebar are shown. Everything is, at first.
#[derive(Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct SavedSidebar {
    pub(crate) visible: bool,
    pub(crate) collections: bool,
    pub(crate) environments: bool,
    pub(crate) history: bool,
}

impl Default for SavedSidebar {
    fn default() -> Self {
        Self {
            visible: true,
            collections: true,
            environments: true,
            history: true,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SavedTab {
    /// A saved request is opened from its file again. The tab keeps its
    /// request only while it differs from the file, and always when the
    /// request is not saved.
    Request {
        title: String,
        /// Where the request is saved. None while it is not.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<SavedFile>,
        /// The name given to a request that is not saved yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<request::Request>,
    },
    Collection {
        path: PathBuf,
    },
    Environment {
        name: String,
    },
    Cookies,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SavedFile {
    pub(crate) path: PathBuf,
    /// Tells the request apart from another one saved at the same path later.
    pub(crate) id: String,
    /// Where the request's relative file paths start.
    pub(crate) collection: PathBuf,
}
