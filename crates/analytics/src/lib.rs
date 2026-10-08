//! Anonymous usage data, sent to PostHog to learn which features people use.

use std::{fs, io, path::Path};

use gpui_kit::{App, Global};
use preferences::Preferences;
use serde_json::{Value, json};
use uuid::Uuid;

/// The PostHog project token. Release builds embed it, so builds from source
/// send nothing.
const TOKEN: Option<&str> = option_env!("REQUEST_EAGLE_POSTHOG_TOKEN");

const ENDPOINT: &str = "https://eu.i.posthog.com/i/v0/e/";

struct Analytics {
    token: &'static str,
    /// A random ID that tells this installation's events apart. Nothing
    /// links it to the person using it.
    distinct_id: String,
    app_version: &'static str,
}

impl Global for Analytics {}

/// Send events from a release build. `directory` keeps the installation ID.
pub fn init(directory: &Path, app_version: &'static str, cx: &mut App) {
    let Some(token) = TOKEN else {
        return;
    };

    match installation_id(&directory.join("analytics-id")) {
        Ok(distinct_id) => cx.set_global(Analytics {
            token,
            distinct_id,
            app_version,
        }),
        Err(error) => eprintln!("Usage data is not sent: {error}"),
    }
}

/// Send an event with `properties`, a JSON object. Nothing is sent while
/// usage data is turned off in Settings, or when the preferences could not be
/// read, since they may turn it off.
pub fn capture(event: &str, properties: Value, cx: &App) {
    let Some(analytics) = cx.try_global::<Analytics>() else {
        return;
    };

    let allowed = preferences::load_error(cx).is_none()
        && cx
            .try_global::<Preferences>()
            .is_some_and(|preferences| preferences.share_usage_data);
    if !allowed {
        return;
    }

    let body = payload(analytics, event, properties).to_string();
    let response = cx.http_client().post_json(ENDPOINT, body.into());

    cx.background_executor()
        .spawn(async move {
            // Being offline is expected. A rejected event points at the build.
            if let Ok(response) = response.await
                && !response.status().is_success()
            {
                log::warn!("PostHog rejected a usage event: {}", response.status());
            }
        })
        .detach();
}

/// The ID kept in `path`, or a new one saved there.
fn installation_id(path: &Path) -> io::Result<String> {
    if let Ok(saved) = fs::read_to_string(path)
        && let Ok(id) = Uuid::parse_str(saved.trim())
    {
        return Ok(id.to_string());
    }

    let id = Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &id)?;

    Ok(id)
}

/// The event as PostHog's capture endpoint takes it.
fn payload(analytics: &Analytics, event: &str, mut properties: Value) -> Value {
    if let Value::Object(properties) = &mut properties {
        properties.extend([
            ("$lib".into(), "request-eagle".into()),
            ("$app_version".into(), analytics.app_version.into()),
            ("$os".into(), os().into()),
            ("arch".into(), std::env::consts::ARCH.into()),
            // Events stay anonymous, without a person profile.
            ("$process_person_profile".into(), false.into()),
        ]);
    }

    json!({
        "api_key": analytics.token,
        "event": event,
        "distinct_id": analytics.distinct_id,
        "properties": properties,
    })
}

/// The operating system as PostHog names it.
fn os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use preferences::Preferences;
    use serde_json::json;

    use super::{Analytics, installation_id, payload};

    #[test]
    fn usage_data_is_shared_until_turned_off() {
        assert!(Preferences::default().share_usage_data);
        assert!(
            serde_json::from_str::<Preferences>(r#"{"vim_mode": true}"#)
                .unwrap()
                .share_usage_data
        );
        assert!(
            !serde_json::from_str::<Preferences>(r#"{"share_usage_data": false}"#)
                .unwrap()
                .share_usage_data
        );
    }

    #[test]
    fn an_installation_keeps_its_id() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing/analytics-id");

        let id = installation_id(&path).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), id);
        assert_eq!(installation_id(&path).unwrap(), id);
    }

    #[test]
    fn an_unreadable_id_is_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("analytics-id");
        fs::write(&path, "not an id").unwrap();

        let id = installation_id(&path).unwrap();

        assert_ne!(id, "not an id");
        assert_eq!(fs::read_to_string(&path).unwrap(), id);
    }

    #[test]
    fn events_carry_the_app_and_stay_anonymous() {
        let analytics = Analytics {
            token: "phc_test",
            distinct_id: "installation".into(),
            app_version: "1.2.3",
        };

        let event = payload(&analytics, "request_sent", json!({ "protocol": "http" }));

        assert_eq!(event["api_key"], "phc_test");
        assert_eq!(event["event"], "request_sent");
        assert_eq!(event["distinct_id"], "installation");

        let properties = &event["properties"];
        assert_eq!(properties["protocol"], "http");
        assert_eq!(properties["$lib"], "request-eagle");
        assert_eq!(properties["$app_version"], "1.2.3");
        assert_eq!(properties["arch"], std::env::consts::ARCH);
        assert_eq!(properties["$process_person_profile"], false);
        assert!(properties["$os"].is_string());
    }
}
