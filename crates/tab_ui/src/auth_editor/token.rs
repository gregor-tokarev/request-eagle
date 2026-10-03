use gpui_kit::component::{notification::Notification, *};
use gpui_kit::*;
use request::{Auth, RequestExecutor};

use super::editor::AuthEditor;
use super::fields::Field;

impl AuthEditor {
    /// Ask the authorization server for a new OAuth 2.0 access token, which
    /// replaces the current one. The authorization code grant opens the
    /// browser to sign in first.
    pub(super) fn get_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Auth::OAuth2(auth) = &self.auth else {
            return;
        };

        let preferences = crate::request_settings::preferences(cx);
        let variables = self.scope.read(cx).request_variables(cx);
        let request = RequestExecutor::new(&preferences)
            .map_err(|error| error.to_string())
            .and_then(|executor| executor.oauth2_token(auth, &variables));
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                window.push_notification(
                    Notification::error(error).title("Could not get an access token"),
                    cx,
                );
                return;
            }
        };

        if let Some(url) = &request.browser_url {
            cx.open_url(url);
        }

        let token = cx.background_executor().spawn(request.token);
        self.token_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = token.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.token_task = None;

                match result {
                    Ok(token) => {
                        this.set_field(Field::AccessToken, token.access_token, window, cx);
                        let expiry = token
                            .expires_in
                            .map(|seconds| format!(" It expires in {}.", duration(seconds)))
                            .unwrap_or_default();
                        window.push_notification(
                            Notification::success(format!(
                                "The new access token replaced the current one.{expiry}"
                            ))
                            .title("Access token received"),
                            cx,
                        );
                    }
                    Err(error) => window.push_notification(
                        Notification::error(error).title("Could not get an access token"),
                        cx,
                    ),
                }

                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Stop waiting for a token, such as for a sign-in that was abandoned.
    pub(super) fn cancel_token(&mut self, cx: &mut Context<Self>) {
        self.token_task = None;
        cx.notify();
    }
}

/// Seconds as the largest whole unit, such as "1 hour".
fn duration(seconds: u64) -> String {
    let (count, unit) = match seconds {
        0..60 => (seconds, "second"),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        _ => (seconds / 86_400, "day"),
    };

    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}
