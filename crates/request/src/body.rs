//! What an HTTP request sends as its body, and the bytes it becomes when
//! the request is sent.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer},
};
use url::form_urlencoded::byte_serialize;

use crate::ExecutionError;

/// An HTTP request's body. GET and HEAD requests are sent without one.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Body {
    /// Text sent as written. Its language decides the highlighting and the
    /// default `Content-Type`.
    Raw { language: RawLanguage, text: String },
    /// An `application/x-www-form-urlencoded` form. Each name and value is
    /// encoded after its `{{variables}}` are filled in.
    UrlEncoded { fields: Vec<(String, String)> },
    /// A `multipart/form-data` form of text fields and files.
    Multipart { parts: Vec<FormPart> },
    /// The contents of a file, read when the request is sent.
    Binary { file: PathBuf },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RawLanguage {
    #[default]
    Json,
    Xml,
    Text,
}

/// A field of a multipart form.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct FormPart {
    pub name: String,
    /// The field's text, or the path of the file it sends.
    pub value: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub file: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl RawLanguage {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Xml => "application/xml",
            Self::Text => "text/plain",
        }
    }
}

impl Body {
    pub fn json(text: impl Into<String>) -> Self {
        Self::Raw {
            language: RawLanguage::Json,
            text: text.into(),
        }
    }

    /// The `Content-Type` the body is sent with unless the request sets one.
    /// A multipart form's also names the boundary between its parts, which
    /// is chosen when it is sent.
    pub fn content_type(&self) -> String {
        match self {
            Self::Raw { language, .. } => language.content_type().into(),
            Self::UrlEncoded { .. } => "application/x-www-form-urlencoded".into(),
            Self::Multipart { .. } => "multipart/form-data".into(),
            Self::Binary { file } => file_type(file),
        }
    }

    /// Store paths of files inside `collection` relative to it, so the
    /// collection keeps working when it is renamed, moved or shared.
    pub fn relative_to(&self, collection: &Path) -> Self {
        self.with_files(|path| path.strip_prefix(collection).ok().map(Path::to_path_buf))
    }

    /// Resolve paths relative to `collection`, as they are sent.
    pub fn resolved_from(&self, collection: &Path) -> Self {
        self.with_files(|path| {
            (!path.as_os_str().is_empty() && path.is_relative()).then(|| collection.join(path))
        })
    }

    /// The body with each file path that `change` gives a new one replaced.
    fn with_files(&self, change: impl Fn(&Path) -> Option<PathBuf>) -> Self {
        let mut body = self.clone();

        match &mut body {
            Self::Multipart { parts } => {
                for part in parts.iter_mut().filter(|part| part.file) {
                    if let Some(path) = change(Path::new(&part.value)) {
                        part.value = path.to_string_lossy().into_owned();
                    }
                }
            }
            Self::Binary { file } => {
                if let Some(path) = change(file) {
                    *file = path;
                }
            }
            Self::Raw { .. } | Self::UrlEncoded { .. } => {}
        }

        body
    }

    /// The bytes sent, and the `Content-Type` they need. Files are read now.
    /// A raw body's text moves into the bytes.
    pub(crate) fn encode(&mut self) -> Result<(Vec<u8>, String), ExecutionError> {
        match self {
            Self::Raw { language, text } => Ok((
                std::mem::take(text).into_bytes(),
                language.content_type().into(),
            )),
            Self::UrlEncoded { fields } => Ok((
                url_encoded(fields).into_bytes(),
                "application/x-www-form-urlencoded".into(),
            )),
            Self::Multipart { parts } => {
                let boundary = boundary();
                let content_type = format!("multipart/form-data; boundary={boundary}");
                Ok((multipart(parts, &boundary)?, content_type))
            }
            Self::Binary { file } => {
                if file.as_os_str().is_empty() {
                    return Err(ExecutionError::MissingFile("the body".into()));
                }

                Ok((read(file)?, file_type(file)))
            }
        }
    }
}

/// The form's fields, each name and value encoded as browsers encode forms.
pub(crate) fn url_encoded(fields: &[(String, String)]) -> String {
    let encode = |text: &str| byte_serialize(text.as_bytes()).collect::<String>();

    fields
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn multipart(parts: &[FormPart], boundary: &str) -> Result<Vec<u8>, ExecutionError> {
    // As browsers do, quotes and line breaks in names are escaped.
    let escape = |name: &str| {
        name.replace('"', "%22")
            .replace('\r', "%0D")
            .replace('\n', "%0A")
    };
    let mut body = Vec::new();

    for part in parts {
        let name = escape(&part.name);
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());

        if part.file {
            if part.value.is_empty() {
                return Err(ExecutionError::MissingFile(format!(
                    "the form field \"{}\"",
                    part.name
                )));
            }

            let path = Path::new(&part.value);
            let file_name = path
                .file_name()
                .map(|name| escape(&name.to_string_lossy()))
                .unwrap_or_default();
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: {}\r\n\r\n",
                    file_type(path)
                )
                .as_bytes(),
            );
            body.extend_from_slice(&read(path)?);
        } else {
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            );
            body.extend_from_slice(part.value.as_bytes());
        }

        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    Ok(body)
}

fn read(path: &Path) -> Result<Vec<u8>, ExecutionError> {
    std::fs::read(path).map_err(|source| ExecutionError::BodyFile {
        path: path.to_owned(),
        source,
    })
}

/// The media type of a file, by its extension.
fn file_type(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
}

/// A boundary that the parts are unlikely to contain.
fn boundary() -> String {
    let mut bytes = [0; 12];
    // Random bytes only make a collision less likely; zeros still work.
    let _ = SystemRandom::new().fill(&mut bytes);
    let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();

    format!("RequestEagleBoundary{suffix}")
}

/// Reads a stored body. Before body types, a body was its raw bytes, sent
/// as JSON unless a header said otherwise.
pub(crate) fn stored<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Body>, D::Error> {
    struct StoredBody;

    impl<'de> Visitor<'de> for StoredBody {
        type Value = Option<Body>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a body table or an array of bytes")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut bytes: A) -> Result<Self::Value, A::Error> {
            let mut text = Vec::new();
            while let Some(byte) = bytes.next_element::<u8>()? {
                text.push(byte);
            }

            Ok(Some(Body::json(String::from_utf8_lossy(&text))))
        }

        fn visit_map<A: MapAccess<'de>>(self, body: A) -> Result<Self::Value, A::Error> {
            Body::deserialize(MapAccessDeserializer::new(body)).map(Some)
        }
    }

    deserializer.deserialize_option(StoredBody)
}
