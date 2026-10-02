use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A gRPC call shared by collection files and editable drafts.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct GrpcRequest {
    /// The server as `host:port`, which may contain `{{variables}}`. A
    /// `grpcs://` or `https://` scheme selects TLS, `grpc://` or `http://`
    /// a plaintext connection.
    pub url: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tls: bool,
    /// The selected method as `package.Service/Method`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
    /// The JSON message sent when the method is invoked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "crate::Auth::is_inherit")]
    pub auth: crate::Auth,
    #[serde(default, skip_serializing_if = "GrpcDefinition::is_reflection")]
    pub definition: GrpcDefinition,
    #[serde(default, skip_serializing_if = "GrpcSettings::is_default")]
    pub settings: GrpcSettings,
    #[serde(default, skip_serializing_if = "crate::GrpcScripts::is_empty")]
    pub scripts: crate::GrpcScripts,
}

/// Per-request options, as in Postman's gRPC Settings tab.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct GrpcSettings {
    /// Unset follows the certificate verification preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_certificates: Option<bool>,
    /// Check the server certificate against this name instead of the host.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub server_name: String,
    /// Show response fields that have their default value, such as `""` or `0`.
    pub include_default_fields: bool,
    /// Largest response message in MiB, zero for any size. Unset follows the
    /// response size preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_response_message_mb: Option<u64>,
    /// Deadline for unary calls and server reflection, zero for none. Unset
    /// follows the timeout preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl Default for GrpcSettings {
    fn default() -> Self {
        Self {
            verify_certificates: None,
            server_name: String::new(),
            include_default_fields: true,
            max_response_message_mb: None,
            timeout_ms: None,
        }
    }
}

impl GrpcSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Where a request finds its services and message types.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum GrpcDefinition {
    /// Ask the server, using the gRPC server reflection protocol.
    #[default]
    Reflection,
    /// Compile a local `.proto` file. Its imports resolve from the import
    /// paths in order, then from the file's own directory. Relative paths
    /// resolve from the collection directory.
    ProtoFile {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        import_paths: Vec<PathBuf>,
    },
}

impl GrpcDefinition {
    pub fn is_reflection(&self) -> bool {
        matches!(self, Self::Reflection)
    }

    /// Store paths inside `collection` relative to it, so the collection
    /// keeps working when it is renamed, moved or shared.
    pub fn relative_to(&self, collection: &Path) -> Self {
        let relative = |path: &PathBuf| {
            path.strip_prefix(collection)
                .map_or_else(|_| path.clone(), Path::to_path_buf)
        };

        match self {
            Self::Reflection => Self::Reflection,
            Self::ProtoFile { path, import_paths } => Self::ProtoFile {
                path: relative(path),
                import_paths: import_paths.iter().map(relative).collect(),
            },
        }
    }

    /// Resolve paths relative to `collection`, so the definition still
    /// loads for a copy of the request kept outside it.
    pub fn resolved_from(&self, collection: &Path) -> Self {
        let resolved = |path: &PathBuf| collection.join(path);

        match self {
            Self::Reflection => Self::Reflection,
            Self::ProtoFile { path, import_paths } => Self::ProtoFile {
                path: resolved(path),
                import_paths: import_paths.iter().map(resolved).collect(),
            },
        }
    }
}

impl GrpcRequest {
    /// The TLS setting after applying a scheme in the URL.
    pub fn uses_tls(&self) -> bool {
        match scheme(&self.url) {
            Some("grpcs" | "https") => true,
            Some("grpc" | "http") => false,
            _ => self.tls,
        }
    }
}

impl GrpcRequest {
    /// Turn TLS on or off, switching a scheme in the URL to match.
    pub fn set_tls(&mut self, tls: bool) {
        self.tls = tls;

        if let Some(("grpc" | "grpcs" | "http" | "https", rest)) = self.url.trim().split_once("://")
        {
            self.url = format!("{}://{rest}", if tls { "grpcs" } else { "grpc" });
        }
    }
}

pub(crate) fn scheme(url: &str) -> Option<&str> {
    url.trim()
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .filter(|scheme| !scheme.contains("{{"))
}
