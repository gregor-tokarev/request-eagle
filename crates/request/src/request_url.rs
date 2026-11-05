//! The parts of an HTTP request URL as written, before its `{{variables}}`
//! resolve: its query parameters and `:name` path variables.

use std::ops::Range;

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use url::form_urlencoded;

/// The URL's query parameters as the server reads them: `+` is a space and
/// percent escapes are decoded. `{{variables}}` stay as written. A parameter
/// without `=` has an empty value.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    url[query_range(url)]
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(decode_pair)
        .collect()
}

/// The URL with its query replaced by `params`, as `query_params` reads them.
/// The characters that would change their meaning are percent-encoded, so the
/// server reads what was entered; `{{variables}}` stay as written until they
/// resolve. A key or value that reads the same as in the parameter the URL
/// held for it keeps its spelling, so escapes the table cannot show, such as
/// encoded braces or bytes that are not UTF-8, are sent unchanged. A new
/// parameter with an empty value is written without `=`.
pub fn with_query_params(url: &str, params: &[(String, String)]) -> String {
    let query = query_range(url);
    let current: Vec<_> = url[query.clone()]
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| (pair, decode_pair(pair)))
        .collect();
    let originals = match_originals(&current, params);
    let start = url[..query.start]
        .strip_suffix('?')
        .map_or(query.start, str::len);
    let mut written = url[..start].to_owned();

    for (index, (key, value)) in params.iter().enumerate() {
        written.push(if index == 0 { '?' } else { '&' });

        let Some(original) = originals[index] else {
            written.push_str(&encode(key));
            if !value.is_empty() {
                written.push('=');
                written.push_str(&encode(value));
            }
            continue;
        };

        let (pair, (old_key, old_value)) = &current[original];
        if old_key == key && old_value == value {
            written.push_str(pair);
            continue;
        }

        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        if old_key == key {
            written.push_str(raw_key);
        } else {
            written.push_str(&encode(key));
        }
        if old_value == value && !value.is_empty() {
            written.push('=');
            written.push_str(raw_value);
        } else if !value.is_empty() {
            written.push('=');
            written.push_str(&encode(value));
        }
    }

    written.push_str(&url[query.end..]);
    written
}

/// The parameter of the URL each row of `params` came from. The most
/// unchanged rows are matched in their order, nearest to where they were; an
/// edited row is matched to a parameter left between the same unchanged
/// ones, so an edit of its key or value keeps the other.
fn match_originals(
    current: &[(&str, (String, String))],
    params: &[(String, String)],
) -> Vec<Option<usize>> {
    let unchanged = |i: usize, j: usize| current[i].1 == params[j];
    // The most matches of the first `i` parameters and `j` rows, then the
    // least distance they moved.
    let mut best = vec![vec![(0, 0); params.len() + 1]; current.len() + 1];
    for i in 1..=current.len() {
        for j in 1..=params.len() {
            let mut score = best[i - 1][j].max(best[i][j - 1]);
            if unchanged(i - 1, j - 1) {
                let (matches, moved) = best[i - 1][j - 1];
                score = score.max((matches + 1, moved - i.abs_diff(j) as isize));
            }
            best[i][j] = score;
        }
    }

    let mut anchors = vec![(current.len(), params.len())];
    let (mut i, mut j) = (current.len(), params.len());
    while i > 0 && j > 0 {
        let (matches, moved) = best[i - 1][j - 1];
        if unchanged(i - 1, j - 1) && best[i][j] == (matches + 1, moved - i.abs_diff(j) as isize) {
            i -= 1;
            j -= 1;
            anchors.push((i, j));
        } else if best[i][j] == best[i - 1][j] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    anchors.reverse();

    let mut originals = vec![None; params.len()];
    let (mut i, mut j) = (0, 0);
    for (anchor_i, anchor_j) in anchors {
        for (original, row) in (i..anchor_i).zip(j..anchor_j) {
            originals[row] = Some(original);
        }
        if anchor_j < params.len() {
            originals[anchor_j] = Some(anchor_i);
        }
        (i, j) = (anchor_i + 1, anchor_j + 1);
    }

    originals
}

/// A `key=value` pair of the query, decoded. Without `=` the value is empty.
fn decode_pair(pair: &str) -> (String, String) {
    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
    (decode(key), decode(value))
}

/// The `:name` segments of the URL's path, where each starts and its name.
/// The scheme, host and port are not part of the path, and a name cannot
/// contain a `{{variable}}`.
pub fn path_variables(url: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let path = path_range(url);
    let mut offset = path.start;

    url[path].split('/').filter_map(move |segment| {
        let start = offset;
        offset += segment.len() + 1;

        let name = segment
            .strip_prefix(':')
            .filter(|name| !name.is_empty() && !name.contains("{{"))?;
        Some((start..start + segment.len(), name))
    })
}

/// Substitute the path variables of a resolved URL that have a value. Ones
/// without a value are sent as written. Each value is resolved with `resolve`,
/// then the characters that would end the path or start a `{{variable}}` are
/// encoded, so the value stays in its segment. A `/` in a value is kept.
pub fn fill_path_variables<E>(
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

/// The characters of a query key or value that a server would not read as
/// themselves: those that end or split a parameter, start an escape, or stand
/// for a space.
const QUERY_RESERVED: &AsciiSet = &CONTROLS.add(b'%').add(b'&').add(b'=').add(b'#').add(b'+');

/// Percent-encode a query key or value around its `{{variables}}`. Spaces at
/// its ends are encoded too, since sending trims the URL.
fn encode(text: &str) -> String {
    let inner = text.trim_matches(' ');
    let leading = text.len() - text.trim_start_matches(' ').len();
    let trailing = text.len() - leading - inner.len();

    let mut encoded = "%20".repeat(leading);
    encoded.push_str(&around_variables(inner, |part| {
        utf8_percent_encode(part, QUERY_RESERVED).to_string()
    }));
    encoded.push_str(&"%20".repeat(trailing));
    encoded
}

/// Decode a query key or value as a server reads it, around its
/// `{{variables}}`.
fn decode(text: &str) -> String {
    around_variables(text, |part| {
        percent_decode_str(&part.replace('+', " "))
            .decode_utf8_lossy()
            .into_owned()
    })
}

/// Form-encode the text around `{{variables}}`.
fn form_encode(text: &str) -> String {
    around_variables(text, |part| {
        form_urlencoded::byte_serialize(part.as_bytes()).collect()
    })
}

/// Apply `map` to the text around `{{variables}}`, which stay as written.
fn around_variables(text: &str, mut map: impl FnMut(&str) -> String) -> String {
    let mut mapped = String::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("{{")
        && let Some(end) = remaining[start..].find("}}")
    {
        let end = start + end + 2;
        mapped.push_str(&map(&remaining[..start]));
        mapped.push_str(&remaining[start..end]);
        remaining = &remaining[end..];
    }

    mapped.push_str(&map(remaining));
    mapped
}
