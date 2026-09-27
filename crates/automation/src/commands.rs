use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_BODY_CHUNK: usize = 256 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub version: u32,
    pub command: Command,
}

/// Commands act on the running application. Paths come from collections.list;
/// tab IDs come from tabs.list and remain stable until the tab closes.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "command", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "app.status")]
    AppStatus {},
    #[serde(rename = "collections.list")]
    CollectionsList {
        #[serde(default)]
        query: String,
    },
    /// Create a collection with the application's default name; use entries.rename to name it.
    #[serde(rename = "collections.create")]
    CollectionsCreate {},
    #[serde(rename = "folders.create")]
    FoldersCreate { parent: PathBuf },
    #[serde(rename = "requests.create")]
    RequestsCreate {
        parent: PathBuf,
        name: String,
        request: RequestInput,
    },
    #[serde(rename = "requests.get")]
    RequestsGet { path: PathBuf },
    /// Open a saved request. Reopening preserves an existing unsaved draft.
    #[serde(rename = "requests.open")]
    RequestsOpen { path: PathBuf },
    #[serde(rename = "entries.rename")]
    EntriesRename { path: PathBuf, name: String },
    #[serde(rename = "entries.move")]
    EntriesMove {
        path: PathBuf,
        target: PathBuf,
        placement: Placement,
    },
    /// Permanently remove a saved entry. Open drafts are retained.
    #[serde(rename = "entries.delete")]
    EntriesDelete { path: PathBuf, confirm: bool },
    #[serde(rename = "tabs.list")]
    TabsList {},
    #[serde(rename = "tabs.new")]
    TabsNew {},
    #[serde(rename = "tabs.select")]
    TabsSelect { tab: u64 },
    #[serde(rename = "tabs.close")]
    TabsClose {
        tab: u64,
        #[serde(default)]
        discard: bool,
    },
    #[serde(rename = "drafts.get")]
    DraftsGet { tab: u64 },
    /// Replace the complete editable request without saving it. Read drafts.get first.
    #[serde(rename = "drafts.set")]
    DraftsSet { tab: u64, request: RequestInput },
    /// Save an existing request, or supply parent and name to save a new draft / Save As.
    #[serde(rename = "drafts.save")]
    DraftsSave {
        tab: u64,
        parent: Option<PathBuf>,
        name: Option<String>,
    },
    /// Starts execution and returns immediately. Poll responses.get until loading=false.
    /// trust_scripts explicitly approves the current scripts in this tab.
    #[serde(rename = "requests.send")]
    RequestsSend {
        tab: u64,
        #[serde(default)]
        trust_scripts: bool,
    },
    #[serde(rename = "requests.cancel")]
    RequestsCancel { tab: u64 },
    /// Body is base64 encoded, paginated by byte offset. Includes headers, cookies,
    /// timing, size, tests and console output. HTTP error statuses are responses.
    #[serde(rename = "responses.get")]
    ResponsesGet {
        tab: u64,
        #[serde(default)]
        offset: usize,
        #[serde(default = "body_limit")]
        limit: usize,
    },
    #[serde(rename = "settings.get")]
    SettingsGet {},
    #[serde(rename = "settings.request")]
    SettingsRequest {
        http_version: Option<HttpVersion>,
        timeout_ms: Option<u64>,
        max_response_size_mb: Option<u64>,
        ssl_certificate_verification: Option<bool>,
        follow_all_redirects: Option<bool>,
    },
    #[serde(rename = "settings.appearance")]
    SettingsAppearance {
        mode: Option<AppearanceMode>,
        light_theme: Option<String>,
        dark_theme: Option<String>,
        editor_font: Option<String>,
        interface_font_size: Option<f32>,
    },
    /// Patch proxy settings. Omitted credentials are preserved only for the same endpoint.
    /// Changing host, port or protocol clears credentials and disables authentication.
    /// To authenticate a new endpoint, explicitly supply username, password and authentication=true.
    /// Credentials are stored through the application's OS credential store.
    #[serde(rename = "settings.proxy")]
    SettingsProxy {
        mode: Option<ProxyMode>,
        protocol: Option<ProxyProtocol>,
        host: Option<String>,
        port: Option<u16>,
        http: Option<bool>,
        https: Option<bool>,
        authentication: Option<bool>,
        username: Option<String>,
        password: Option<String>,
        bypass: Option<String>,
    },
    #[serde(rename = "themes.list")]
    ThemesList {},
    #[serde(rename = "fonts.list")]
    FontsList {},
    #[serde(rename = "keybindings.list")]
    KeybindingsList {},
    /// null disables the binding. Use keybindings.reset to restore defaults.
    #[serde(rename = "keybindings.set")]
    KeybindingsSet {
        id: String,
        keystrokes: Option<String>,
    },
    /// Omit id to reset all bindings.
    #[serde(rename = "keybindings.reset")]
    KeybindingsReset { id: Option<String> },
    /// Show a settings page, or workspace to close settings.
    #[serde(rename = "ui.show")]
    UiShow { page: Page },
    #[serde(rename = "ui.sidebar")]
    UiSidebar { visible: bool },
    #[serde(rename = "updates.check")]
    UpdatesCheck {},
    #[serde(rename = "updates.download")]
    UpdatesDownload {},
    #[serde(rename = "updates.status")]
    UpdatesStatus {},
    /// Quit and relaunch into an already downloaded, verified update.
    #[serde(rename = "updates.install")]
    UpdatesInstall { confirm: bool },
}

fn body_limit() -> usize {
    64 * 1024
}

/// Complete request draft. Body accepts UTF-8 text or an array of bytes.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestInput {
    pub method: Method,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub query: Vec<(String, String)>,
    pub body: Option<Body>,
    #[serde(default)]
    pub pre_request: String,
    #[serde(default)]
    pub post_response: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Body {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Head,
    Options,
    Delete,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Before,
    After,
    Inside,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    Auto,
    Http1_1,
    Http2,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    System,
    Custom,
    Disabled,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProxyProtocol {
    Http,
    Https,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    Workspace,
    General,
    Appearance,
    Proxy,
    Keybindings,
}

pub fn schema() -> Value {
    json!({"protocol_version": PROTOCOL_VERSION, "commands": schemars::schema_for!(Command),
        "output": {"version": PROTOCOL_VERSION, "ok": true, "result": "command-specific JSON"},
        "error": {"version": PROTOCOL_VERSION, "ok": false, "error": {"code": "stable_code", "message": "details"}},
        "limits": {"message_bytes": MAX_MESSAGE_BYTES, "body_chunk_bytes": MAX_BODY_CHUNK},
        "workflow": ["collections.list", "requests.open", "drafts.get", "drafts.set", "drafts.save", "requests.send", "responses.get"]})
}

pub fn success(result: Value) -> Value {
    json!({"version": PROTOCOL_VERSION, "ok": true, "result": result})
}

pub fn failure(code: &str, message: impl std::fmt::Display) -> Value {
    json!({"version": PROTOCOL_VERSION, "ok": false, "error": {"code": code, "message": message.to_string()}})
}
