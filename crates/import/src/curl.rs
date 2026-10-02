//! cURL commands, as browsers' "Copy as cURL" and API documentation write
//! them for a POSIX shell. A command becomes the request cURL would send.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use request::{HttpRequest, HttpSettings, Method};
use thiserror::Error;

use crate::body::{BOUNDARY, multipart_form};

/// Characters `--data-urlencode` leaves as they are.
const URL_ENCODED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Short options that take a value. Others are flags, which can be combined,
/// as in `-sSL`.
const SHORT_WITH_VALUE: &str = "AbcCdDeEFHKmoPQrTtuUwXxyYz";

/// Long options that take a value. Unknown options are taken to be flags.
const LONG_WITH_VALUE: &[&str] = &[
    "abstract-unix-socket",
    "alt-svc",
    "aws-sigv4",
    "cacert",
    "capath",
    "cert",
    "cert-type",
    "ciphers",
    "config",
    "connect-timeout",
    "connect-to",
    "continue-at",
    "cookie",
    "cookie-jar",
    "create-file-mode",
    "crlfile",
    "curves",
    "data",
    "data-ascii",
    "data-binary",
    "data-raw",
    "data-urlencode",
    "delegation",
    "dns-interface",
    "dns-ipv4-addr",
    "dns-ipv6-addr",
    "dns-servers",
    "doh-url",
    "dump-header",
    "ech",
    "egd-file",
    "engine",
    "etag-compare",
    "etag-save",
    "expect100-timeout",
    "form",
    "form-string",
    "ftp-account",
    "ftp-alternative-to-user",
    "ftp-method",
    "ftp-port",
    "ftp-ssl-ccc-mode",
    "happy-eyeballs-timeout-ms",
    "haproxy-clientip",
    "header",
    "hostpubmd5",
    "hostpubsha256",
    "hsts",
    "interface",
    "ip-tos",
    "ipfs-gateway",
    "json",
    "keepalive-cnt",
    "keepalive-time",
    "key",
    "key-type",
    "krb",
    "libcurl",
    "limit-rate",
    "local-port",
    "login-options",
    "mail-auth",
    "mail-from",
    "mail-rcpt",
    "max-filesize",
    "max-redirs",
    "max-time",
    "netrc-file",
    "noproxy",
    "oauth2-bearer",
    "output",
    "output-dir",
    "parallel-max",
    "pass",
    "pinnedpubkey",
    "preproxy",
    "proto",
    "proto-default",
    "proto-redir",
    "proxy",
    "proxy-cacert",
    "proxy-capath",
    "proxy-cert",
    "proxy-cert-type",
    "proxy-ciphers",
    "proxy-crlfile",
    "proxy-header",
    "proxy-key",
    "proxy-key-type",
    "proxy-pass",
    "proxy-pinnedpubkey",
    "proxy-service-name",
    "proxy-tls13-ciphers",
    "proxy-tlsauthtype",
    "proxy-tlspassword",
    "proxy-tlsuser",
    "proxy-user",
    "proxy1.0",
    "pubkey",
    "quote",
    "random-file",
    "range",
    "rate",
    "referer",
    "request",
    "request-target",
    "resolve",
    "retry",
    "retry-delay",
    "retry-max-time",
    "sasl-authzid",
    "service-name",
    "socks4",
    "socks4a",
    "socks5",
    "socks5-gssapi-service",
    "socks5-hostname",
    "speed-limit",
    "speed-time",
    "stderr",
    "telnet-option",
    "tftp-blksize",
    "time-cond",
    "tls-max",
    "tls13-ciphers",
    "tlsauthtype",
    "tlspassword",
    "tlsuser",
    "trace",
    "trace-ascii",
    "trace-config",
    "unix-socket",
    "upload-file",
    "url",
    "url-query",
    "user",
    "user-agent",
    "variable",
    "write-out",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CurlError {
    #[error("The text is not a cURL command.")]
    NotCurl,
    #[error("The cURL command has an unclosed quote.")]
    UnclosedQuote,
    #[error("The cURL option {0} needs a value.")]
    MissingValue(String),
    #[error("The cURL command has no URL.")]
    MissingUrl,
    #[error("Request Eagle cannot send {0} requests.")]
    UnsupportedMethod(String),
    #[error("Request Eagle cannot send files, so it cannot import {0}.")]
    File(String),
    #[error("The cURL command sends both form fields and data. cURL accepts only one of them.")]
    FormAndData,
    #[error("Request Eagle cannot send a body with {0} requests.")]
    BodyWithoutMethod(&'static str),
}

/// Whether the text is a cURL command rather than a URL, such as text pasted
/// into the address bar.
pub fn is_curl(text: &str) -> bool {
    // A backslash can continue the command on the next line right away.
    text.trim_start().strip_prefix("curl").is_some_and(|rest| {
        rest.strip_prefix('\\')
            .unwrap_or(rest)
            .starts_with(char::is_whitespace)
    })
}

/// The request a cURL command sends. `--insecure` and `--max-time` become
/// its settings. Other options that only change how cURL connects or prints,
/// such as `--location` or `--silent`, are left out.
pub fn parse_curl(command: &str) -> Result<HttpRequest, CurlError> {
    if !is_curl(command) {
        return Err(CurlError::NotCurl);
    }

    let words = words(command)?;
    let mut options = Options::default();
    let mut arguments = words.into_iter().skip(1);

    while let Some(word) = arguments.next() {
        if let Some(option) = word.strip_prefix("--").filter(|option| !option.is_empty()) {
            // The value can follow in the next word or after `=`: `--request=PUT`.
            let (name, value) = match option.split_once('=') {
                Some((name, value)) if LONG_WITH_VALUE.contains(&name) => {
                    (name, Some(value.to_owned()))
                }
                _ if LONG_WITH_VALUE.contains(&option) => (
                    option,
                    Some(
                        arguments
                            .next()
                            .ok_or_else(|| CurlError::MissingValue(word.clone()))?,
                    ),
                ),
                _ => (option, None),
            };
            options.apply(name, value)?;
        } else if let Some(flags) = word.strip_prefix('-').filter(|flags| !flags.is_empty()) {
            // Flags can be combined, and the last one can take its value from
            // the rest of the word or from the next word: `-sSXPOST`.
            for (index, flag) in flags.char_indices() {
                if SHORT_WITH_VALUE.contains(flag) {
                    let rest = &flags[index + flag.len_utf8()..];
                    let value = if rest.is_empty() {
                        arguments
                            .next()
                            .ok_or_else(|| CurlError::MissingValue(format!("-{flag}")))?
                    } else {
                        rest.to_owned()
                    };
                    options.apply(long_name(flag), Some(value))?;
                    break;
                }

                options.apply(long_name(flag), None)?;
            }
        } else if options.url.is_none() {
            options.url = Some(word);
        }
    }

    options.into_request()
}

/// The long name of a short option that changes the request.
fn long_name(flag: char) -> &'static str {
    match flag {
        'A' => "user-agent",
        'b' => "cookie",
        'd' => "data",
        'e' => "referer",
        'F' => "form",
        'G' => "get",
        'H' => "header",
        'I' => "head",
        'k' => "insecure",
        'm' => "max-time",
        'T' => "upload-file",
        'u' => "user",
        'X' => "request",
        _ => "",
    }
}

#[derive(Default)]
struct Options {
    url: Option<String>,
    method: Option<String>,
    head: bool,
    get: bool,
    headers: Vec<(String, String)>,
    /// Lowercase names of headers removed with `-H 'Name:'`.
    removed: Vec<String>,
    data: Option<String>,
    json: bool,
    form: Vec<(String, String)>,
    user: Option<String>,
    cookies: Vec<String>,
    insecure: bool,
    timeout_ms: Option<u64>,
}

impl Options {
    fn apply(&mut self, name: &str, value: Option<String>) -> Result<(), CurlError> {
        let Some(value) = value else {
            match name {
                "get" => self.get = true,
                "head" => self.head = true,
                "insecure" => self.insecure = true,
                _ => {}
            }
            return Ok(());
        };

        match name {
            "url" => self.url = Some(value),
            "request" => self.method = Some(value),
            "header" => {
                if value.starts_with('@') {
                    return Err(CurlError::File(format!("--header {value}")));
                }
                self.header(&value);
            }
            "user-agent" => self.headers.push(("User-Agent".into(), value)),
            "referer" => {
                let referer = value.strip_suffix(";auto").unwrap_or(&value);
                if !referer.is_empty() {
                    self.headers.push(("Referer".into(), referer.to_owned()));
                }
            }
            // Without `=`, the value names a cookie file to read.
            "cookie" if value.contains('=') => self.cookies.push(value),
            "user" => self.user = Some(value),
            "oauth2-bearer" => self
                .headers
                .push(("Authorization".into(), format!("Bearer {value}"))),
            "data" | "data-ascii" | "data-binary" | "json" => {
                if value.starts_with('@') {
                    return Err(CurlError::File(format!("--{name} {value}")));
                }
                self.json |= name == "json";
                self.add_data(&value, name != "json");
            }
            "data-raw" => self.add_data(&value, true),
            "data-urlencode" => self.add_data(&url_encode_data(&value)?, true),
            "form" => {
                let (field, content) = value.split_once('=').unwrap_or((&value, ""));
                if content.starts_with(['@', '<']) {
                    return Err(CurlError::File(format!("--form {value}")));
                }
                self.form.push((field.to_owned(), form_value(content)));
            }
            "form-string" => {
                let (field, content) = value.split_once('=').unwrap_or((&value, ""));
                self.form.push((field.to_owned(), content.to_owned()));
            }
            "upload-file" => return Err(CurlError::File(format!("--upload-file {value}"))),
            // In seconds, which may have a fraction. Zero is no limit, as in
            // Request Eagle.
            "max-time" => {
                self.timeout_ms = value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|seconds| seconds.is_finite() && *seconds >= 0.)
                    .map(|seconds| (seconds * 1000.).round() as u64);
            }
            _ => {}
        }

        Ok(())
    }

    /// Data from `--json` continues the previous data. Other data options
    /// separate their data with `&`.
    fn add_data(&mut self, data: &str, separated: bool) {
        match &mut self.data {
            Some(previous) => {
                if separated {
                    previous.push('&');
                }
                previous.push_str(data);
            }
            None => self.data = Some(data.to_owned()),
        }
    }

    /// A header as cURL reads it: `Name: value`, `Name;` for an empty value,
    /// or `Name:` to remove a header cURL would add.
    fn header(&mut self, header: &str) {
        if let Some((name, value)) = header.split_once(':') {
            let (name, value) = (name.trim(), value.trim());
            if value.is_empty() {
                self.removed.push(name.to_ascii_lowercase());
            } else {
                self.headers.push((name.to_owned(), value.to_owned()));
            }
        } else if let Some(name) = header.trim().strip_suffix(';') {
            self.headers.push((name.trim().to_owned(), String::new()));
        }
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(header, _)| header.eq_ignore_ascii_case(name))
    }

    /// Adds a header cURL would add, unless the command sets or removes it.
    fn add_default(&mut self, name: &str, value: &str) {
        if !self.has_header(name) && !self.removed.contains(&name.to_ascii_lowercase()) {
            self.headers.push((name.to_owned(), value.to_owned()));
        }
    }

    fn into_request(mut self) -> Result<HttpRequest, CurlError> {
        let mut url = self.url.take().ok_or(CurlError::MissingUrl)?;
        if !url.contains("://") && !url.starts_with("{{") {
            // cURL uses HTTP when the URL names no scheme.
            url = format!("http://{url}");
        }

        if !self.form.is_empty() && self.data.is_some() {
            return Err(CurlError::FormAndData);
        }

        if let Some(user) = self.user.take()
            && !self.has_header("authorization")
        {
            let credentials = if user.contains(':') {
                user
            } else {
                format!("{user}:")
            };
            self.headers.push((
                "Authorization".into(),
                format!("Basic {}", STANDARD.encode(credentials)),
            ));
        }

        if !self.cookies.is_empty() {
            self.headers
                .push(("Cookie".into(), self.cookies.join("; ")));
        }

        let mut body = None;

        if self.get {
            // `--get` sends the data in the query instead of the body.
            if let Some(data) = self.data.take() {
                let fragment = url.find('#').map(|start| url.split_off(start));
                let separator = if url.contains('?') { '&' } else { '?' };
                url = format!("{url}{separator}{data}{}", fragment.unwrap_or_default());
            }
        } else if let Some(data) = self.data.take() {
            if self.json {
                self.add_default("Content-Type", "application/json");
                self.add_default("Accept", "application/json");
            } else {
                self.add_default("Content-Type", "application/x-www-form-urlencoded");
            }
            body = Some(data.into_bytes());
        } else if !self.form.is_empty() {
            // cURL adds the boundary to a multipart type written without one.
            for (name, value) in &mut self.headers {
                if name.eq_ignore_ascii_case("content-type")
                    && value.to_ascii_lowercase().starts_with("multipart/")
                    && !value.contains("boundary=")
                {
                    *value = format!("{value}; boundary={BOUNDARY}");
                }
            }
            body = Some(multipart_form(&self.form, &mut self.headers));
        }

        let method = match self.method.as_deref() {
            Some(method) => method_named(method)?,
            None if self.head => Method::Head,
            None if body.is_some() => Method::Post,
            None => Method::Get,
        };

        // Request Eagle leaves out the body of GET and HEAD requests.
        if body.is_some() && matches!(method, Method::Get | Method::Head) {
            return Err(CurlError::BodyWithoutMethod(method.as_str()));
        }

        Ok(HttpRequest {
            method,
            path: url,
            headers: self.headers,
            body,
            settings: HttpSettings {
                timeout_ms: self.timeout_ms,
                follow_redirects: None,
                verify_certificates: self.insecure.then_some(false),
            },
            ..HttpRequest::default()
        })
    }
}

fn method_named(method: &str) -> Result<Method, CurlError> {
    Ok(match method.to_ascii_uppercase().as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => return Err(CurlError::UnsupportedMethod(method.to_owned())),
    })
}

/// The text of a `--form` field. cURL ends an unquoted value at `;`, where
/// attributes such as `;type=text/plain` follow. A quoted value can contain
/// `;`, and `\"` and `\\` in it stand for `"` and `\`. Without its closing
/// quote, the value is taken as it is written.
fn form_value(content: &str) -> String {
    let unquoted = || content.split(';').next().unwrap_or_default().to_owned();
    let Some(quoted) = content.strip_prefix('"') else {
        return unquoted();
    };

    let mut value = String::new();
    let mut chars = quoted.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            '"' => return value,
            '\\' => value.push(
                chars
                    .next_if(|next| matches!(next, '"' | '\\'))
                    .unwrap_or('\\'),
            ),
            char => value.push(char),
        }
    }

    unquoted()
}

/// `--data-urlencode` encodes `content`, the content of `=content`, or the
/// content of `name=content`. `@file` and `name@file` read a file.
fn url_encode_data(value: &str) -> Result<String, CurlError> {
    if let Some((name, content)) = value.split_once('=') {
        let content = utf8_percent_encode(content, URL_ENCODED).to_string();
        return Ok(if name.is_empty() {
            content
        } else {
            format!("{name}={content}")
        });
    }

    if value.contains('@') {
        return Err(CurlError::File(format!("--data-urlencode {value}")));
    }

    Ok(utf8_percent_encode(value, URL_ENCODED).to_string())
}

/// Splits a command into words as a POSIX shell does, with its quotes and
/// escapes, including `$'…'`. A backslash at the end of a line continues the
/// command on the next line. A word that starts with `|`, `&`, `;`, `<` or
/// `>` ends the command, as in `curl … | jq`. Inside a word they are kept, so
/// an unquoted URL keeps its query.
fn words(command: &str) -> Result<Vec<String>, CurlError> {
    let mut words = Vec::new();
    let mut word = String::new();
    // Distinguishes an empty quoted word, `''`, from no word.
    let mut in_word = false;
    let mut chars = command.chars().peekable();

    while let Some(char) = chars.next() {
        match char {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') => {
                    chars.next_if_eq(&'\n');
                }
                Some(escaped) => {
                    word.push(escaped);
                    in_word = true;
                }
                None => {}
            },
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(char) => word.push(char),
                        None => return Err(CurlError::UnclosedQuote),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('\n') => {}
                            Some(escaped @ ('$' | '`' | '"' | '\\')) => word.push(escaped),
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err(CurlError::UnclosedQuote),
                        },
                        Some(char) => word.push(char),
                        None => return Err(CurlError::UnclosedQuote),
                    }
                }
            }
            '|' | '&' | ';' | '<' | '>' if !in_word => break,
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                ansi_c_quoted(&mut chars, &mut word)?;
            }
            char => {
                word.push(char);
                in_word = true;
            }
        }
    }

    if in_word {
        words.push(word);
    }

    Ok(words)
}

/// The rest of a `$'…'` word, whose escapes are those of C strings. `\x` and
/// octal escapes write bytes, which together can form UTF-8 characters.
fn ansi_c_quoted(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    word: &mut String,
) -> Result<(), CurlError> {
    let mut bytes = Vec::new();
    let push = |bytes: &mut Vec<u8>, char: char| {
        bytes.extend_from_slice(char.encode_utf8(&mut [0; 4]).as_bytes());
    };

    loop {
        match chars.next().ok_or(CurlError::UnclosedQuote)? {
            '\'' => break,
            '\\' => {
                let escaped = chars.next().ok_or(CurlError::UnclosedQuote)?;
                match escaped {
                    'n' => bytes.push(b'\n'),
                    't' => bytes.push(b'\t'),
                    'r' => bytes.push(b'\r'),
                    'a' => bytes.push(0x07),
                    'b' => bytes.push(0x08),
                    'e' | 'E' => bytes.push(0x1b),
                    'f' => bytes.push(0x0c),
                    'v' => bytes.push(0x0b),
                    'x' => match code(chars, 16, 2) {
                        Some(byte) => bytes.push(byte as u8),
                        None => bytes.extend_from_slice(b"\\x"),
                    },
                    'u' | 'U' => {
                        let digits = if escaped == 'u' { 4 } else { 8 };
                        match code(chars, 16, digits) {
                            Some(code) => {
                                push(&mut bytes, char::from_u32(code).unwrap_or('\u{fffd}'))
                            }
                            None => {
                                bytes.push(b'\\');
                                push(&mut bytes, escaped);
                            }
                        }
                    }
                    '0'..='7' => {
                        let mut byte = escaped.to_digit(8).unwrap_or_default();
                        for _ in 0..2 {
                            let Some(digit) = chars.peek().and_then(|char| char.to_digit(8)) else {
                                break;
                            };
                            chars.next();
                            byte = byte * 8 + digit;
                        }
                        bytes.push(byte as u8);
                    }
                    '\\' | '\'' | '"' | '?' => push(&mut bytes, escaped),
                    other => {
                        bytes.push(b'\\');
                        push(&mut bytes, other);
                    }
                }
            }
            char => push(&mut bytes, char),
        }
    }

    word.push_str(&String::from_utf8_lossy(&bytes));

    Ok(())
}

/// Reads up to `digits` digits in `radix`, or nothing when the next
/// character is not one.
fn code(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    radix: u32,
    digits: usize,
) -> Option<u32> {
    let mut code = None;

    for _ in 0..digits {
        let Some(digit) = chars.peek().and_then(|char| char.to_digit(radix)) else {
            break;
        };
        chars.next();
        code = Some(code.unwrap_or(0) * radix + digit);
    }

    code
}
