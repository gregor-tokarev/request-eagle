//! The parts of a request URL as written, before its `{{variables}}`
//! resolve: its query parameters, `:name` path variables and display labels.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use url::{Position, Url, form_urlencoded};

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
        // The Params table leaves out a parameter without a key or value, so
        // no row comes from one.
        .filter(|(_, (key, value))| !key.is_empty() || !value.is_empty())
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

/// The most pairs of changed rows and parameters `match_originals` compares
/// to align them by their order; past it each unchanged row is matched to the
/// next parameter that reads the same.
const MAX_ALIGNED: usize = 1 << 16;

/// The parameter of the URL each row of `params` came from. The most
/// unchanged rows are matched in their order, nearest to where they were; an
/// edited row is matched to a parameter left between the same unchanged
/// ones, so an edit of its key or value keeps the other.
fn match_originals(
    current: &[(&str, (String, String))],
    params: &[(String, String)],
) -> Vec<Option<usize>> {
    let unchanged = |i: usize, j: usize| current[i].1 == params[j];
    let (n, m) = (current.len(), params.len());
    // An edit changes a few rows, so those before and after it are matched
    // as they are and only the rest are aligned.
    let prefix = (0..n.min(m)).take_while(|&k| unchanged(k, k)).count();
    let suffix = (0..n.min(m) - prefix)
        .take_while(|&k| unchanged(n - 1 - k, m - 1 - k))
        .count();
    let (rows, cols) = (n - prefix - suffix, m - prefix - suffix);

    // Unchanged rows matched in the middle, by their offset into it, then the
    // end of the middle.
    let mut anchors = Vec::new();
    if rows * cols <= MAX_ALIGNED {
        let pair = |i: usize, j: usize| unchanged(prefix + i, prefix + j);
        // The most matches of the first `i` parameters and `j` rows, then the
        // least distance they moved.
        let mut best = vec![vec![(0, 0); cols + 1]; rows + 1];
        let step = |i: usize, j: usize, best: &[Vec<(usize, isize)>]| {
            let (matches, moved) = best[i - 1][j - 1];
            (matches + 1, moved - i.abs_diff(j) as isize)
        };
        for i in 1..=rows {
            for j in 1..=cols {
                let mut score = best[i - 1][j].max(best[i][j - 1]);
                if pair(i - 1, j - 1) {
                    score = score.max(step(i, j, &best));
                }
                best[i][j] = score;
            }
        }

        let (mut i, mut j) = (rows, cols);
        while i > 0 && j > 0 {
            if pair(i - 1, j - 1) && best[i][j] == step(i, j, &best) {
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
    } else {
        let mut same: HashMap<&(String, String), VecDeque<usize>> = HashMap::new();
        for i in 0..rows {
            same.entry(&current[prefix + i].1).or_default().push_back(i);
        }
        let mut next = 0;
        for j in 0..cols {
            let Some(originals) = same.get_mut(&params[prefix + j]) else {
                continue;
            };
            while originals.front().is_some_and(|&i| i < next) {
                originals.pop_front();
            }
            if let Some(i) = originals.pop_front() {
                anchors.push((i, j));
                next = i + 1;
            }
        }
    }
    anchors.push((rows, cols));

    let mut originals: Vec<_> = (0..m).map(|j| (j < prefix).then_some(j)).collect();
    for k in 0..suffix {
        originals[m - 1 - k] = Some(n - 1 - k);
    }
    let (mut i, mut j) = (0, 0);
    for (anchor_i, anchor_j) in anchors {
        for (original, row) in (i..anchor_i).zip(j..anchor_j) {
            originals[prefix + row] = Some(prefix + original);
        }
        if anchor_j < cols {
            originals[prefix + anchor_j] = Some(prefix + anchor_i);
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

/// A short host and path label for a request URL, without its scheme,
/// credentials, query parameters, fragment or trailing slash.
pub fn endpoint_label(url: &str) -> Option<String> {
    endpoint_label_with_default_scheme(url, "https")
}

/// The full request URL for display, with any credentials in its authority
/// removed.
pub fn endpoint_tooltip(url: &str) -> Option<String> {
    endpoint_tooltip_with_default_scheme(url, "https")
}

pub(crate) fn endpoint_label_with_default_scheme(
    url: &str,
    default_scheme: &str,
) -> Option<String> {
    let cleaned = strip_url_controls(url);
    if cleaned.is_empty() {
        return None;
    }

    if !cleaned.contains("{{") && !is_relative_path(&cleaned) {
        return parse_display_url(&cleaned, default_scheme)
            .and_then(|(parsed, _)| url_label(&parsed));
    }

    format_endpoint_label(parse_endpoint(&cleaned)?)
}

pub(crate) fn endpoint_tooltip_with_default_scheme(
    url: &str,
    default_scheme: &str,
) -> Option<String> {
    let cleaned = strip_url_controls(url);
    if cleaned.is_empty() {
        return None;
    }

    if !cleaned.contains("{{") && !is_relative_path(&cleaned) {
        return parse_display_url(&cleaned, default_scheme).and_then(
            |(parsed, prepended_default)| url_tooltip(&parsed, &cleaned, prepended_default),
        );
    }

    Some(format_endpoint_tooltip(parse_endpoint(&cleaned)?))
}

/// A short label for a gRPC server URL using gRPC's server address rules:
/// only `://` introduces a scheme (so `http:50051` stays `http:50051`), and
/// explicit ports such as `:80` on `http://` stay visible because gRPC
/// defaults to port `443` for every scheme.
pub(crate) fn grpc_endpoint_label(url: &str) -> Option<String> {
    let cleaned = strip_url_controls(url);
    if cleaned.is_empty() {
        return None;
    }

    format_endpoint_label(parse_grpc_endpoint(&cleaned)?)
}

/// The full gRPC server URL for display using gRPC's server address rules,
/// with any credentials in its authority removed.
pub(crate) fn grpc_endpoint_tooltip(url: &str) -> Option<String> {
    let cleaned = strip_url_controls(url);
    if cleaned.is_empty() {
        return None;
    }

    Some(format_endpoint_tooltip(parse_grpc_endpoint(&cleaned)?))
}

fn format_endpoint_label(endpoint: ParsedEndpoint<'_>) -> Option<String> {
    let host = endpoint.host.strip_suffix(':').unwrap_or(endpoint.host);
    let path = endpoint.path.trim_end_matches(['/', '\\']);
    let mut label = String::new();

    if let Some(EndpointScheme::Template(prefix)) = endpoint.scheme {
        label.push_str(prefix);
    }
    label.push_str(host);
    label.push_str(path);

    (!label.is_empty()).then_some(label)
}

fn format_endpoint_tooltip(endpoint: ParsedEndpoint<'_>) -> String {
    let mut tooltip = String::new();

    if let Some(EndpointScheme::Concrete(prefix)) = endpoint.scheme {
        tooltip.push_str(&prefix);
    } else if let Some(EndpointScheme::Template(prefix)) = endpoint.scheme {
        tooltip.push_str(prefix);
    }
    tooltip.push_str(endpoint.host);
    tooltip.push_str(endpoint.path);
    tooltip.push_str(endpoint.suffix);

    tooltip
}

/// Strip leading/trailing whitespace and C0 controls (matching both `str::trim`
/// used when sending requests and the WHATWG URL preprocessor) and remove ASCII
/// tabs and newlines anywhere in the URL.
fn strip_url_controls(raw: &str) -> Cow<'_, str> {
    let trimmed = raw.trim_matches(|ch: char| ch <= ' ' || ch.is_whitespace());
    if trimmed.contains(['\t', '\n', '\r']) {
        Cow::Owned(trimmed.replace(['\t', '\n', '\r'], ""))
    } else {
        Cow::Borrowed(trimmed)
    }
}

/// Where the URL's query or fragment begins outside complete `{{...}}`
/// template placeholders, or `url.len()` when neither is present.
pub(crate) fn find_query_or_fragment(url: &str) -> usize {
    find_outside_templates(url, |byte| matches!(byte, b'?' | b'#')).unwrap_or(url.len())
}

/// Whether `raw` is a bare path starting with a single `/` or `\` rather than
/// a protocol-relative URL (`//...`) or a slash-prefixed authority such as
/// `/alice:secret@example.com/api` or `/localhost:/api`.
fn is_relative_path(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix(['/', '\\']) else {
        return false;
    };
    if rest.starts_with(['/', '\\']) {
        return false;
    }

    let first_segment_end =
        find_outside_templates(rest, |byte| matches!(byte, b'/' | b'\\' | b'?' | b'#'))
            .unwrap_or(rest.len());
    let first_segment = &rest[..first_segment_end];

    !rfind_outside_templates(first_segment, b'@').is_some_and(|at| at > 0)
        && find_outside_templates(first_segment, |byte| byte == b':').is_none()
}

/// Parse a template-free HTTP or WebSocket URL with WHATWG `Url`, prepending
/// `default_scheme` for bare addresses the way request execution does. Returns
/// the parsed URL and whether `default_scheme` was prepended.
fn parse_display_url(cleaned: &str, default_scheme: &str) -> Option<(Url, bool)> {
    let first_delim = cleaned.find(['/', '\\', '?', '#']).unwrap_or(cleaned.len());

    // Reject explicit `://` prefixes whose scheme is not valid RFC 3986 syntax
    // (such as `alice:secret@host://example.com` or `://example.com`).
    if let Some((name, _)) = cleaned.split_once("://")
        && name.len() < first_delim
    {
        if !is_scheme(name) {
            return None;
        }
        return parse_authority_url(cleaned).map(|url| (url, false));
    }

    // Special-scheme forms without `://` (such as `https:/user:pass@host/api`
    // or `ws:/user:pass@host/chat`) are parsed directly by WHATWG `Url` so
    // their credentials are recognized and stripped, unless they are a bare
    // host:port (including an empty port such as `http:/api` or `ws:/chat`).
    if !is_bare_special_host_port(cleaned)
        && let Some(colon) = cleaned.find(':').filter(|&colon| colon < first_delim)
        && is_special_scheme(&cleaned[..colon])
    {
        return parse_authority_url(cleaned).map(|url| (url, false));
    }

    let with_scheme = format!("{default_scheme}://{cleaned}");
    parse_authority_url(&with_scheme).map(|url| (url, true))
}

/// Parse a URL using the same WHATWG parser that request execution uses when it
/// yields an authority with a host (including special-scheme forms such as
/// `https:/user:pass@host/path`, `https:user:pass@host/path` or backslashes).
fn parse_authority_url(raw: &str) -> Option<Url> {
    let parsed = Url::parse(raw).ok()?;
    parsed.has_host().then_some(parsed)
}

fn url_label(url: &Url) -> Option<String> {
    let host = &url[Position::BeforeHost..Position::AfterPort];
    if host.is_empty() {
        return None;
    }

    let path = url.path().trim_end_matches('/');
    Some(format!("{host}{path}"))
}

fn url_tooltip(url: &Url, raw: &str, prepended_default: bool) -> Option<String> {
    if !url.has_host() {
        return None;
    }

    if !url.username().is_empty() || url.password().is_some() {
        let mut sanitized = url.clone();
        let _ = sanitized.set_username("");
        let _ = sanitized.set_password(None);

        if prepended_default {
            let without_scheme = &sanitized[Position::BeforeHost..];
            return Some(if raw.starts_with("//") {
                format!("//{without_scheme}")
            } else {
                without_scheme.to_owned()
            });
        }

        return Some(sanitized.into());
    }

    Some(raw.to_owned())
}

enum EndpointScheme<'a> {
    /// A concrete scheme prefix such as `https://` or `//`, omitted from the
    /// short label and kept in the full tooltip.
    Concrete(Cow<'a, str>),
    /// A templated scheme prefix such as `{{scheme}}://`, kept in both the
    /// short label and the full tooltip.
    Template(&'a str),
}

struct ParsedEndpoint<'a> {
    scheme: Option<EndpointScheme<'a>>,
    host: &'a str,
    path: &'a str,
    suffix: &'a str,
}

/// Split an arbitrary request URL (which may include `{{variables}}` or omit a
/// scheme) into its scheme prefix, credential-free host, path and query/fragment
/// suffix. Follows WHATWG special-scheme authority rules so forms such as
/// `https:/user:pass@{{host}}/api` never leak credentials. Returns `None` when
/// there is no host or path to identify it by, when an explicit `://` scheme
/// prefix is invalid, or when a leading template leaves authority credentials
/// ambiguous.
fn parse_endpoint(raw: &str) -> Option<ParsedEndpoint<'_>> {
    if raw.is_empty() {
        return None;
    }

    let first_delim =
        find_outside_templates(raw, |byte| matches!(byte, b'/' | b'\\' | b'?' | b'#'))
            .unwrap_or(raw.len());
    let (scheme, rest) = if let Some(sep) = find_str_outside_templates(raw, "://")
        && sep < first_delim
    {
        let name = &raw[..sep];
        let after = &raw[sep + 3..];
        let prefix = &raw[..sep + 3];
        if is_special_scheme(name) {
            let rest = after.trim_start_matches(['/', '\\']);
            let prefix = if after.starts_with(['/', '\\']) {
                Cow::Owned(format!("{name}://"))
            } else {
                Cow::Borrowed(prefix)
            };

            (Some(EndpointScheme::Concrete(prefix)), rest)
        } else if is_scheme(name) {
            (Some(EndpointScheme::Concrete(Cow::Borrowed(prefix))), after)
        } else if is_template_scheme(name) {
            (Some(EndpointScheme::Template(prefix)), after)
        } else {
            return None;
        }
    } else if !is_bare_special_host_port(raw)
        && let Some(colon) =
            find_outside_templates(raw, |byte| byte == b':').filter(|&colon| colon < first_delim)
        && is_special_scheme(&raw[..colon])
    {
        // Special schemes enter the WHATWG authority state after `:` and zero
        // or more `/` or `\` characters.
        let name = &raw[..colon];
        let rest = raw[colon + 1..].trim_start_matches(['/', '\\']);

        (
            Some(EndpointScheme::Concrete(Cow::Owned(format!("{name}://")))),
            rest,
        )
    } else if let Some(rest) = raw
        .strip_prefix("//")
        .or_else(|| raw.strip_prefix("\\\\"))
        .or_else(|| raw.strip_prefix("/\\"))
        .or_else(|| raw.strip_prefix("\\/"))
    {
        (
            Some(EndpointScheme::Concrete(Cow::Borrowed("//"))),
            rest.trim_start_matches(['/', '\\']),
        )
    } else if raw.starts_with(['/', '\\']) && !is_relative_path(raw) {
        // `HttpRequest::prepare_for_send` and `websocket_url` prepend a special
        // scheme, so a leading `/` or `\` before `user@host` (or `user:pass`
        // while typing before `@`) is also parsed as authority by WHATWG `Url`.
        (None, raw.trim_start_matches(['/', '\\']))
    } else {
        (None, raw)
    };

    let suffix_start = find_query_or_fragment(rest);
    let host_and_path = &rest[..suffix_start];
    let suffix = &rest[suffix_start..];

    let path_start = find_outside_templates(host_and_path, |byte| matches!(byte, b'/' | b'\\'))
        .unwrap_or(host_and_path.len());
    let authority = &host_and_path[..path_start];
    let path = &host_and_path[path_start..];

    let (had_userinfo, host) = match rfind_outside_templates(authority, b'@') {
        Some(at) => (true, authority[at + 1..].trim()),
        None => (false, authority.trim()),
    };

    if (scheme.is_some() || had_userinfo) && host.is_empty() {
        return None;
    }

    if !host.is_empty() && !is_valid_host(host) {
        return None;
    }

    let ambiguous_authority = if scheme.is_none() {
        has_complete_template(authority)
    } else {
        is_only_templates(authority)
    };
    if ambiguous_authority && path_could_contain_credentials(path) {
        return None;
    }

    if host.is_empty() && path.trim_end_matches(['/', '\\']).is_empty() {
        return None;
    }

    Some(ParsedEndpoint {
        scheme,
        host,
        path,
        suffix,
    })
}

/// Split a gRPC server URL using `grpc::transport::Target::parse`'s address
/// rules: only `://` separates a scheme, explicit ports stay intact, and
/// authority credentials, invalid schemes or malformed authorities are never
/// exposed.
fn parse_grpc_endpoint(raw: &str) -> Option<ParsedEndpoint<'_>> {
    if raw.is_empty() {
        return None;
    }

    let first_delim =
        find_outside_templates(raw, |byte| matches!(byte, b'/' | b'\\' | b'?' | b'#'))
            .unwrap_or(raw.len());
    let (scheme, rest) = if let Some(sep) = find_str_outside_templates(raw, "://")
        && sep < first_delim
    {
        let name = &raw[..sep];
        let after = &raw[sep + 3..];
        let prefix = &raw[..sep + 3];
        if is_scheme(name) {
            (Some(EndpointScheme::Concrete(Cow::Borrowed(prefix))), after)
        } else if is_template_scheme(name) {
            (Some(EndpointScheme::Template(prefix)), after)
        } else {
            return None;
        }
    } else {
        (None, raw)
    };

    let suffix_start = find_query_or_fragment(rest);
    let host_and_path = &rest[..suffix_start];
    let suffix = &rest[suffix_start..];

    let path_start = find_outside_templates(host_and_path, |byte| matches!(byte, b'/' | b'\\'))
        .unwrap_or(host_and_path.len());
    let authority = &host_and_path[..path_start];
    let path = &host_and_path[path_start..];

    let host = match rfind_outside_templates(authority, b'@') {
        Some(at) => authority[at + 1..].trim(),
        None => authority.trim(),
    };

    if !is_valid_host(host) {
        return None;
    }

    let ambiguous_authority = if scheme.is_none() {
        has_complete_template(authority)
    } else {
        is_only_templates(authority)
    };
    if ambiguous_authority && path_could_contain_credentials(path) {
        return None;
    }

    Some(ParsedEndpoint {
        scheme,
        host,
        path,
        suffix,
    })
}

/// Whether `raw` begins with `<special_scheme>:<port>` without `://` where
/// `<port>` is empty or a bare port/template rather than a WHATWG special-scheme
/// authority with credentials (such as `http:8080/api`, `http:/api`,
/// `ws:/chat`, `ws:9000/chat`, or `http:{{ port }}/api`). Both
/// `HttpRequest::prepare_for_send` and `websocket_url` prepend a default scheme
/// when `://` is absent, so these addresses identify a host named after the
/// scheme rather than a scheme prefix.
fn is_bare_special_host_port(raw: &str) -> bool {
    let first_delim =
        find_outside_templates(raw, |byte| matches!(byte, b'/' | b'\\' | b'?' | b'#'))
            .unwrap_or(raw.len());
    let authority = &raw[..first_delim];

    let Some(colon) = find_outside_templates(authority, |byte| byte == b':') else {
        return false;
    };
    let name = &authority[..colon];
    let port = &authority[colon + 1..];
    if !is_special_scheme(name) {
        return false;
    }

    if port.is_empty() {
        let after_colon = &raw[colon + 1..];
        return !after_colon.starts_with(['/', '\\']) || is_relative_path(after_colon);
    }

    rfind_outside_templates(port, b'@').is_none()
}

/// Whether a path following an ambiguous templated authority could become
/// authority credentials (such as `alice:secret@example.com`, `token@host`, or
/// partial `alice:secret` before `@host`) if the leading template resolves to a
/// scheme prefix like `https:/` or empty text.
fn path_could_contain_credentials(path: &str) -> bool {
    let mut remaining = path;

    while !remaining.is_empty() {
        let seg_end = find_outside_templates(remaining, |byte| matches!(byte, b'/' | b'\\'))
            .unwrap_or(remaining.len());
        let segment = &remaining[..seg_end];
        remaining = remaining.get(seg_end + 1..).unwrap_or("");

        if segment.is_empty() {
            continue;
        }

        if rfind_outside_templates(segment, b'@').is_some_and(|at| at > 0) {
            return true;
        }
        if find_outside_templates(segment, |byte| byte == b':').is_some_and(|colon| colon > 0)
            && !is_valid_host(segment)
        {
            return true;
        }

        if !is_only_templates(segment) {
            return false;
        }
    }

    false
}

/// Whether `text` contains at least one closed `{{...}}` placeholder.
fn has_complete_template(text: &str) -> bool {
    let Some(start) = text.find("{{") else {
        return false;
    };
    text[start + 2..].contains("}}")
}

/// Whether `text` is non-empty and made up exclusively of complete `{{...}}`
/// placeholders (which could resolve to empty text or slashes).
fn is_only_templates(text: &str) -> bool {
    let mut remaining = text;
    let mut count = 0;

    while let Some(after_open) = remaining.strip_prefix("{{") {
        let Some(end) = after_open.find("}}") else {
            return false;
        };
        count += 1;
        remaining = &after_open[end + 2..];
    }

    count > 0 && remaining.is_empty()
}

/// Find the first ASCII byte matching `predicate` outside complete `{{...}}`
/// template placeholders.
fn find_outside_templates(text: &str, predicate: impl Fn(u8) -> bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"{{")
            && let Some(end) = text[index + 2..].find("}}")
        {
            index += end + 4;
            continue;
        }
        if predicate(bytes[index]) {
            return Some(index);
        }
        index += 1;
    }

    None
}

/// Find the last occurrence of ASCII byte `needle` outside complete `{{...}}`
/// template placeholders.
fn rfind_outside_templates(text: &str, needle: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut found = None;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"{{")
            && let Some(end) = text[index + 2..].find("}}")
        {
            index += end + 4;
            continue;
        }
        if bytes[index] == needle {
            found = Some(index);
        }
        index += 1;
    }

    found
}

/// Find the first occurrence of ASCII `needle` outside complete `{{...}}`
/// template placeholders.
fn find_str_outside_templates(text: &str, needle: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut index = 0;

    while index + needle_bytes.len() <= bytes.len() {
        if bytes[index..].starts_with(b"{{")
            && let Some(end) = text[index + 2..].find("}}")
        {
            index += end + 4;
            continue;
        }
        if bytes[index..].starts_with(needle_bytes) {
            return Some(index);
        }
        index += 1;
    }

    None
}

/// Whether `host` is a plausible `host` or `host:port` (including bracketed
/// IPv6 literals and `{{variables}}`), rather than a malformed authority such
/// as `alice:secret` typed before `@example.com`.
fn is_valid_host(host: &str) -> bool {
    if host.is_empty() || !valid_host_chars(host) {
        return false;
    }

    if let Some(rest) = host.strip_prefix('[') {
        let Some((address, after)) = rest.split_once(']') else {
            return false;
        };
        if address.is_empty() || address.contains('[') {
            return false;
        }
        return match after {
            "" => true,
            _ => after.strip_prefix(':').is_some_and(is_valid_port),
        };
    }

    if find_outside_templates(host, |byte| matches!(byte, b'[' | b']')).is_some() {
        return false;
    }

    match split_host_port(host) {
        Some((name, port)) => !name.is_empty() && is_valid_port(port),
        None => true,
    }
}

/// Whether `text` has no whitespace or C0 controls outside `{{...}}` variable
/// markers, and every closed `{{...}}` marker has a non-empty trimmed name
/// (matching `environment::VariableResolver::resolve`).
fn valid_host_chars(text: &str) -> bool {
    let mut remaining = text;

    while let Some(start) = remaining.find("{{") {
        let before = &remaining[..start];
        if before.chars().any(|ch| ch <= ' ' || ch.is_whitespace()) {
            return false;
        }

        let after_open = &remaining[start + 2..];
        let Some(end) = after_open.find("}}") else {
            return !after_open.chars().any(|ch| ch <= ' ' || ch.is_whitespace());
        };
        let inner = &after_open[..end];
        if inner.trim().is_empty() || inner.chars().any(|ch| ch < ' ') {
            return false;
        }

        remaining = &after_open[end + 2..];
    }

    !remaining.chars().any(|ch| ch <= ' ' || ch.is_whitespace())
}

/// Split `host:port` at the first `:` outside `{{...}}` template variables.
fn split_host_port(host: &str) -> Option<(&str, &str)> {
    let index = find_outside_templates(host, |byte| byte == b':')?;
    Some((&host[..index], &host[index + 1..]))
}

/// A valid port is empty (accepted as the default port by WHATWG `Url`), an
/// unsigned 16-bit integer (ASCII digits only), or a template expression
/// composed solely of ASCII digits and non-empty `{{...}}` placeholders.
fn is_valid_port(port: &str) -> bool {
    if port.is_empty() {
        return true;
    }
    if port.bytes().all(|byte| byte.is_ascii_digit()) {
        return port.parse::<u16>().is_ok();
    }

    let mut remaining = port;
    let mut has_template = false;
    while let Some(start) = remaining.find("{{") {
        if !remaining[..start].bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        let after_open = &remaining[start + 2..];
        let Some(end) = after_open.find("}}") else {
            return false;
        };
        if after_open[..end].trim().is_empty() {
            return false;
        }
        has_template = true;
        remaining = &after_open[end + 2..];
    }

    has_template && remaining.bytes().all(|byte| byte.is_ascii_digit())
}

/// WHATWG special schemes whose authority follows `:` with any number of `/` or `\`.
fn is_special_scheme(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("ws")
        || scheme.eq_ignore_ascii_case("wss")
        || scheme.eq_ignore_ascii_case("ftp")
}

/// RFC 3986 scheme syntax: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();

    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// A scheme prefix containing at least one `{{variable}}` placeholder, with
/// only valid RFC 3986 scheme characters outside placeholders.
fn is_template_scheme(scheme: &str) -> bool {
    let is_scheme_char =
        |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.');

    let mut remaining = scheme;
    let mut at_start = true;
    let mut has_template = false;

    while let Some(start) = remaining.find("{{") {
        let before = &remaining[..start];
        if !before.is_empty() {
            if at_start && !before.as_bytes()[0].is_ascii_alphabetic() {
                return false;
            }
            if !before.bytes().all(is_scheme_char) {
                return false;
            }
        }

        let after_open = &remaining[start + 2..];
        let Some(end) = after_open.find("}}") else {
            return false;
        };
        let inner = &after_open[..end];
        if inner.trim().is_empty()
            || inner.starts_with('!')
            || inner.contains(['{', '}'])
            || inner.chars().any(|ch| ch < ' ')
        {
            return false;
        }

        has_template = true;
        at_start = false;
        remaining = &after_open[end + 2..];
    }

    has_template && remaining.bytes().all(is_scheme_char)
}
