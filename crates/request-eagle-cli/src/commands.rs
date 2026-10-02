use request::Field;
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
        request: SavedRequest,
    },
    /// Replace a saved request. Read requests.get first to obtain its ID and contents.
    #[serde(rename = "requests.update")]
    RequestsUpdate {
        path: PathBuf,
        expected_id: String,
        request: SavedRequest,
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
    /// A gRPC request sends its saved message once, including on client streams,
    /// and returns every response message with the final status. Requests store
    /// and send cookies in the app's cookie jar unless cookie_jar is off.
    /// timeout_ms replaces the request's and the setting's timeout for this run.
    #[serde(rename = "requests.run")]
    RequestsRun {
        path: PathBuf,
        #[serde(default)]
        trust_scripts: bool,
        #[serde(default)]
        variables: HashMap<String, String>,
        timeout_ms: Option<u64>,
    },
    /// Saved flows, which connect blocks such as HTTP requests, FQL and loops
    /// like Postman Flows. Lists the flows of every collection, or of one.
    #[serde(rename = "flows.list")]
    FlowsList {
        collection: Option<PathBuf>,
        #[serde(default)]
        query: String,
    },
    /// A flow's blocks and connections, with the name, URL and variables of
    /// each request its HTTP Request blocks send.
    #[serde(rename = "flows.get")]
    FlowsGet { path: PathBuf },
    /// Create a flow in a collection or folder. Without `flow` it holds a
    /// Start block.
    #[serde(rename = "flows.create")]
    FlowsCreate {
        parent: PathBuf,
        name: String,
        flow: Option<flow::Flow>,
    },
    /// Replace a saved flow's blocks and connections. Read flows.get first to
    /// obtain its ID and contents.
    #[serde(rename = "flows.update")]
    FlowsUpdate {
        path: PathBuf,
        expected_id: String,
        flow: flow::Flow,
    },
    /// Every block type with its settings, inputs, outputs and defaults.
    #[serde(rename = "flows.blocks")]
    FlowsBlocks {},
    /// Run a saved flow and return what its Output blocks received, each
    /// block's last run and the Log blocks' values. `input` is what Start
    /// blocks send; `variables` override the collection's for its requests,
    /// like an active environment. Requests use the app's cookie jar unless
    /// cookie_jar is off. The run stops after timeout_ms [default: 300000].
    #[serde(rename = "flows.run")]
    FlowsRun {
        path: PathBuf,
        input: Option<Value>,
        #[serde(default)]
        trust_scripts: bool,
        #[serde(default)]
        variables: HashMap<String, String>,
        timeout_ms: Option<u64>,
    },
    /// Evaluate an FQL (JSONata) expression, as Evaluate blocks do. The
    /// fields of `input` are the expression's variables; `bindings` are
    /// available as `$name`. An undefined result has `"defined": false`.
    #[serde(rename = "fql.evaluate")]
    FqlEvaluate {
        expression: String,
        input: Option<Value>,
        #[serde(default)]
        bindings: HashMap<String, Value>,
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
        /// Keep the cookies that responses set and send them with later requests.
        cookie_jar: Option<bool>,
        /// A PEM file of certificate authorities to trust in addition to the
        /// system's. An empty path stops trusting them.
        ca_certificates: Option<PathBuf>,
    },
    /// The cookies in the app's cookie jar, optionally of one domain. Each has
    /// its domain, path, name, value, attributes and expiry in Unix seconds,
    /// or null for a session cookie.
    #[serde(rename = "cookies.list")]
    CookiesList { domain: Option<String> },
    /// Delete a domain's cookies, or only the one with this name.
    #[serde(rename = "cookies.delete")]
    CookiesDelete {
        domain: String,
        name: Option<String>,
    },
    /// Present a certificate to the servers of a host that ask for one (mutual
    /// TLS). Supply PEM files, or a PKCS #12 file. The passphrase is kept in the
    /// OS credential store. The result lists the certificate with its ID.
    #[serde(rename = "settings.client_certificates.add")]
    SettingsClientCertificatesAdd {
        /// `api.example.com`, optionally with a port. `*.example.com` matches
        /// its subdomains. Without a port, any port matches.
        host: String,
        /// A PEM certificate, followed by any intermediates. It may hold the key.
        certificate: Option<PathBuf>,
        /// A PEM private key, if it is not in the certificate file.
        key: Option<PathBuf>,
        /// A PKCS #12 file (.p12 or .pfx), instead of PEM files.
        pkcs12: Option<PathBuf>,
        /// Decrypts an encrypted key or the PKCS #12 file.
        passphrase: Option<String>,
    },
    #[serde(rename = "settings.client_certificates.remove")]
    SettingsClientCertificatesRemove { id: String },
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

/// A complete HTTP or gRPC request.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SavedRequest {
    Http(RequestInput),
    Grpc(GrpcRequestInput),
}

/// Complete saved gRPC request. The method is `package.Service/Method`.
/// Without `proto_file`, services are loaded with server reflection.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrpcRequestInput {
    pub protocol: GrpcProtocol,
    /// `host:port`; a `grpcs://` scheme selects TLS and `grpc://` plaintext.
    pub url: String,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub method: String,
    /// JSON message sent when the method is invoked.
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    #[schemars(with = "Vec<FieldSchema>")]
    pub metadata: Vec<Field>,
    /// A `.proto` file; relative paths resolve from the collection directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proto_file: Option<PathBuf>,
    /// Directories that the `.proto` file's imports resolve from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub import_paths: Vec<PathBuf>,
    /// Unset follows the ssl_certificate_verification setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_certificates: Option<bool>,
    /// The certificate name to expect instead of the URL's host.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub server_name: String,
    /// Include response fields with default values [default: true].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_default_fields: Option<bool>,
    /// MiB, or 0 for any size. Unset follows max_response_size_mb.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_response_message_mb: Option<u64>,
    /// Milliseconds for unary calls and server reflection, or 0 for no
    /// deadline. Unset follows the timeout_ms setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// JavaScript run before the method is invoked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before_invoke: String,
    /// JavaScript run for each message the server sends.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on_message: String,
    /// JavaScript run after the server ends the call.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after_response: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrpcProtocol {
    Grpc,
}

/// Complete saved HTTP request.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestInput {
    pub method: Method,
    pub url: String,
    #[serde(default)]
    #[schemars(with = "Vec<FieldSchema>")]
    pub headers: Vec<Field>,
    #[serde(default)]
    #[schemars(with = "Vec<FieldSchema>")]
    pub query: Vec<Field>,
    /// Values for `:name` segments of the URL's path, such as `id` in
    /// `/pets/:id`. A variable without a value is sent as written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_variables: Vec<(String, String)>,
    pub body: Option<Body>,
    #[serde(default)]
    pub pre_request: String,
    #[serde(default)]
    pub post_response: String,
    /// Milliseconds, or 0 for no deadline. Unset follows the timeout_ms setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Unset follows the follow_all_redirects setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    /// Unset follows the ssl_certificate_verification setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_certificates: Option<bool>,
}

/// A header, query parameter or metadata row as `[key, value]`, or as an
/// object to switch it off or describe it. Rows that are off are not sent.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum FieldSchema {
    Pair(String, String),
    Row {
        key: String,
        value: String,
        #[serde(default)]
        disabled: bool,
        #[serde(default)]
        description: String,
    },
}

/// A request body. Text alone is a raw JSON body.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Body {
    Text(String),
    Typed(TypedBody),
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TypedBody {
    /// Text sent as written. The language sets the default Content-Type.
    Raw { language: Language, text: String },
    /// An application/x-www-form-urlencoded form. Sending encodes each name
    /// and value after filling in its variables.
    UrlEncoded { fields: Vec<(String, String)> },
    /// A multipart/form-data form.
    Multipart { parts: Vec<FormPart> },
    /// The contents of a file, read when the request runs. A relative path
    /// starts at the collection's directory.
    Binary { file: PathBuf },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Json,
    Xml,
    Text,
}

/// A text field, or with `file: true` a file whose path is `value`. A
/// relative path starts at the collection's directory.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormPart {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file: bool,
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
        "workflow": ["collections.list", "requests.list", "requests.get", "requests.update", "requests.run"],
        "flows": ["flows.blocks", "flows.list", "flows.get", "flows.update", "flows.run", "fql.evaluate"]
    })
}
