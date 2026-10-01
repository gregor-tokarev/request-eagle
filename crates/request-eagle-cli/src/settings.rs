use anyhow::{Result, bail};
use preferences::{Preferences, PreferencesFile};
use serde_json::{Value, json};

use crate::commands::{AppearanceMode, Command, HttpVersion, ProxyMode, ProxyProtocol};

pub async fn dispatch(file: &PreferencesFile, command: Command) -> Result<Value> {
    let preferences = match command {
        Command::SettingsGet {} => file.read()?,
        Command::SettingsRequest {
            http_version,
            timeout_ms,
            max_response_size_mb,
            ssl_certificate_verification,
            follow_all_redirects,
            cookie_jar,
        } => file.update(|preferences| {
            let request = &mut preferences.request;
            if let Some(version) = http_version {
                request.http_version = match version {
                    HttpVersion::Auto => preferences::HttpVersion::Auto,
                    HttpVersion::Http1_1 => preferences::HttpVersion::Http1_1,
                    HttpVersion::Http2 => preferences::HttpVersion::Http2,
                };
            }
            if let Some(value) = timeout_ms {
                request.timeout_ms = value;
            }
            if let Some(value) = max_response_size_mb {
                request.max_response_size_mb = value;
            }
            if let Some(value) = ssl_certificate_verification {
                request.ssl_certificate_verification = value;
            }
            if let Some(value) = follow_all_redirects {
                request.follow_all_redirects = value;
            }
            if let Some(value) = cookie_jar {
                request.cookie_jar = value;
            }
            Ok(())
        })?,
        Command::SettingsAppearance {
            mode,
            light_theme,
            dark_theme,
            editor_font,
            interface_font_size,
        } => file.update(|preferences| {
            let appearance = &mut preferences.appearance;
            if let Some(mode) = mode {
                appearance.mode = match mode {
                    AppearanceMode::System => preferences::AppearanceMode::System,
                    AppearanceMode::Light => preferences::AppearanceMode::Light,
                    AppearanceMode::Dark => preferences::AppearanceMode::Dark,
                };
            }
            if let Some(value) = light_theme {
                appearance.light_theme = value;
            }
            if let Some(value) = dark_theme {
                appearance.dark_theme = value;
            }
            if let Some(value) = editor_font {
                appearance.editor_font = value.trim().into();
            }
            if let Some(value) = interface_font_size {
                if !(12. ..=24.).contains(&value) {
                    bail!("Interface font size must be between 12 and 24");
                }
                appearance.interface_font_size = value;
            }
            Ok(())
        })?,
        Command::SettingsProxy {
            mode,
            protocol,
            host,
            port,
            http,
            https,
            authentication,
            username,
            password,
            bypass,
        } => {
            let credentials = match (username, password) {
                (Some(username), Some(password)) => Some((username, password)),
                (None, None) => None,
                _ => bail!("Supply username and password together to replace proxy credentials"),
            };
            file.update_proxy(
                |proxy| {
                    if let Some(mode) = mode {
                        proxy.mode = match mode {
                            ProxyMode::System => preferences::ProxyMode::System,
                            ProxyMode::Custom => preferences::ProxyMode::Custom,
                            ProxyMode::Disabled => preferences::ProxyMode::Disabled,
                        };
                    }
                    if let Some(protocol) = protocol {
                        proxy.protocol = match protocol {
                            ProxyProtocol::Http => preferences::ProxyProtocol::Http,
                            ProxyProtocol::Https => preferences::ProxyProtocol::Https,
                        };
                    }
                    if let Some(value) = host {
                        proxy.host = value.trim().into();
                    }
                    if let Some(value) = port {
                        proxy.port = value;
                    }
                    if let Some(value) = http {
                        proxy.http = value;
                    }
                    if let Some(value) = https {
                        proxy.https = value;
                    }
                    if let Some(value) = authentication {
                        proxy.authentication = value;
                    }
                    if let Some(value) = bypass {
                        proxy.bypass = value;
                    }
                    Ok(())
                },
                credentials,
            )
            .await?
        }
        _ => bail!("Expected a settings operation"),
    };

    Ok(snapshot(&preferences))
}

fn snapshot(preferences: &Preferences) -> Value {
    // Preferences serialization already excludes proxy secrets. Also omit the
    // internal credential reference from the CLI's public settings response.
    json!({"appearance": preferences.appearance, "request": preferences.request})
}
