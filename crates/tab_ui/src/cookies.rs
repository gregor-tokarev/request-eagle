use gpui_kit::*;
use request::CookieJar;

/// The cookie jar that every request in the workspace shares. Observers of
/// this global learn when requests or the cookies page change it.
pub struct Cookies {
    jar: CookieJar,
    /// The jar's revision when observers were last notified.
    revision: u64,
    /// Why the jar could not be saved the last time it changed.
    error: Option<String>,
}

impl Global for Cookies {}

impl Cookies {
    pub fn init(jar: CookieJar, cx: &mut App) {
        // A change made just before quitting may still be waiting to be
        // written. Saving blocks the quit until it is done.
        let saved = jar.clone();
        cx.on_app_quit(move |_| {
            let result = saved.save();

            async move {
                if let Err(error) = result {
                    eprintln!("Could not save cookies: {error}");
                }
            }
        })
        .detach();

        cx.set_global(Self {
            revision: jar.revision(),
            jar,
            error: None,
        });
    }

    /// The shared jar. Before `init`, requests get an unsaved jar of their own.
    pub(crate) fn jar(cx: &App) -> CookieJar {
        cx.try_global::<Self>()
            .map(|cookies| cookies.jar.clone())
            .unwrap_or_default()
    }

    pub(crate) fn error(cx: &App) -> Option<&str> {
        cx.try_global::<Self>()?.error.as_deref()
    }

    /// Notify observers and save the jar in the background, if it changed
    /// since the last call.
    pub(crate) fn changed(cx: &mut App) {
        let Some(cookies) = cx.try_global::<Self>() else {
            return;
        };

        let revision = cookies.jar.revision();
        if revision == cookies.revision {
            return;
        }

        let jar = cookies.jar.clone();
        cx.update_global::<Self, _>(|cookies, _| cookies.revision = revision);

        let save = cx.background_executor().spawn(async move { jar.save() });
        cx.spawn(async move |cx| {
            let error = save
                .await
                .err()
                .map(|error| format!("Could not save cookies: {error}"));

            cx.update(|cx| {
                if Self::error(cx) != error.as_deref() {
                    cx.update_global::<Self, _>(|cookies, _| cookies.error = error);
                }
            });
        })
        .detach();
    }
}
