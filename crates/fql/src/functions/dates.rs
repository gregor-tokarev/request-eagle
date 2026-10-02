//! Dates and times. Pictures are XPath's, such as
//! `[Y0001]-[M01]-[D01]`, or, as Postman's FQL also accepts, Unicode
//! patterns such as `yyyy-MM-dd`, told apart by whether they contain `[`.

use chrono::{
    DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
};

use super::formatting::format_integer;
use super::registry::{Args, Builtin};
use super::strings::builtin;
use crate::error::{Error, Result};
use crate::value::Value;

pub(super) static FUNCTIONS: &[Builtin] = &[
    builtin(
        "now",
        "$now([picture[, timezone]])",
        "The time the evaluation started, as an ISO 8601 timestamp",
        0,
        2,
        false,
        now,
    ),
    builtin(
        "millis",
        "$millis()",
        "The time the evaluation started, in milliseconds since the Unix epoch",
        0,
        0,
        false,
        millis,
    ),
    builtin(
        "fromMillis",
        "$fromMillis(number[, picture[, timezone]])",
        "Formats milliseconds since the Unix epoch as a timestamp",
        1,
        3,
        true,
        from_millis,
    ),
    builtin(
        "toMillis",
        "$toMillis(timestamp[, picture])",
        "Reads a timestamp as milliseconds since the Unix epoch",
        1,
        2,
        true,
        to_millis,
    ),
];

/// ISO 8601 with milliseconds in UTC, `$now()`'s default.
const ISO: &str = "[Y0001]-[M01]-[D01]T[H01]:[m01]:[s01].[f001][Z01:01t]";

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

fn now<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let millis = args.evaluation.now.timestamp_millis();
    format(args, millis as f64, args.string(0)?, args.string(1)?)
}

fn millis<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(Value::Number(args.evaluation.now.timestamp_millis() as f64))
}

fn from_millis<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(millis) = args.number(0)? else {
        return Ok(Value::Undefined);
    };
    format(args, millis, args.string(1)?, args.string(2)?)
}

fn format<'a>(
    args: &Args<'a, '_>,
    millis: f64,
    picture: Option<&str>,
    timezone: Option<&str>,
) -> Result<Value<'a>> {
    let offset = match timezone {
        Some(timezone) => parse_offset(timezone).ok_or_else(|| {
            args.error(
                "D3134",
                format!("The timezone \"{timezone}\" is not of the form ±HHMM"),
            )
        })?,
        None => FixedOffset::east_opt(0).expect("UTC"),
    };
    let time = offset
        .timestamp_millis_opt(millis.floor() as i64)
        .single()
        .ok_or_else(|| args.error("D3110", "The timestamp is out of range"))?;

    let picture = match picture {
        Some(picture) if picture.contains('[') => picture.to_owned(),
        Some(picture) => from_unicode(picture),
        None => ISO.to_owned(),
    };

    format_picture(&time, &picture)
        .map(Value::string)
        .map_err(|error| error.or_at(args.position))
}

fn to_millis<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };

    let millis = match args.string(1)? {
        Some(picture) if picture.contains('[') => parse_picture(text, picture),
        Some(picture) => parse_picture(text, &from_unicode(picture)),
        None => parse_iso(text).ok_or_else(|| {
            Error::new("D3110", format!("The timestamp \"{text}\" is not ISO 8601"))
        }),
    }
    .map_err(|error| error.or_at(args.position))?;

    Ok(Value::Number(millis as f64))
}

/// `+0530`, `-05:00`, `0000` or `Z`.
fn parse_offset(text: &str) -> Option<FixedOffset> {
    if text == "Z" {
        return FixedOffset::east_opt(0);
    }
    let (sign, digits) = match text.chars().next()? {
        '+' => (1, &text[1..]),
        '-' => (-1, &text[1..]),
        _ => (1, text),
    };
    let digits: String = digits.chars().filter(|ch| *ch != ':').collect();
    if digits.len() != 4 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;

    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
}

/// Milliseconds of an ISO 8601 date or timestamp. One without an offset is UTC.
pub(super) fn parse_iso(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Some(time.timestamp_millis());
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%dT%H:%M%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
    ] {
        if let Ok(time) = DateTime::parse_from_str(text, format) {
            return Some(time.timestamp_millis());
        }
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(time) = NaiveDateTime::parse_from_str(text, format) {
            return Some(time.and_utc().timestamp_millis());
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(date.and_time(NaiveTime::MIN).and_utc().timestamp_millis());
    }
    if let Ok(date) = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d") {
        return Some(date.and_time(NaiveTime::MIN).and_utc().timestamp_millis());
    }
    if text.len() == 4
        && let Ok(year) = text.parse()
    {
        return NaiveDate::from_ymd_opt(year, 1, 1)
            .map(|date| date.and_time(NaiveTime::MIN).and_utc().timestamp_millis());
    }

    None
}

/// A Unicode date pattern, such as `dd/MM/yyyy HH:mm`, as an XPath picture.
fn from_unicode(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut picture = String::new();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if ch == '\'' {
            // Quoted text, where '' is a quote.
            index += 1;
            while index < chars.len() {
                if chars[index] == '\'' {
                    if chars.get(index + 1) == Some(&'\'') {
                        picture.push('\'');
                        index += 2;
                        continue;
                    }
                    break;
                }
                push_literal(&mut picture, chars[index]);
                index += 1;
            }
            index += 1;
            continue;
        }

        let run = chars[index..]
            .iter()
            .take_while(|&&next| next == ch)
            .count();
        index += run;
        let component = match (ch, run) {
            ('y', 2) => "[Y01]".to_owned(),
            ('y', _) => format!("[Y{}]", "0".repeat(run.max(1) - 1) + "1"),
            ('M', 1) => "[M]".to_owned(),
            ('M', 2) => "[M01]".to_owned(),
            ('M', 3) => "[MNn,*-3]".to_owned(),
            ('M', _) => "[MNn]".to_owned(),
            ('d', 1) => "[D]".to_owned(),
            ('d', _) => "[D01]".to_owned(),
            ('D', _) => "[d]".to_owned(),
            ('E', 4..) => "[FNn]".to_owned(),
            ('E', _) => "[FNn,*-3]".to_owned(),
            ('H', 1) => "[H]".to_owned(),
            ('H', _) => "[H01]".to_owned(),
            ('h', 1) => "[h]".to_owned(),
            ('h', _) => "[h01]".to_owned(),
            ('m', 1) => "[m]".to_owned(),
            ('m', _) => "[m01]".to_owned(),
            ('s', 1) => "[s]".to_owned(),
            ('s', _) => "[s01]".to_owned(),
            ('S', run) => format!("[f{}]", "0".repeat(run - 1) + "1"),
            ('a', _) => "[PN]".to_owned(),
            ('Z', _) => "[Z0101]".to_owned(),
            ('X', 1) => "[Z01t]".to_owned(),
            ('X', 2) => "[Z0101t]".to_owned(),
            ('X', _) => "[Z01:01t]".to_owned(),
            ('x', _) => "[Z01:01]".to_owned(),
            (ch, run) => {
                let mut literal = String::new();
                for _ in 0..run {
                    push_literal(&mut literal, ch);
                }
                literal
            }
        };
        picture.push_str(&component);
    }

    picture
}

fn push_literal(picture: &mut String, ch: char) {
    match ch {
        '[' => picture.push_str("[["),
        ']' => picture.push_str("]]"),
        ch => picture.push(ch),
    }
}

/// A component of a picture: its letter, presentation and width.
struct Component {
    letter: char,
    presentation: String,
    ordinal: bool,
    /// `t`: a zero offset is written `Z`.
    zulu: bool,
    min_width: Option<usize>,
    max_width: Option<usize>,
}

enum Part {
    Literal(String),
    Component(Component),
}

fn parse_parts(picture: &str) -> Result<Vec<Part>> {
    let chars: Vec<char> = picture.chars().collect();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut index = 0;

    while index < chars.len() {
        match chars[index] {
            '[' if chars.get(index + 1) == Some(&'[') => {
                literal.push('[');
                index += 2;
            }
            ']' if chars.get(index + 1) == Some(&']') => {
                literal.push(']');
                index += 2;
            }
            '[' => {
                let end = chars[index..]
                    .iter()
                    .position(|&ch| ch == ']')
                    .map(|offset| index + offset)
                    .ok_or_else(|| {
                        Error::new("D3135", "The picture has a [ without a matching ]")
                    })?;
                let body: String = chars[index + 1..end]
                    .iter()
                    .filter(|ch| !ch.is_whitespace())
                    .collect();
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Component(component(&body)?));
                index = end + 1;
            }
            ch => {
                literal.push(ch);
                index += 1;
            }
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }

    Ok(parts)
}

fn component(body: &str) -> Result<Component> {
    let mut chars = body.chars();
    let letter = chars
        .next()
        .ok_or_else(|| Error::new("D3132", "The picture has an empty component"))?;
    if !"YMDdFWwXxHhPmsfZzCE".contains(letter) {
        return Err(Error::new(
            "D3132",
            format!("Unknown component specifier {letter} in date/time picture"),
        ));
    }
    let rest: String = chars.collect();
    let (modifiers, width) = match rest.split_once(',') {
        Some((modifiers, width)) => (modifiers.to_owned(), Some(width.to_owned())),
        None => (rest, None),
    };

    let mut presentation = modifiers.clone();
    let mut ordinal = false;
    let mut zulu = false;
    if letter == 'Z' || letter == 'z' {
        if presentation.ends_with('t') {
            presentation.pop();
            zulu = true;
        }
    } else if presentation.ends_with('o') && presentation.len() > 1 {
        presentation.pop();
        ordinal = true;
    } else if presentation.ends_with('c') && presentation.len() > 1 {
        presentation.pop();
    }

    let (min_width, max_width) = match width {
        Some(width) => {
            let (min, max) = width.split_once('-').unwrap_or((&width, ""));
            let parse = |text: &str| {
                (text != "*" && !text.is_empty())
                    .then(|| text.parse().ok())
                    .flatten()
            };
            (parse(min), parse(max))
        }
        None => (None, None),
    };

    Ok(Component {
        letter,
        presentation,
        ordinal,
        zulu,
        min_width,
        max_width,
    })
}

fn format_picture(time: &DateTime<FixedOffset>, picture: &str) -> Result<String> {
    let mut output = String::new();

    for part in parse_parts(picture)? {
        match part {
            Part::Literal(text) => output.push_str(&text),
            Part::Component(component) => output.push_str(&format_component(time, &component)?),
        }
    }

    Ok(output)
}

fn format_component(time: &DateTime<FixedOffset>, component: &Component) -> Result<String> {
    let name = |names: &[&str], index: usize| -> String {
        let text = names[index];
        let text = match component.presentation.as_str() {
            "N" => text.to_uppercase(),
            "n" => text.to_lowercase(),
            _ => text.to_owned(),
        };
        match component.max_width {
            Some(width) => text.chars().take(width).collect(),
            None => text,
        }
    };
    let is_name = matches!(component.presentation.as_str(), "N" | "n" | "Nn");

    let value: i64 = match component.letter {
        'Y' => time.year() as i64,
        'M' if is_name => return Ok(name(&MONTHS, time.month0() as usize)),
        'M' => time.month() as i64,
        'D' => time.day() as i64,
        'd' => time.ordinal() as i64,
        'F' if is_name || component.presentation.is_empty() => {
            let text = DAYS[time.weekday().num_days_from_monday() as usize];
            let text = match component.presentation.as_str() {
                "N" => text.to_uppercase(),
                "Nn" => text.to_owned(),
                _ => text.to_lowercase(),
            };
            return Ok(match component.max_width {
                Some(width) => text.chars().take(width).collect(),
                None => text,
            });
        }
        'F' => time.weekday().number_from_monday() as i64,
        'W' => time.iso_week().week() as i64,
        'w' => (time.day() + 6 - time.weekday().num_days_from_monday())
            .div_ceil(7)
            .max(1) as i64,
        'H' => time.hour() as i64,
        'h' => match time.hour() % 12 {
            0 => 12,
            hour => hour as i64,
        },
        'P' => {
            let text = if time.hour() < 12 { "am" } else { "pm" };
            return Ok(if component.presentation == "N" {
                text.to_uppercase()
            } else {
                text.to_owned()
            });
        }
        'm' => time.minute() as i64,
        's' => time.second() as i64,
        'f' => {
            // Fractional seconds, to as many digits as the picture has.
            let digits = component
                .presentation
                .chars()
                .filter(char::is_ascii_digit)
                .count()
                .max(1);
            let digits = component.min_width.unwrap_or(digits);
            let millis = format!("{:03}", time.timestamp_subsec_millis());
            return Ok(format!("{millis:0<digits$}").chars().take(digits).collect());
        }
        'Z' | 'z' => {
            let seconds = time.offset().local_minus_utc();
            if seconds == 0 && component.zulu {
                return Ok("Z".to_owned());
            }
            let sign = if seconds < 0 { '-' } else { '+' };
            let hours = seconds.abs() / 3600;
            let minutes = seconds.abs() % 3600 / 60;
            let offset = match component.presentation.as_str() {
                "0101" => format!("{sign}{hours:02}{minutes:02}"),
                "01" | "1" if minutes == 0 => format!("{sign}{hours:02}"),
                _ => format!("{sign}{hours:02}:{minutes:02}"),
            };
            return Ok(if component.letter == 'z' {
                format!("GMT{offset}")
            } else {
                offset
            });
        }
        'E' => return Ok("AD".to_owned()),
        'C' => return Ok("ISO".to_owned()),
        _ => {
            return Err(Error::new(
                "D3133",
                format!("The {} component is not supported", component.letter),
            ));
        }
    };

    let presentation = if component.presentation.is_empty() {
        match component.letter {
            'm' | 's' => "01",
            _ => "1",
        }
    } else {
        component.presentation.as_str()
    };
    let presentation = if component.ordinal {
        format!("{presentation};o")
    } else {
        presentation.to_owned()
    };

    let mut text = format_integer(value, &presentation)?;
    // A two digit year is the year's last two digits.
    if component.letter == 'Y'
        && (presentation.trim_end_matches(";o") == "01" || component.max_width == Some(2))
    {
        text = format!("{:02}", value.rem_euclid(100));
    }
    if let Some(width) = component.min_width
        && text.chars().count() < width
        && presentation
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
    {
        text = format!("{text:0>width$}");
    }

    Ok(text)
}

/// Read a timestamp written to a picture.
fn parse_picture(text: &str, picture: &str) -> Result<i64> {
    let parts = parse_parts(picture)?;
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;

    let mut year = 1970;
    let mut month = 1;
    let mut day = 1;
    let mut ordinal_day = None;
    let mut hour = 0;
    let mut minute = 0;
    let mut second = 0;
    let mut millis = 0;
    let mut pm = None;
    let mut offset = 0;

    let mismatch = || {
        Error::new(
            "D3110",
            format!("The timestamp \"{text}\" does not match the picture \"{picture}\""),
        )
    };

    for (position, part) in parts.iter().enumerate() {
        match part {
            Part::Literal(literal) => {
                for expected in literal.chars() {
                    if chars.get(index) != Some(&expected) {
                        return Err(mismatch());
                    }
                    index += 1;
                }
            }
            Part::Component(component) => {
                // Digits are read up to the width the picture gives them when
                // another component follows directly.
                let followed = matches!(parts.get(position + 1), Some(Part::Component(_)));
                let width = component
                    .presentation
                    .chars()
                    .filter(char::is_ascii_digit)
                    .count();
                match component.letter {
                    'M' if matches!(component.presentation.as_str(), "N" | "n" | "Nn") => {
                        let word: String = chars[index..]
                            .iter()
                            .take_while(|ch| ch.is_alphabetic())
                            .collect();
                        index += word.chars().count();
                        month = MONTHS
                            .iter()
                            .position(|name| {
                                name.to_lowercase().starts_with(&word.to_lowercase())
                                    && !word.is_empty()
                            })
                            .ok_or_else(mismatch)? as u32
                            + 1;
                    }
                    'F' => {
                        let word: String = chars[index..]
                            .iter()
                            .take_while(|ch| ch.is_alphanumeric())
                            .collect();
                        index += word.chars().count();
                    }
                    'P' => {
                        let word: String = chars[index..]
                            .iter()
                            .take_while(|ch| ch.is_alphabetic())
                            .collect();
                        index += word.chars().count();
                        pm = Some(word.to_lowercase().starts_with('p'));
                    }
                    'Z' | 'z' => {
                        let rest: String = chars[index..].iter().collect();
                        let rest = rest.strip_prefix("GMT").unwrap_or(&rest);
                        let skipped = chars.len() - index - rest.chars().count();
                        if let Some(stripped) = rest.strip_prefix('Z') {
                            let _ = stripped;
                            index += skipped + 1;
                        } else {
                            let length = rest
                                .chars()
                                .take_while(|ch| {
                                    ch.is_ascii_digit() || matches!(ch, '+' | '-' | ':')
                                })
                                .count();
                            let zone: String = rest.chars().take(length).collect();
                            offset = parse_offset(&zone).ok_or_else(mismatch)?.local_minus_utc();
                            index += skipped + length;
                        }
                    }
                    letter => {
                        let take = if followed && width > 0 {
                            width
                        } else {
                            usize::MAX
                        };
                        let digits: String = chars[index..]
                            .iter()
                            .take_while(|ch| ch.is_ascii_digit())
                            .take(take)
                            .collect();
                        if digits.is_empty() {
                            return Err(mismatch());
                        }
                        index += digits.len();
                        let number: i64 = digits.parse().map_err(|_| mismatch())?;
                        match letter {
                            'Y' => {
                                year = if digits.len() == 2 {
                                    2000 + number as i32
                                } else {
                                    number as i32
                                }
                            }
                            'M' => month = number as u32,
                            'D' => day = number as u32,
                            'd' => ordinal_day = Some(number as u32),
                            'H' | 'h' => hour = number as u32,
                            'm' => minute = number as u32,
                            's' => second = number as u32,
                            'f' => {
                                let scaled = format!("{digits:0<3}");
                                millis = scaled[..3].parse().unwrap_or(0);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if index != chars.len() {
        return Err(mismatch());
    }

    if let Some(pm) = pm {
        hour %= 12;
        if pm {
            hour += 12;
        }
    }
    let date = match ordinal_day {
        Some(ordinal) => NaiveDate::from_yo_opt(year, ordinal),
        None => NaiveDate::from_ymd_opt(year, month, day),
    }
    .ok_or_else(mismatch)?;
    let time = date
        .and_hms_milli_opt(hour, minute, second, millis)
        .ok_or_else(mismatch)?;

    Ok(time.and_utc().timestamp_millis() - offset as i64 * 1000)
}

/// Milliseconds of an argument that is a timestamp or a number of milliseconds.
pub(super) fn instant(args: &Args, index: usize) -> Result<Option<i64>> {
    match args.get(index) {
        Value::Undefined => Ok(None),
        Value::Number(millis) => Ok(Some(millis as i64)),
        Value::String(text) => parse_iso(text.as_str()).map(Some).ok_or_else(|| {
            args.error(
                "D3110",
                format!("The timestamp \"{}\" is not ISO 8601", text.as_str()),
            )
        }),
        _ => Err(args.mismatch(index)),
    }
}

/// A UTC date and time from milliseconds.
pub(super) fn utc(millis: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(millis).single()
}
