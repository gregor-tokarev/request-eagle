//! The parts of an HTTP request URL as written, before its `{{variables}}`
//! resolve: its query parameters and `:name` path variables.

use std::ops::Range;

use url::form_urlencoded;

/// The URL's query parameters as written, without decoding them. A parameter
/// without `=` has an empty value.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    let query = &url[query_range(url)];

    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

/// The URL with its query replaced by `params`. Keys and values are written as
/// they are, except for the characters that would end them. A parameter with
/// an empty value is written without `=`.
pub fn with_query_params(url: &str, params: &[(String, String)]) -> String {
    let query = query_range(url);
    let start = url[..query.start]
        .strip_suffix('?')
        .map_or(query.start, str::len);
    let mut written = url[..start].to_owned();

    for (index, (key, value)) in params.iter().enumerate() {
        written.push(if index == 0 { '?' } else { '&' });
        written.push_str(&escape(key, &['&', '=', '#']));

        if !value.is_empty() {
            written.push('=');
            written.push_str(&escape(value, &['&', '#']));
        }
    }

    written.push_str(&url[query.end..]);
    written
}

/// The `:name` segments of the URL's path, where each starts and its name.
/// The scheme, host and port are not part of the path.
pub fn path_variables(url: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let path = path_range(url);
    let mut offset = path.start;

    url[path].split('/').filter_map(move |segment| {
        let start = offset;
        offset += segment.len() + 1;

        let name = segment.strip_prefix(':').filter(|name| !name.is_empty())?;
        Some((start..start + segment.len(), name))
    })
}

/// Substitute the path variables that have a value. Ones without a value are
/// sent as written. Each value is resolved with `resolve` first, then the
/// characters that would end the path or start a `{{variable}}` are encoded,
/// so the value stays in its place. A `/` in a value is kept.
pub(crate) fn fill_path_variables<E>(
    url: &str,
    values: &[(String, String)],
    mut resolve: impl FnMut(&str) -> Result<String, E>,
) -> Result<String, E> {
    let mut filled = String::new();
    let mut written = 0;

    for (range, name) in path_variables(url) {
        let Some((_, value)) = values
            .iter()
            .find(|(key, value)| key == name && !value.is_empty())
        else {
            continue;
        };

        filled.push_str(&url[written..range.start]);
        filled.push_str(&escape(&resolve(value)?, &['?', '#', '{', '}']));
        written = range.end;
    }

    filled.push_str(&url[written..]);
    Ok(filled)
}

/// Append `params` to the URL's query, encoded as sending appended them
/// beside the URL, so the same request is sent. `{{variables}}` stay as
/// written; they resolve when the request is sent.
pub(crate) fn append_encoded_query(url: &str, params: &[(String, String)]) -> String {
    // Sending trimmed the URL before appending them.
    let url = url.trim();
    let query = query_range(url);
    let mut written = url[..query.end].to_owned();
    let mut separator = match url[..query.end].find('?') {
        None => "?",
        Some(_) if query.is_empty() => "",
        Some(_) => "&",
    };

    for (key, value) in params {
        written.push_str(separator);
        separator = "&";

        written.push_str(&form_encode(key));
        written.push('=');
        written.push_str(&form_encode(value));
    }

    written.push_str(&url[query.end..]);
    written
}

/// The query, after `?` and before any fragment. Empty, at the end of the
/// path, when there is none.
fn query_range(url: &str) -> Range<usize> {
    let end = url.find('#').unwrap_or(url.len());

    match url[..end].find('?') {
        Some(start) => start + 1..end,
        None => end..end,
    }
}

/// The path, after the scheme and host and before the query or fragment.
fn path_range(url: &str) -> Range<usize> {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let host = url[..end].find("://").map_or(0, |scheme| scheme + 3);
    let start = url[host..end].find('/').map_or(end, |slash| host + slash);

    start..end
}

fn escape(text: &str, reserved: &[char]) -> String {
    text.chars()
        .map(|ch| {
            if reserved.contains(&ch) {
                format!("%{:02X}", ch as u32)
            } else {
                ch.to_string()
            }
        })
        .collect()
}

/// Form-encode the text around `{{variables}}`.
fn form_encode(text: &str) -> String {
    let mut encoded = String::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("{{")
        && let Some(end) = remaining[start..].find("}}")
    {
        let end = start + end + 2;
        encoded.extend(form_urlencoded::byte_serialize(
            &remaining.as_bytes()[..start],
        ));
        encoded.push_str(&remaining[start..end]);
        remaining = &remaining[end..];
    }

    encoded.extend(form_urlencoded::byte_serialize(remaining.as_bytes()));
    encoded
}
