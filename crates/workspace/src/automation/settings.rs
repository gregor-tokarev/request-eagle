use gpui_kit::App;
use preferences::Preferences;
use request_eagle_automation::Command;
use serde_json::{Value, json};

pub(super) fn proxy(
    command: Command,
    cx: &mut App,
) -> Result<gpui_kit::Task<anyhow::Result<()>>, String> {
    let Command::SettingsProxy {
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
    } = command
    else {
        unreachable!()
    };
    let mut settings = cx.global::<Preferences>().request.proxy.clone();
    if let Some(value) = mode {
        settings.mode = serde_json::from_value(json!(value)).unwrap();
    }
    if let Some(value) = protocol {
        settings.protocol = serde_json::from_value(json!(value)).unwrap();
    }
    if let Some(value) = host {
        settings.host = value;
    }
    if let Some(value) = port {
        settings.port = value;
    }
    if let Some(value) = http {
        settings.http = value;
    }
    if let Some(value) = https {
        settings.https = value;
    }
    if let Some(value) = authentication {
        settings.authentication = value;
    }
    if let Some(value) = username {
        settings.username = value;
    }
    if let Some(value) = password {
        settings.password = value;
    }
    if let Some(value) = bypass {
        settings.bypass = value;
    }
    settings.validate().map_err(str::to_owned)?;
    Ok(preferences::update_proxy(settings, cx))
}

pub(super) fn apply(command: Command, cx: &mut App) -> Result<Value, String> {
    match command {
        Command::SettingsGet {} => return Ok(json!({"appearance": cx.global::<Preferences>().appearance, "request": cx.global::<Preferences>().request, "credential_error": preferences::credential_error(cx)})),
        Command::SettingsRequest { http_version, timeout_ms, max_response_size_mb, ssl_certificate_verification, follow_all_redirects } => preferences::update(cx, |settings| {
            if let Some(value) = http_version { settings.request.http_version = serde_json::from_value(json!(value)).unwrap(); }
            if let Some(value) = timeout_ms { settings.request.timeout_ms = value; }
            if let Some(value) = max_response_size_mb { settings.request.max_response_size_mb = value; }
            if let Some(value) = ssl_certificate_verification { settings.request.ssl_certificate_verification = value; }
            if let Some(value) = follow_all_redirects { settings.request.follow_all_redirects = value; }
        }).map_err(|e| e.to_string())?,
        Command::SettingsAppearance { mode, light_theme, dark_theme, editor_font, interface_font_size } => {
            if interface_font_size.is_some_and(|size| !size.is_finite() || !(12. ..=24.).contains(&size)) { return Err("Interface font size must be between 12 and 24".into()); }
            for (name, dark) in [(&light_theme, false), (&dark_theme, true)] {
                if let Some(name) = name && !request_eagle_theme::config(name, cx).is_some_and(|config| config.mode.is_dark() == dark) { return Err(format!("Unknown or incompatible theme: {name}")); }
            }
            if let Some(font) = &editor_font && !font.is_empty() && !cx.text_system().all_font_names().contains(font) { return Err("Unknown font; use fonts.list".into()); }
            preferences::update(cx, |settings| {
                if let Some(value) = mode { settings.appearance.mode = serde_json::from_value(json!(value)).unwrap(); }
                if let Some(value) = light_theme { settings.appearance.light_theme = value; }
                if let Some(value) = dark_theme { settings.appearance.dark_theme = value; }
                if let Some(value) = editor_font { settings.appearance.editor_font = value; }
                if let Some(value) = interface_font_size { settings.appearance.interface_font_size = value; }
            }).map_err(|e| e.to_string())?;
        }
        Command::ThemesList {} => return Ok(json!(request_eagle_theme::themes(cx).iter().map(|theme| json!({"name": theme.name, "mode": if theme.mode.is_dark() { "dark" } else { "light" }})).collect::<Vec<_>>())),
        Command::FontsList {} => return Ok(json!(cx.text_system().all_font_names())),
        Command::KeybindingsList {} => return Ok(json!(keybindings_service::commands(cx).into_iter().map(|command| json!({"id": command.id, "label": command.label, "description": command.description, "category": command.category, "keystrokes": command.binding.map(|binding| binding.keystrokes), "default": command.default_binding.map(|binding| binding.keystrokes), "error": command.binding_error})).collect::<Vec<_>>())),
        Command::KeybindingsSet { id, keystrokes } => keybindings_service::set_override(&id, keystrokes.as_deref(), cx).map_err(|e| e.to_string())?,
        Command::KeybindingsReset { id } => match id {
            Some(id) => keybindings_service::reset_command(&id, cx),
            None => keybindings_service::reset_all(cx),
        }.map_err(|e| e.to_string())?,
        _ => return Err("Unsupported settings command".into()),
    }
    Ok(json!({"saved": true}))
}
