//! XPath picture strings: `$formatNumber`'s decimal formats, and
//! `$formatInteger` and `$parseInteger`'s digits, words, roman numerals and
//! letters.

use crate::error::{Error, Result};
use crate::value::{Object, Value};

use super::numbers::round_half_even;

struct Symbols {
    decimal: char,
    grouping: char,
    exponent: char,
    minus: String,
    percent: String,
    per_mille: String,
    zero: char,
    digit: char,
    pattern: char,
    infinity: String,
    nan: String,
}

impl Symbols {
    fn new(options: Option<&Object>) -> Result<Self> {
        let mut symbols = Self {
            decimal: '.',
            grouping: ',',
            exponent: 'e',
            minus: "-".to_owned(),
            percent: "%".to_owned(),
            per_mille: "\u{2030}".to_owned(),
            zero: '0',
            digit: '#',
            pattern: ';',
            infinity: "Infinity".to_owned(),
            nan: "NaN".to_owned(),
        };

        let Some(options) = options else {
            return Ok(symbols);
        };
        for (key, value) in options.entries() {
            let Value::String(text) = value else {
                return Err(Error::new(
                    "D3080",
                    format!("The option {key} of $formatNumber must be a string"),
                ));
            };
            let text = text.as_str().to_owned();
            let single = || {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(ch), None) => Ok(ch),
                    _ => Err(Error::new(
                        "D3080",
                        format!("The option {key} of $formatNumber must be one character"),
                    )),
                }
            };
            match key.as_str() {
                "decimal-separator" => symbols.decimal = single()?,
                "grouping-separator" => symbols.grouping = single()?,
                "exponent-separator" => symbols.exponent = single()?,
                "zero-digit" => symbols.zero = single()?,
                "digit" => symbols.digit = single()?,
                "pattern-separator" => symbols.pattern = single()?,
                "minus-sign" => symbols.minus = text,
                "percent" => symbols.percent = text,
                "per-mille" => symbols.per_mille = text,
                "infinity" => symbols.infinity = text,
                "NaN" => symbols.nan = text,
                _ => {}
            }
        }

        Ok(symbols)
    }

    fn is_digit(&self, ch: char) -> bool {
        (self.zero..=char::from_u32(self.zero as u32 + 9).unwrap_or(self.zero)).contains(&ch)
    }
}

/// `$formatNumber`: a number in a decimal format such as `#,##0.00`.
pub(crate) fn format_number(
    number: f64,
    picture: &str,
    options: Option<&Object>,
) -> Result<String> {
    let symbols = Symbols::new(options)?;
    if number.is_nan() {
        return Ok(symbols.nan);
    }

    let pictures: Vec<&str> = picture.split(symbols.pattern).collect();
    if pictures.len() > 2 {
        return Err(Error::new(
            "D3080",
            "The picture of $formatNumber has more than two parts",
        ));
    }
    let (picture, negative_prefix) = if number < 0. && pictures.len() == 2 {
        (pictures[1], "")
    } else if number < 0. {
        (pictures[0], symbols.minus.as_str())
    } else {
        (pictures[0], "")
    };

    // Active characters make up the number; the rest are its prefix and suffix.
    let active = |ch: char| {
        symbols.is_digit(ch)
            || ch == symbols.digit
            || ch == symbols.decimal
            || ch == symbols.grouping
            || ch == symbols.exponent
    };
    let chars: Vec<char> = picture.chars().collect();
    let first = chars
        .iter()
        .position(|&ch| active(ch) && ch != symbols.exponent);
    let last = chars
        .iter()
        .rposition(|&ch| active(ch) && ch != symbols.exponent);
    let (Some(first), Some(last)) = (first, last) else {
        return Err(Error::new(
            "D3085",
            "The picture of $formatNumber has no digits",
        ));
    };
    let prefix: String = chars[..first].iter().collect();
    let suffix: String = chars[last + 1..].iter().collect();
    let body: String = chars[first..=last].iter().collect();

    let percent = prefix.contains(&symbols.percent) || suffix.contains(&symbols.percent);
    let per_mille = prefix.contains(&symbols.per_mille) || suffix.contains(&symbols.per_mille);
    let mut value = number.abs();
    if percent {
        value *= 100.;
    } else if per_mille {
        value *= 1000.;
    }
    if value.is_infinite() {
        return Ok(format!(
            "{negative_prefix}{prefix}{}{suffix}",
            symbols.infinity
        ));
    }

    let (mantissa, exponent_part) = match body.split_once(symbols.exponent) {
        Some((mantissa, exponent))
            if exponent.chars().all(|ch| symbols.is_digit(ch)) && !exponent.is_empty() =>
        {
            (mantissa.to_owned(), Some(exponent.chars().count()))
        }
        _ => (body.clone(), None),
    };
    let (integer_picture, fraction_picture) = match mantissa.split_once(symbols.decimal) {
        Some((integer, fraction)) => (integer.to_owned(), fraction.to_owned()),
        None => (mantissa.clone(), String::new()),
    };

    let mut minimum_integer = integer_picture
        .chars()
        .filter(|&ch| symbols.is_digit(ch))
        .count();
    let minimum_fraction = fraction_picture
        .chars()
        .filter(|&ch| symbols.is_digit(ch))
        .count();
    let maximum_fraction = fraction_picture
        .chars()
        .filter(|&ch| symbols.is_digit(ch) || ch == symbols.digit)
        .count();
    if minimum_integer == 0 && maximum_fraction == 0 {
        minimum_integer = 1;
    }

    // With an exponent, the mantissa keeps the integer digits the picture asks for.
    let mut exponent = 0i32;
    if exponent_part.is_some() && value != 0. {
        let digits = minimum_integer.max(1) as i32;
        exponent = value.abs().log10().floor() as i32 - (digits - 1);
        value /= 10f64.powi(exponent);
    }

    let rounded = round_half_even(value, maximum_fraction as i32);
    // A rounded mantissa can gain a digit, such as 9.99 to 10.0.
    let rounded = if exponent_part.is_some() && rounded >= 10f64.powi(minimum_integer.max(1) as i32)
    {
        exponent += 1;
        round_half_even(value / 10., maximum_fraction as i32)
    } else {
        rounded
    };

    let text = format!("{rounded:.maximum_fraction$}");
    let (integer_digits, fraction_digits) = match text.split_once('.') {
        Some((integer, fraction)) => (integer.to_owned(), fraction.to_owned()),
        None => (text, String::new()),
    };

    let mut integer_digits = integer_digits.trim_start_matches('0').to_owned();
    while integer_digits.chars().count() < minimum_integer {
        integer_digits.insert(0, '0');
    }
    let mut fraction_digits = fraction_digits;
    while fraction_digits.len() > minimum_fraction && fraction_digits.ends_with('0') {
        fraction_digits.pop();
    }

    let integer = group(&integer_digits, &integer_picture, &symbols, true);
    let fraction = group(&fraction_digits, &fraction_picture, &symbols, false);
    let digit = |ch: char| match ch.to_digit(10) {
        Some(value) => char::from_u32(symbols.zero as u32 + value).unwrap_or(ch),
        None => ch,
    };

    let mut result = format!("{negative_prefix}{prefix}");
    result.extend(integer.chars().map(digit));
    if !fraction.is_empty() {
        result.push(symbols.decimal);
        result.extend(fraction.chars().map(digit));
    }
    if let Some(exponent_digits) = exponent_part {
        result.push(symbols.exponent);
        if exponent < 0 {
            result.push_str(&symbols.minus);
        }
        let digits = format!("{:0>width$}", exponent.abs(), width = exponent_digits);
        result.extend(digits.chars().map(digit));
    }
    result.push_str(&suffix);

    Ok(result)
}

/// Insert the picture's grouping separators: counted from the right of the
/// integer part, or from the left of the fraction. Evenly spaced
/// separators repeat.
fn group(digits: &str, picture: &str, symbols: &Symbols, integer: bool) -> String {
    let marks: Vec<usize> = {
        let chars: Vec<char> = picture.chars().collect();
        let ordered: Vec<char> = if integer {
            chars.iter().rev().copied().collect()
        } else {
            chars
        };
        let mut count = 0;
        let mut marks = Vec::new();
        for ch in ordered {
            if ch == symbols.grouping {
                marks.push(count);
            } else if symbols.is_digit(ch) || ch == symbols.digit {
                count += 1;
            }
        }
        marks
    };
    if marks.is_empty() {
        return digits.to_owned();
    }

    let regular = integer && marks.iter().all(|&mark| mark % marks[0] == 0) && marks[0] > 0;
    let chars: Vec<char> = if integer {
        digits.chars().rev().collect()
    } else {
        digits.chars().collect()
    };
    let mut output = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        let at = index;
        let separate = if regular {
            at > 0 && at % marks[0] == 0
        } else {
            marks.contains(&at) && at > 0
        };
        if separate {
            output.push(symbols.grouping);
        }
        output.push(*ch);
    }
    if integer {
        output.reverse();
    }

    output.into_iter().collect()
}

pub(crate) fn to_radix(number: i64, radix: u32) -> String {
    let negative = number < 0;
    let mut value = number.unsigned_abs();
    let mut digits = Vec::new();
    loop {
        let digit = (value % radix as u64) as u32;
        digits.push(std::char::from_digit(digit, radix).unwrap_or('0'));
        value /= radix as u64;
        if value == 0 {
            break;
        }
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

const FEW: [&str; 20] = [
    "Zero",
    "One",
    "Two",
    "Three",
    "Four",
    "Five",
    "Six",
    "Seven",
    "Eight",
    "Nine",
    "Ten",
    "Eleven",
    "Twelve",
    "Thirteen",
    "Fourteen",
    "Fifteen",
    "Sixteen",
    "Seventeen",
    "Eighteen",
    "Nineteen",
];
const ORDINALS: [&str; 20] = [
    "Zeroth",
    "First",
    "Second",
    "Third",
    "Fourth",
    "Fifth",
    "Sixth",
    "Seventh",
    "Eighth",
    "Ninth",
    "Tenth",
    "Eleventh",
    "Twelfth",
    "Thirteenth",
    "Fourteenth",
    "Fifteenth",
    "Sixteenth",
    "Seventeenth",
    "Eighteenth",
    "Nineteenth",
];
const DECADES: [&str; 8] = [
    "Twenty", "Thirty", "Forty", "Fifty", "Sixty", "Seventy", "Eighty", "Ninety",
];
const MAGNITUDES: [&str; 4] = ["Thousand", "Million", "Billion", "Trillion"];

/// English words for a number, as JSONata writes them.
fn words(number: u64, previous: bool, ordinal: bool) -> String {
    if number <= 19 {
        let word = if ordinal {
            ORDINALS[number as usize]
        } else {
            FEW[number as usize]
        };
        return format!("{}{word}", if previous { " and " } else { "" });
    }
    if number < 100 {
        let tens = (number / 10) as usize;
        let remainder = number % 10;
        let mut text = format!(
            "{}{}",
            if previous { " and " } else { "" },
            DECADES[tens - 2]
        );
        if remainder > 0 {
            text.push('-');
            text.push_str(&words(remainder, false, ordinal));
        } else if ordinal {
            text.pop();
            text.push_str("ieth");
        }
        return text;
    }
    if number < 1000 {
        let mut text = format!(
            "{}{} Hundred",
            if previous { ", " } else { "" },
            FEW[(number / 100) as usize]
        );
        let remainder = number % 100;
        if remainder > 0 {
            text.push_str(&words(remainder, true, ordinal));
        } else if ordinal {
            text.push_str("th");
        }
        return text;
    }

    let magnitude = (((number as f64).log10() / 3.).floor() as usize).min(MAGNITUDES.len());
    let factor = 10u64.pow(magnitude as u32 * 3);
    let mantissa = number / factor;
    let remainder = number - mantissa * factor;
    let mut text = format!(
        "{}{} {}",
        if previous { ", " } else { "" },
        words(mantissa, false, false),
        MAGNITUDES[magnitude - 1]
    );
    if remainder > 0 {
        text.push_str(&words(remainder, true, ordinal));
    } else if ordinal {
        text.push_str("th");
    }
    text
}

fn roman(mut number: u64) -> String {
    const NUMERALS: [(u64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut text = String::new();
    for (value, numeral) in NUMERALS {
        while number >= value {
            text.push_str(numeral);
            number -= value;
        }
    }
    text
}

fn letters(mut number: u64, base: char) -> String {
    let mut text = Vec::new();
    while number > 0 {
        number -= 1;
        text.push(char::from_u32(base as u32 + (number % 26) as u32).unwrap_or(base));
        number /= 26;
    }
    text.iter().rev().collect()
}

fn ordinal_suffix(number: u64) -> &'static str {
    match (number % 10, number % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

/// A picture and its `;o` ordinal modifier.
fn split_picture(picture: &str) -> (&str, bool) {
    match picture.rsplit_once(';') {
        Some((primary, modifier)) if !primary.is_empty() => (primary, modifier.starts_with('o')),
        _ => (picture, false),
    }
}

/// `$formatInteger`.
pub(crate) fn format_integer(number: i64, picture: &str) -> Result<String> {
    let (primary, ordinal) = split_picture(picture);
    let value = number.unsigned_abs();
    let sign = if number < 0 { "-" } else { "" };

    Ok(match primary {
        "w" => format!(
            "{}{}",
            if number < 0 { "minus " } else { "" },
            words(value, false, ordinal).to_lowercase()
        ),
        "W" => format!(
            "{}{}",
            if number < 0 { "MINUS " } else { "" },
            words(value, false, ordinal).to_uppercase()
        ),
        "Ww" => format!(
            "{}{}",
            if number < 0 { "Minus " } else { "" },
            words(value, false, ordinal)
        ),
        "I" => format!("{sign}{}", roman(value)),
        "i" => format!("{sign}{}", roman(value).to_lowercase()),
        "A" => format!("{sign}{}", letters(value, 'A')),
        "a" => format!("{sign}{}", letters(value, 'a')),
        primary => {
            let mandatory = primary.chars().filter(char::is_ascii_digit).count();
            let optional = primary.chars().filter(|&ch| ch == '#').count();
            if mandatory == 0 && optional == 0 {
                return Err(Error::new(
                    "D3130",
                    format!("The picture \"{picture}\" of $formatInteger is not supported"),
                ));
            }
            let digits = format!("{:0>width$}", value, width = mandatory.max(1));
            let separator = primary
                .chars()
                .find(|ch| !ch.is_ascii_digit() && *ch != '#')
                .unwrap_or(',');
            let symbols = Symbols {
                grouping: separator,
                ..Symbols::new(None)?
            };
            let grouped = group(&digits, primary, &symbols, true);
            format!(
                "{sign}{grouped}{}",
                if ordinal { ordinal_suffix(value) } else { "" }
            )
        }
    })
}

/// `$parseInteger`: the inverse of `$formatInteger`.
pub(crate) fn parse_integer(text: &str, picture: &str) -> Result<Option<i64>> {
    let (primary, _) = split_picture(picture);
    let text = text.trim();

    Ok(match primary {
        "w" | "W" | "Ww" => parse_words(text),
        "I" | "i" => parse_roman(&text.to_uppercase()),
        "A" | "a" => {
            let mut number: i64 = 0;
            for ch in text.to_lowercase().chars() {
                if !ch.is_ascii_lowercase() {
                    return Ok(None);
                }
                number = number * 26 + (ch as i64 - 'a' as i64 + 1);
            }
            Some(number)
        }
        _ => {
            let digits: String = text
                .chars()
                .filter(|ch| ch.is_ascii_digit() || *ch == '-')
                .collect();
            digits.parse().ok()
        }
    })
}

fn parse_roman(text: &str) -> Option<i64> {
    let value = |ch: char| match ch {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };
    let values: Vec<i64> = text.chars().map(value).collect::<Option<_>>()?;
    let mut total = 0;
    for (index, current) in values.iter().enumerate() {
        if values.get(index + 1).is_some_and(|next| next > current) {
            total -= current;
        } else {
            total += current;
        }
    }
    Some(total)
}

fn parse_words(text: &str) -> Option<i64> {
    let mut total: i64 = 0;
    let mut current: i64 = 0;

    for word in text
        .to_lowercase()
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '-')
        .filter(|word| !word.is_empty() && *word != "and")
    {
        let cardinal = FEW
            .iter()
            .position(|few| few.to_lowercase() == word)
            .or_else(|| {
                ORDINALS
                    .iter()
                    .position(|ordinal| ordinal.to_lowercase() == word)
            })
            .map(|value| value as i64)
            .or_else(|| {
                DECADES
                    .iter()
                    .position(|decade| {
                        let decade = decade.to_lowercase();
                        decade == word || format!("{}ieth", &decade[..decade.len() - 1]) == word
                    })
                    .map(|index| (index as i64 + 2) * 10)
            });

        if let Some(value) = cardinal {
            current += value;
        } else if word == "hundred" || word == "hundredth" {
            current *= 100;
        } else if let Some(index) = MAGNITUDES.iter().position(|magnitude| {
            magnitude.to_lowercase() == word || format!("{}th", magnitude.to_lowercase()) == word
        }) {
            total += current * 10i64.pow(3 * (index as u32 + 1));
            current = 0;
        } else if word == "minus" {
            continue;
        } else {
            return None;
        }
    }

    let total = total + current;
    Some(if text.to_lowercase().starts_with("minus") {
        -total
    } else {
        total
    })
}
