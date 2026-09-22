use crate::{FormBody, HttpError, MultipartField};

impl FormBody {
    /// Multipart requests add their generated boundary when sent.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::UrlEncoded(_) => "application/x-www-form-urlencoded",
            Self::Multipart(_) => "multipart/form-data",
        }
    }

    /// Preview the encoded length without opening upload files.
    pub fn encoded_len(&self) -> Option<usize> {
        match self {
            Self::UrlEncoded(fields) => Some(encode_urlencoded(fields).len()),
            Self::Multipart(_) => None,
        }
    }

    pub(crate) async fn encode(self) -> Result<(Vec<u8>, String), HttpError> {
        match self {
            Self::UrlEncoded(fields) => Ok((
                encode_urlencoded(&fields),
                "application/x-www-form-urlencoded".into(),
            )),
            Self::Multipart(fields) => encode_multipart(fields).await,
        }
    }
}

fn encode_urlencoded(fields: &[(String, String)]) -> Vec<u8> {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish()
        .into_bytes()
}

async fn encode_multipart(fields: Vec<MultipartField>) -> Result<(Vec<u8>, String), HttpError> {
    let mut parts = Vec::with_capacity(fields.len());

    for field in fields {
        let part = match field {
            MultipartField::Text { name, value } => {
                let name = escape_parameter(&name);

                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}")
                    .into_bytes()
            }
            MultipartField::File { name, path } => {
                let data = smol::fs::read(&path)
                    .await
                    .map_err(|source| HttpError::ReadUpload {
                        path: path.clone(),
                        source,
                    })?;
                let name = escape_parameter(&name);
                let filename =
                    escape_parameter(&path.file_name().unwrap_or_default().to_string_lossy());
                let mut part = format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .into_bytes();

                part.extend_from_slice(&data);
                part
            }
        };

        parts.push(part);
    }

    // The delimiter must not occur in field values or uploaded bytes. Checking
    // the completed parts also covers names and filenames without random state.
    let boundary = (0_u64..)
        .map(|counter| format!("request-eagle-boundary-{counter:x}"))
        .find(|candidate| {
            !parts.iter().any(|part| {
                part.windows(candidate.len())
                    .any(|window| window == candidate.as_bytes())
            })
        })
        .expect("a multipart boundary is available");
    let mut body = Vec::new();

    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(&part);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    Ok((body, format!("multipart/form-data; boundary={boundary}")))
}

fn escape_parameter(value: &str) -> String {
    let mut escaped = String::new();

    for character in value.chars() {
        match character {
            '"' => escaped.push_str("%22"),
            '\\' => escaped.push_str("\\\\"),
            character if character.is_ascii_control() => {
                use std::fmt::Write as _;

                write!(escaped, "%{:02X}", character as u8).unwrap();
            }
            character => escaped.push(character),
        }
    }

    escaped
}
