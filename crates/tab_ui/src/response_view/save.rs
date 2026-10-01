use std::path::Path;

use gpui_kit::*;

use super::content::{Preview, ResponseContent};
use super::view::ResponseView;

impl ResponseView {
    /// Ask where to save the body's bytes, as received after decompression.
    pub(super) fn save_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = &self.content else {
            return;
        };

        let body = content.http().body.clone();
        let directory = std::env::home_dir()
            .map(|home| home.join("Downloads"))
            .filter(|downloads| downloads.is_dir())
            .or_else(std::env::home_dir)
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&directory, Some(&content.file_name()));

        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            let path = match path.await {
                Ok(Ok(Some(path))) => path,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.saved = Some(Err(
                            format!("Could not open the save dialog: {error}").into()
                        ));
                        cx.notify();
                    });
                    return;
                }
                // The dialog was cancelled.
                _ => return,
            };

            let written = cx
                .background_executor()
                .spawn(async move { std::fs::write(&path, body).map(|()| path) })
                .await
                .map_err(|error| format!("Could not save the response: {error}").into());
            let _ = this.update(cx, |this, cx| {
                this.saved = Some(written);
                cx.notify();
            });
        }));
    }
}

impl ResponseContent {
    /// A name for the saved body: the one the server suggests, else the last
    /// segment of the request's path, else a generic one. Each has an
    /// extension for the body's kind when it lacks one.
    pub(super) fn file_name(&self) -> String {
        let name = self
            .http()
            .headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok())
            .and_then(disposition_file_name)
            .or_else(|| self.url_name.clone())
            .unwrap_or_else(|| "response".to_owned());

        if Path::new(&name).extension().is_some() {
            name
        } else {
            format!("{name}.{}", self.extension())
        }
    }

    fn extension(&self) -> &'static str {
        match &self.preview {
            Some(Preview::Image(image)) => image.format.extension(),
            Some(Preview::Pdf) => "pdf",
            _ if self.binary => "bin",
            _ => match self.language {
                "json" => "json",
                "html" => "html",
                "xml" => "xml",
                "javascript" => "js",
                "css" => "css",
                "yaml" => "yaml",
                _ => "txt",
            },
        }
    }
}

/// The last segment of a URL's path, when it can name a file. Segments with
/// unresolved variables cannot.
pub(super) fn url_file_name(url: &str) -> Option<String> {
    let url = url.split(['?', '#']).next()?;
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        None => url,
    };
    let segment = path.rsplit('/').next()?;

    if segment.contains("{{") {
        return None;
    }

    file_name(&percent_decode(segment))
}

/// The file name in a Content-Disposition header, preferring the encoded
/// `filename*` form.
fn disposition_file_name(value: &str) -> Option<String> {
    let parameters: Vec<_> = value
        .split(';')
        .filter_map(|parameter| parameter.split_once('='))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim()))
        .collect();

    let encoded = parameters
        .iter()
        .find(|(name, _)| name == "filename*")
        .and_then(|(_, value)| {
            // charset'language'percent-encoded name
            let (charset, rest) = value.split_once('\'')?;
            let (_, name) = rest.split_once('\'')?;
            charset
                .eq_ignore_ascii_case("utf-8")
                .then(|| percent_decode(name))
        });
    let plain = || {
        parameters
            .iter()
            .find(|(name, _)| name == "filename")
            .map(|(_, value)| value.trim_matches('"').to_owned())
    };

    encoded.or_else(plain).as_deref().and_then(file_name)
}

/// The final component of a suggested name, without control characters.
fn file_name(name: &str) -> Option<String> {
    let name: String = name
        .rsplit(['/', '\\'])
        .next()?
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let name = name.trim();

    (!name.is_empty() && name != "." && name != "..").then(|| name.to_owned())
}

fn percent_decode(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();

    while let Some((&byte, tail)) = rest.split_first() {
        let decoded = (byte == b'%')
            .then(|| tail.get(..2))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());

        match decoded {
            Some(decoded) => {
                bytes.push(decoded);
                rest = &tail[2..];
            }
            None => {
                bytes.push(byte);
                rest = tail;
            }
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}
