use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;

/// Saved-data operations. Paths and request IDs are returned by list/get commands.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "command", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "collections.list")]
    CollectionsList {},
    #[serde(rename = "collections.get")]
    CollectionsGet { path: PathBuf },
    /// Create with the default name; entries.rename can change it.
    #[serde(rename = "collections.create")]
    CollectionsCreate {},
    #[serde(rename = "folders.create")]
    FoldersCreate { parent: PathBuf },
    #[serde(rename = "requests.list")]
    RequestsList {
        collection: Option<PathBuf>,
        #[serde(default)]
        query: String,
    },
    #[serde(rename = "requests.get")]
    RequestsGet { path: PathBuf },
    #[serde(rename = "requests.create")]
    RequestsCreate {
        parent: PathBuf,
        name: String,
        request: RequestInput,
    },
    /// Replace a saved request. Read requests.get first to obtain its ID and contents.
    #[serde(rename = "requests.update")]
    RequestsUpdate {
        path: PathBuf,
        expected_id: String,
        request: RequestInput,
    },
    #[serde(rename = "entries.rename")]
    EntriesRename { path: PathBuf, name: String },
    #[serde(rename = "entries.move")]
    EntriesMove {
        path: PathBuf,
        target: PathBuf,
        placement: Placement,
    },
    /// Permanently remove a request, folder or collection.
    #[serde(rename = "entries.delete")]
    EntriesDelete { path: PathBuf, confirm: bool },
    /// Execute a saved request and return the completed response. Script changes
    /// to environment variables last for this invocation and are never persisted.
    #[serde(rename = "requests.run")]
    RequestsRun {
        path: PathBuf,
        #[serde(default)]
        trust_scripts: bool,
        #[serde(default)]
        variables: HashMap<String, String>,
        timeout_ms: Option<u64>,
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
    /// Patch proxy settings. Supply username and password together to replace
    /// credentials. Changing the endpoint without them disables authentication.
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
}

/// Complete saved request. Body accepts UTF-8 text or an array of bytes.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Body {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Before,
    After,
    Inside,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HttpVersion {
    Auto,
    Http1_1,
    Http2,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    System,
    Custom,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProxyProtocol {
    Http,
    Https,
}

pub fn schema() -> Value {
    json!({
        "format_version": FORMAT_VERSION,
        "commands": schemars::schema_for!(Command),
        "output": {"version": FORMAT_VERSION, "ok": true, "result": "command-specific JSON"},
        "error": {"version": FORMAT_VERSION, "ok": false, "error": {"code": "stable_code", "message": "details"}},
        "limits": {"input_bytes": MAX_INPUT_BYTES},
        "workflow": ["collections.list", "requests.list", "requests.get", "requests.update", "requests.run"]
    })
}
