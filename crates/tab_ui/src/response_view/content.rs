use gpui_kit::{Image, ImageFormat, SharedString};
use request::{Execution, HttpResponse, Response};
use std::{
    borrow::Cow,
    sync::Arc,
    time::{Duration, Instant},
};

pub struct ResponseContent {
    pub(super) execution: Execution,
    /// The body as text. Empty when the body is binary.
    pub(super) raw: SharedString,
    /// Highlighted text, reformatted for JSON and XML.
    pub(super) pretty: Option<SharedString>,
    pub(super) raw_only: bool,
    pub(super) language: &'static str,
    /// Whether the body's bytes are not text.
    pub(super) binary: bool,
    pub(super) preview: Option<Preview>,
    /// The last segment of the request's path, to name a saved body.
    pub(super) url_name: Option<String>,
    pub(super) processing: Duration,
    pub(super) headers: Arc<[(SharedString, SharedString)]>,
    pub(super) cookies: Arc<[(SharedString, SharedString)]>,
    /// The size of a body that history did not keep.
    pub(super) omitted_body: Option<usize>,
}

/// HTML up to this many bytes can be previewed.
const HTML_PREVIEW_LIMIT: usize = 512 * 1024;

/// A body that can be shown as what it represents, besides its text or bytes.
#[derive(Clone)]
pub(super) enum Preview {
    Html,
    Image(Arc<Image>),
    /// The document's bytes, which its preview shares.
    Pdf(Arc<Vec<u8>>),
}

impl ResponseContent {
    /// Prepare display text off the UI thread. The body is kept once: in its
    /// preview, as its text when the text reads as its bytes, or else in the
    /// response.
    pub fn new(mut execution: Execution) -> Self {
        let started = Instant::now();
        let Response::Http(response) = &mut execution.response;
        let content_type = response
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let mut parameters = content_type.split(';');
        let media_type = parameters.next().unwrap_or("").trim().to_ascii_lowercase();
        let charset = parameters
            .filter_map(|parameter| parameter.split_once('='))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("charset"))
            .and_then(|(_, label)| {
                encoding_rs::Encoding::for_label(label.trim().trim_matches('"').as_bytes())
            });
        let body = std::mem::take(&mut response.body);

        let image = image_format(&media_type, &body);
        let pdf =
            !body.is_empty() && (media_type == "application/pdf" || body.starts_with(b"%PDF-"));
        let binary = match image {
            Some(ImageFormat::Svg) => false,
            Some(_) => true,
            None => pdf || (!is_text(&media_type) && looks_binary(&body)),
        };

        // Text is UTF-8 unless the response names another charset. `None`
        // means that decoding left the bytes as they are.
        let decoded = (!binary).then(|| {
            match charset
                .unwrap_or(encoding_rs::UTF_8)
                .decode_without_bom_handling(&body)
                .0
            {
                Cow::Borrowed(_) => None,
                Cow::Owned(text) => Some(text),
            }
        });
        let (raw, mut body): (SharedString, _) = match decoded {
            None => (SharedString::default(), Some(body)),
            Some(Some(text)) => (text.into(), Some(body)),
            // An SVG image keeps its bytes for its preview.
            Some(None) if image.is_some() => (
                String::from_utf8_lossy(&body).into_owned().into(),
                Some(body),
            ),
            Some(None) => (
                String::from_utf8(body)
                    .expect("text decoded unchanged is UTF-8")
                    .into(),
                None,
            ),
        };
        let language = if binary {
            "text"
        } else {
            language(&media_type, &raw)
        };

        let mut raw_only = exceeds_editor_limit(&raw);
        let mut pretty = match language {
            _ if raw_only => None,
            "json" => serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .map(Into::into),
            // Malformed XML still reads better highlighted.
            "xml" => Some(pretty_xml(&raw).map_or_else(|| raw.clone(), Into::into)),
            "html" | "javascript" | "css" | "yaml" => Some(raw.clone()),
            _ => None,
        };
        if pretty
            .as_ref()
            .is_some_and(|text| exceeds_editor_limit(text))
        {
            raw_only = true;
            pretty = None;
        }

        let preview = match image {
            Some(format) => body
                .take()
                .map(|bytes| Preview::Image(Arc::new(Image::from_bytes(format, bytes)))),
            None if pdf => body.take().map(|bytes| Preview::Pdf(Arc::new(bytes))),
            // A rendered page lays out all of its text at once.
            None if language == "html"
                && !raw.trim().is_empty()
                && raw.len() <= HTML_PREVIEW_LIMIT =>
            {
                Some(Preview::Html)
            }
            None => None,
        };
        // Bytes that neither the preview nor the text took stay in the response.
        response.body = body.unwrap_or_default();

        let headers = response
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string().into(),
                    String::from_utf8_lossy(value.as_bytes())
                        .into_owned()
                        .into(),
                )
            })
            .collect::<Arc<[(SharedString, SharedString)]>>();
        let cookies = headers
            .iter()
            .filter(|(name, _)| name == "set-cookie")
            .map(|(_, value)| {
                value
                    .split_once('=')
                    .map(|(name, value)| (name.to_owned().into(), value.to_owned().into()))
                    .unwrap_or_else(|| ("set-cookie".into(), value.clone()))
            })
            .collect();

        Self {
            execution,
            raw,
            pretty,
            raw_only,
            language,
            binary,
            preview,
            url_name: None,
            processing: started.elapsed(),
            headers,
            cookies,
            omitted_body: None,
        }
    }

    /// A response kept in history, to show again.
    pub fn recorded(response: request_history::Response) -> Self {
        let omitted_body = response.body.is_none().then_some(response.body_size);

        Self {
            omitted_body,
            ..Self::new(response.into_execution())
        }
    }

    /// Name a saved body after the request's URL, unless the server names it.
    pub(crate) fn named_after(mut self, url: &str) -> Self {
        self.url_name = super::save::url_file_name(url);
        self
    }

    pub(super) fn http(&self) -> &HttpResponse {
        let Response::Http(response) = &self.execution.response;
        response
    }

    /// The body's bytes, as received after decompression.
    pub(super) fn body(&self) -> &[u8] {
        match &self.preview {
            Some(Preview::Image(image)) => image.bytes(),
            Some(Preview::Pdf(bytes)) => bytes,
            // The text took over the bytes it reads as.
            _ if self.http().body.is_empty() => self.raw.as_bytes(),
            _ => &self.http().body,
        }
    }

    /// What the body is, as the body toolbar names it.
    pub(super) fn label(&self) -> &'static str {
        match &self.preview {
            Some(Preview::Image(image)) => match image.format {
                ImageFormat::Png => "PNG",
                ImageFormat::Jpeg => "JPEG",
                ImageFormat::Webp => "WebP",
                ImageFormat::Gif => "GIF",
                ImageFormat::Svg => "SVG",
                ImageFormat::Bmp => "BMP",
                ImageFormat::Tiff => "TIFF",
                ImageFormat::Ico => "ICO",
                ImageFormat::Pnm => "PNM",
            },
            Some(Preview::Pdf(_)) => "PDF",
            _ if self.binary => "Binary",
            _ => match self.language {
                "json" => "JSON",
                "html" => "HTML",
                "xml" => "XML",
                "javascript" => "JavaScript",
                "css" => "CSS",
                "yaml" => "YAML",
                _ => "Text",
            },
        }
    }
}

/// Whether text is too large for the highlighted editor to stay responsive.
pub(crate) fn exceeds_editor_limit(text: &str) -> bool {
    text.len() > 256 * 1024 || text.split('\n').any(|line| line.len() > 32 * 1024)
}

/// The format of an image body. Servers often label images generically, so
/// distinctive signatures count regardless of the media type.
fn image_format(media_type: &str, body: &[u8]) -> Option<ImageFormat> {
    if body.is_empty() {
        return None;
    }

    let signature = if body.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if body.starts_with(b"\xff\xd8\xff") {
        Some(ImageFormat::Jpeg)
    } else if body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if body.starts_with(b"RIFF") && body.get(8..12) == Some(b"WEBP") {
        Some(ImageFormat::Webp)
    } else {
        None
    };

    let declared = match media_type {
        "image/x-icon" | "image/vnd.microsoft.icon" => Some(ImageFormat::Ico),
        "image/x-ms-bmp" => Some(ImageFormat::Bmp),
        media_type => ImageFormat::from_mime_type(media_type),
    };

    signature.or(declared)
}

/// Media types whose bodies are text, even when they are not valid UTF-8.
fn is_text(media_type: &str) -> bool {
    media_type.starts_with("text/")
        || is_xml(media_type)
        || ["json", "javascript", "ecmascript", "yaml", "graphql"]
            .iter()
            .any(|name| media_type.contains(name))
        || media_type == "application/x-www-form-urlencoded"
}

/// XML media types; Office documents mention XML in their names, but are ZIP files.
fn is_xml(media_type: &str) -> bool {
    media_type.ends_with("/xml") || media_type.ends_with("+xml")
}

/// Whether bytes of an undeclared kind are binary: their start contains a NUL
/// byte or is not UTF-8.
fn looks_binary(body: &[u8]) -> bool {
    let start = &body[..body.len().min(8 * 1024)];

    if start.contains(&0) {
        return true;
    }

    match std::str::from_utf8(start) {
        Ok(_) => false,
        // The sample may end inside a character.
        Err(error) => error.error_len().is_some() || start.len() == body.len(),
    }
}

/// The highlighting language of a text body, by its media type or, for
/// generic types, by how it starts.
fn language(media_type: &str, text: &str) -> &'static str {
    let start = text.trim_start_matches('\u{feff}').trim_start();
    let starts_with = |prefix: &str| {
        start
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };

    if media_type.contains("json") {
        "json"
    } else if media_type.contains("html") {
        "html"
    } else if is_xml(media_type) {
        "xml"
    } else if media_type.contains("javascript") || media_type.contains("ecmascript") {
        "javascript"
    } else if media_type == "text/css" {
        "css"
    } else if media_type.contains("yaml") {
        "yaml"
    } else if start.starts_with(['{', '[']) {
        "json"
    } else if starts_with("<!doctype html") || starts_with("<html") {
        "html"
    } else if starts_with("<?xml") {
        "xml"
    } else {
        "text"
    }
}

/// Indent XML elements, or `None` when the text is not well-formed XML or
/// indenting could change its text. Whitespace between elements is layout;
/// text keeps its whitespace, so mixed content and `xml:space` stay as received.
fn pretty_xml(text: &str) -> Option<String> {
    use quick_xml::{Reader, Writer, events::Event};

    let blank = |event: &Event| matches!(event, Event::Text(text) if text.iter().all(u8::is_ascii_whitespace));
    let mut reader = Reader::from_str(text);
    // Each event, with the index of its parent element's start.
    let mut events = Vec::new();
    // The open elements' starts, and whether each contains elements and text.
    let mut open: Vec<(usize, bool, bool)> = Vec::new();
    let mut parents = std::collections::HashSet::new();

    loop {
        let event = reader.read_event().ok()?.into_owned();
        match &event {
            Event::Eof => break,
            Event::Start(element) | Event::Empty(element) => {
                if element.attributes().any(|attribute| {
                    attribute.is_ok_and(|attribute| attribute.key.as_ref() == b"xml:space")
                }) {
                    return None;
                }
                if let Some(parent) = open.last_mut() {
                    parent.1 = true;
                }
            }
            Event::End(_) => {
                let (start, elements, text) = open.pop()?;
                if elements && text {
                    return None;
                }
                if elements {
                    parents.insert(start);
                }
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if !blank(&event) => {
                if let Some(parent) = open.last_mut() {
                    parent.2 = true;
                }
            }
            _ => {}
        }

        let parent = open.last().map(|(start, ..)| *start);
        if matches!(event, Event::Start(_)) {
            open.push((events.len(), false, false));
        }
        events.push((event, parent));
    }

    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    for (event, parent) in events {
        let layout = blank(&event) && parent.is_none_or(|parent| parents.contains(&parent));
        if !layout {
            writer.write_event(event).ok()?;
        }
    }

    String::from_utf8(writer.into_inner()).ok()
}
