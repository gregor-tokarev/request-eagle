use base64::{Engine as _, engine::general_purpose::STANDARD};
use indexmap::IndexMap;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use regex::Regex;

use super::registry::{Args, Builtin, stringify};
use crate::error::{Error, Result};
use crate::value::{Frame, Value};

pub(super) static FUNCTIONS: &[Builtin] = &[
    builtin(
        "string",
        "$string(arg[, prettify])",
        "Casts a value to a string; other values become JSON",
        1,
        2,
        true,
        string,
    ),
    builtin(
        "length",
        "$length(str)",
        "The number of characters in a string",
        1,
        1,
        true,
        length,
    ),
    builtin(
        "substring",
        "$substring(str, start[, length])",
        "Characters from start, which counts from the end when negative",
        2,
        3,
        true,
        substring,
    ),
    builtin(
        "substringBefore",
        "$substringBefore(str, chars)",
        "The part before the first occurrence of chars",
        2,
        2,
        true,
        substring_before,
    ),
    builtin(
        "substringAfter",
        "$substringAfter(str, chars)",
        "The part after the first occurrence of chars",
        2,
        2,
        true,
        substring_after,
    ),
    builtin(
        "uppercase",
        "$uppercase(str)",
        "The string in upper case",
        1,
        1,
        true,
        uppercase,
    ),
    builtin(
        "lowercase",
        "$lowercase(str)",
        "The string in lower case",
        1,
        1,
        true,
        lowercase,
    ),
    builtin(
        "trim",
        "$trim(str)",
        "Removes surrounding whitespace and collapses the rest to single spaces",
        1,
        1,
        true,
        trim,
    ),
    builtin(
        "pad",
        "$pad(str, width[, char])",
        "Pads to width on the right, or on the left when width is negative",
        2,
        3,
        true,
        pad,
    ),
    builtin(
        "contains",
        "$contains(str, pattern)",
        "Whether the string contains the text or matches the regular expression",
        2,
        2,
        true,
        contains,
    ),
    builtin(
        "split",
        "$split(str, separator[, limit])",
        "Splits a string by text or a regular expression",
        2,
        3,
        true,
        split,
    ),
    builtin(
        "join",
        "$join(array[, separator])",
        "Joins an array of strings",
        1,
        2,
        false,
        join,
    ),
    builtin(
        "match",
        "$match(str, pattern[, limit])",
        "Matches of a regular expression, with their index and groups",
        2,
        3,
        true,
        match_,
    ),
    builtin(
        "replace",
        "$replace(str, pattern, replacement[, limit])",
        "Replaces text or regular expression matches; $1 refers to a group",
        3,
        4,
        true,
        replace,
    ),
    builtin(
        "eval",
        "$eval(expr[, context])",
        "Evaluates an expression written in a string",
        1,
        2,
        false,
        eval,
    ),
    builtin(
        "base64encode",
        "$base64encode(str)",
        "Encodes UTF-8 text as Base64",
        1,
        1,
        true,
        base64_encode,
    ),
    builtin(
        "base64decode",
        "$base64decode(str)",
        "Decodes Base64 to UTF-8 text",
        1,
        1,
        true,
        base64_decode,
    ),
    builtin(
        "encodeUrlComponent",
        "$encodeUrlComponent(str)",
        "Percent-encodes a URL component",
        1,
        1,
        true,
        encode_url_component,
    ),
    builtin(
        "encodeUrl",
        "$encodeUrl(str)",
        "Percent-encodes a URL, keeping its reserved characters",
        1,
        1,
        true,
        encode_url,
    ),
    builtin(
        "decodeUrlComponent",
        "$decodeUrlComponent(str)",
        "Decodes a percent-encoded URL component",
        1,
        1,
        true,
        decode_url_component,
    ),
    builtin(
        "decodeUrl",
        "$decodeUrl(str)",
        "Decodes a percent-encoded URL",
        1,
        1,
        true,
        decode_url,
    ),
];

#[allow(clippy::too_many_arguments)]
pub(super) const fn builtin(
    name: &'static str,
    signature: &'static str,
    description: &'static str,
    min: usize,
    max: usize,
    context: bool,
    implementation: for<'a, 'e> fn(&Args<'a, 'e>) -> Result<Value<'a>>,
) -> Builtin {
    Builtin {
        name,
        signature,
        description,
        min,
        max,
        context,
        implementation,
    }
}

fn string<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let value = args.get(0);
    if value.is_undefined() {
        return Ok(Value::Undefined);
    }
    let pretty = args.boolean(1)?.unwrap_or(false);

    stringify(&value, pretty)
        .map(Value::string)
        .map_err(|error| error.or_at(args.position))
}

fn length<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.string(0)? {
        Some(text) => Value::Number(text.chars().count() as f64),
        None => Value::Undefined,
    })
}

fn substring<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let start = args.number(1)?.ok_or_else(|| args.mismatch(1))?;
    let length = args.number(2)?;
    let chars: Vec<char> = text.chars().collect();
    let count = chars.len() as i64;

    // As JavaScript's slice, which counts negative positions from the end.
    let clamp = |position: f64| -> usize {
        let position = position.trunc() as i64;
        let position = if position < 0 {
            count + position
        } else {
            position
        };
        position.clamp(0, count) as usize
    };

    let start_number = if count as f64 + start < 0. { 0. } else { start };
    let (from, to) = match length {
        Some(length) if length <= 0. => return Ok(Value::string("")),
        Some(length) => {
            let end = if start_number >= 0. {
                start_number + length
            } else {
                count as f64 + start_number + length
            };
            (clamp(start_number), clamp(end))
        }
        None => (clamp(start_number), count as usize),
    };

    Ok(Value::string(
        chars
            .get(from..to.max(from))
            .unwrap_or_default()
            .iter()
            .collect::<String>(),
    ))
}

fn substring_before<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let chars = args.string(1)?.ok_or_else(|| args.mismatch(1))?;

    Ok(Value::string(match text.find(chars) {
        Some(index) => &text[..index],
        None => text,
    }))
}

fn substring_after<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let chars = args.string(1)?.ok_or_else(|| args.mismatch(1))?;

    Ok(Value::string(match text.find(chars) {
        Some(index) => &text[index + chars.len()..],
        None => text,
    }))
}

fn uppercase<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args
        .string(0)?
        .map_or(Value::Undefined, |text| Value::string(text.to_uppercase())))
}

fn lowercase<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args
        .string(0)?
        .map_or(Value::Undefined, |text| Value::string(text.to_lowercase())))
}

fn trim<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args.string(0)?.map_or(Value::Undefined, |text| {
        Value::string(
            text.split([' ', '\t', '\n', '\r'])
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        )
    }))
}

fn pad<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let width = args.number(1)?.ok_or_else(|| args.mismatch(1))?;
    let padding = match args.string(2)? {
        Some("") | None => " ",
        Some(padding) => padding,
    };

    let length = text.chars().count() as i64;
    let missing = (width.abs().trunc() as i64 - length).max(0) as usize;
    let fill: String = padding.chars().cycle().take(missing).collect();

    Ok(Value::string(if width < 0. {
        format!("{fill}{text}")
    } else {
        format!("{text}{fill}")
    }))
}

/// A string or regular expression argument.
enum Pattern<'r> {
    Text(String),
    Regex(&'r Regex),
}

fn pattern<'r>(args: &'r Args<'_, '_>, index: usize) -> Result<Pattern<'r>> {
    match args.values.get(index) {
        Some(Value::String(text)) => Ok(Pattern::Text(text.as_str().to_owned())),
        Some(Value::Regex(regex)) => Ok(Pattern::Regex(regex)),
        _ => Err(args.mismatch(index)),
    }
}

fn contains<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };

    Ok(Value::Bool(match pattern(args, 1)? {
        Pattern::Text(token) => text.contains(&token),
        Pattern::Regex(regex) => regex.is_match(text),
    }))
}

fn limit(args: &Args, index: usize, code: &'static str) -> Result<usize> {
    match args.number(index)? {
        Some(limit) if limit < 0. => Err(args.error(
            code,
            format!(
                "Third argument of {} function must evaluate to a positive number",
                args.name
            ),
        )),
        Some(limit) => Ok(limit as usize),
        None => Ok(usize::MAX),
    }
}

fn split<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let limit = limit(args, 2, "D3020")?;

    let parts: Vec<String> = match pattern(args, 1)? {
        Pattern::Text(separator) if separator.is_empty() => {
            text.chars().map(String::from).collect()
        }
        Pattern::Text(separator) => text.split(separator.as_str()).map(str::to_owned).collect(),
        Pattern::Regex(regex) => regex.split(text).map(str::to_owned).collect(),
    };

    Ok(Value::array(
        parts.into_iter().take(limit).map(Value::string).collect(),
    ))
}

fn join<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    let separator = args.string(1)?.unwrap_or_default();

    let texts = items
        .iter()
        .map(|item| match item {
            Value::String(text) => Ok(text.as_str().to_owned()),
            _ => Err(args.error(
                "T0412",
                "Argument 1 of function \"join\" must be an array of strings",
            )),
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Value::string(texts.join(separator)))
}

/// A match object: the text, its character index and its groups.
fn match_object<'a>(text: &str, captures: &regex::Captures) -> Value<'a> {
    let whole = captures.get(0).expect("a match has its whole text");
    let groups = captures
        .iter()
        .skip(1)
        .map(|group| Value::string(group.map_or("", |group| group.as_str())))
        .collect();

    let mut object = IndexMap::new();
    object.insert("match".to_owned(), Value::string(whole.as_str()));
    object.insert(
        "index".to_owned(),
        Value::Number(text[..whole.start()].chars().count() as f64),
    );
    object.insert("groups".to_owned(), Value::array(groups));
    Value::object(object)
}

fn match_<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let Pattern::Regex(regex) = pattern(args, 1)? else {
        return Err(args.mismatch(1));
    };
    let limit = limit(args, 2, "D3040")?;

    let matches = regex
        .captures_iter(text)
        .take(limit)
        .map(|captures| match_object(text, &captures))
        .collect();

    Ok(Value::sequence(matches))
}

/// When a regular expression is called like a function: its first match.
pub(crate) fn regex_match<'a>(
    regex: &Regex,
    argument: Option<&Value<'a>>,
    position: usize,
) -> Result<Value<'a>> {
    let Some(Value::String(text)) = argument else {
        return match argument {
            None | Some(Value::Undefined) => Ok(Value::Undefined),
            Some(_) => Err(Error::at(
                "T0410",
                position,
                "A regular expression matches strings",
            )),
        };
    };

    Ok(regex
        .captures(text.as_str())
        .map_or(Value::Undefined, |captures| {
            match_object(text.as_str(), &captures)
        }))
}

fn replace<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let pattern = pattern(args, 1)?;
    let replacement = args.get(2);
    let limit = limit(args, 3, "D3011")?;

    if let Pattern::Text(token) = &pattern
        && token.is_empty()
    {
        return Err(args.error(
            "D3010",
            "Second argument of replace function cannot be an empty string",
        ));
    }

    match pattern {
        Pattern::Text(token) => {
            let Value::String(replacement) = &replacement else {
                return Err(args.mismatch(2));
            };
            let mut result = String::new();
            let mut rest = text;
            let mut count = 0;
            while count < limit
                && let Some(index) = rest.find(&token)
            {
                result.push_str(&rest[..index]);
                result.push_str(replacement.as_str());
                rest = &rest[index + token.len()..];
                count += 1;
            }
            result.push_str(rest);
            Ok(Value::string(result))
        }
        Pattern::Regex(regex) => {
            let mut result = String::new();
            let mut last = 0;
            for captures in regex.captures_iter(text).take(limit) {
                let whole = captures.get(0).expect("a match has its whole text");
                if whole.as_str().is_empty() {
                    return Err(
                        args.error("D1004", "Regular expression matches zero length string")
                    );
                }
                result.push_str(&text[last..whole.start()]);
                match &replacement {
                    Value::String(template) => expand(template.as_str(), &captures, &mut result),
                    function if function.is_function() => {
                        match args.apply(function, vec![match_object(text, &captures)])? {
                            Value::String(text) => result.push_str(text.as_str()),
                            _ => {
                                return Err(args.error(
                                    "D3012",
                                    "Attempted to replace a matched string with a non-string value",
                                ));
                            }
                        }
                    }
                    _ => return Err(args.mismatch(2)),
                }
                last = whole.end();
            }
            result.push_str(&text[last..]);
            Ok(Value::string(result))
        }
    }
}

/// A replacement with `$0`… replaced by groups and `$$` by `$`. A number
/// with more digits than groups uses fewer digits.
fn expand(template: &str, captures: &regex::Captures, result: &mut String) {
    let mut chars = template.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }

        match chars.peek() {
            Some((_, '$')) => {
                chars.next();
                result.push('$');
            }
            Some((start, digit)) if digit.is_ascii_digit() => {
                let start = *start;
                let digits: String = template[start..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                // The longest prefix of the digits that names a group.
                let mut used = digits.len();
                while used > 1
                    && digits[..used]
                        .parse::<usize>()
                        .map_or(true, |group| group >= captures.len())
                {
                    used -= 1;
                }
                let group: usize = digits[..used].parse().unwrap_or(0);
                if group < captures.len() {
                    result.push_str(captures.get(group).map_or("", |group| group.as_str()));
                }
                for _ in 0..used {
                    chars.next();
                }
            }
            _ => result.push('$'),
        }
    }
}

fn eval<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(source) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let expression = args.evaluation.parse(source).map_err(|error| {
        args.error(
            "D3120",
            format!(
                "Syntax error in expression passed to function eval: {}",
                error.message
            ),
        )
    })?;

    // The expression sees the variables where `$eval` is called.
    let input = match args.get(1) {
        Value::Undefined if args.len() < 2 => args.input.clone(),
        value => Value::context(value),
    };
    let scope = args.evaluation.scope().unwrap_or_else(Frame::root);
    args.evaluation
        .evaluate(expression, &input, &scope)
        .map_err(|error| {
            args.error(
                "D3121",
                format!(
                    "Dynamic error evaluating the expression passed to function eval: {}",
                    error.message
                ),
            )
        })
}

/// A value that owns its data, built from JSON.
pub(crate) fn owned<'a>(json: &serde_json::Value) -> Value<'a> {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(*value),
        serde_json::Value::Number(number) => Value::Number(number.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(text) => Value::string(text.as_str()),
        serde_json::Value::Array(items) => Value::array(items.iter().map(owned).collect()),
        serde_json::Value::Object(object) => Value::object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), owned(value)))
                .collect(),
        ),
    }
}

fn base64_encode<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args.string(0)?.map_or(Value::Undefined, |text| {
        Value::string(STANDARD.encode(text))
    }))
}

fn base64_decode<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let bytes = STANDARD
        .decode(text.trim())
        .map_err(|_| args.error("D3137", "The argument of $base64decode is not Base64"))?;

    Ok(Value::string(String::from_utf8_lossy(&bytes).into_owned()))
}

/// What encodeURIComponent leaves as it is.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// What encodeURI leaves as it is, which also keeps a URL's structure.
const URL: &AsciiSet = &COMPONENT
    .remove(b';')
    .remove(b',')
    .remove(b'/')
    .remove(b'?')
    .remove(b':')
    .remove(b'@')
    .remove(b'&')
    .remove(b'=')
    .remove(b'+')
    .remove(b'$')
    .remove(b'#');

fn encode_url_component<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args.string(0)?.map_or(Value::Undefined, |text| {
        Value::string(utf8_percent_encode(text, COMPONENT).to_string())
    }))
}

fn encode_url<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args.string(0)?.map_or(Value::Undefined, |text| {
        Value::string(utf8_percent_encode(text, URL).to_string())
    }))
}

fn decode_url_component<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };

    percent_decode_str(text)
        .decode_utf8()
        .map(|text| Value::string(text.into_owned()))
        .map_err(|_| args.error("D3140", "Malformed URL passed to $decodeUrlComponent()"))
}

fn decode_url<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };

    // Escapes of reserved characters stay escaped, as decodeURI leaves them.
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('%') {
        kept.push_str(&rest[..index]);
        let escape = rest.get(index..index + 3).unwrap_or(&rest[index..]);
        let reserved = u8::from_str_radix(escape.get(1..).unwrap_or_default(), 16)
            .ok()
            .filter(|byte| b";/?:@&=+$,#".contains(byte));
        if reserved.is_some() {
            kept.push_str(&escape.replace('%', "%25"));
        } else {
            kept.push_str(escape);
        }
        rest = &rest[index + escape.len()..];
    }
    kept.push_str(rest);

    percent_decode_str(&kept)
        .decode_utf8()
        .map(|text| Value::string(text.into_owned()))
        .map_err(|_| args.error("D3140", "Malformed URL passed to $decodeUrl()"))
}
