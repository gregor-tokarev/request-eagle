use super::formatting;
use super::registry::{Args, Builtin};
use super::strings::builtin;
use crate::error::Result;
use crate::value::{Value, format_number};

pub(super) static FUNCTIONS: &[Builtin] = &[
    builtin(
        "number",
        "$number(arg)",
        "Casts a string or boolean to a number; strings may be 0x hex, 0o octal or 0b binary",
        1,
        1,
        true,
        number,
    ),
    builtin("abs", "$abs(number)", "The absolute value", 1, 1, true, abs),
    builtin("floor", "$floor(number)", "Rounds down", 1, 1, true, floor),
    builtin("ceil", "$ceil(number)", "Rounds up", 1, 1, true, ceil),
    builtin(
        "round",
        "$round(number[, precision])",
        "Rounds half to even, to precision decimal places",
        1,
        2,
        true,
        round,
    ),
    builtin(
        "power",
        "$power(base, exponent)",
        "base raised to exponent",
        2,
        2,
        true,
        power,
    ),
    builtin("sqrt", "$sqrt(number)", "The square root", 1, 1, true, sqrt),
    builtin(
        "random",
        "$random()",
        "A random number from 0 up to 1",
        0,
        0,
        false,
        random,
    ),
    builtin(
        "formatNumber",
        "$formatNumber(number, picture[, options])",
        "Formats a number with an XPath picture such as \"#,##0.00\"",
        2,
        3,
        true,
        format_number_,
    ),
    builtin(
        "formatBase",
        "$formatBase(number[, radix])",
        "The number in base radix, from 2 to 36",
        1,
        2,
        true,
        format_base,
    ),
    builtin(
        "formatInteger",
        "$formatInteger(number, picture)",
        "Formats an integer as digits, words (\"w\") or roman numerals (\"I\")",
        2,
        2,
        true,
        format_integer,
    ),
    builtin(
        "parseInteger",
        "$parseInteger(string, picture)",
        "Reads an integer written as $formatInteger writes it",
        2,
        2,
        true,
        parse_integer,
    ),
    builtin(
        "sum",
        "$sum(array)",
        "The sum of an array of numbers",
        1,
        1,
        false,
        sum,
    ),
    builtin(
        "max",
        "$max(array)",
        "The largest number of an array",
        1,
        1,
        false,
        max,
    ),
    builtin(
        "min",
        "$min(array)",
        "The smallest number of an array",
        1,
        1,
        false,
        min,
    ),
    builtin(
        "average",
        "$average(array)",
        "The mean of an array of numbers",
        1,
        1,
        false,
        average,
    ),
    builtin(
        "count",
        "$count(array)",
        "The number of items",
        1,
        1,
        false,
        count,
    ),
    builtin(
        "boolean",
        "$boolean(arg)",
        "Casts a value to a boolean",
        1,
        1,
        true,
        boolean,
    ),
    builtin(
        "not",
        "$not(arg)",
        "The boolean opposite of a value",
        1,
        1,
        true,
        not,
    ),
    builtin(
        "exists",
        "$exists(arg)",
        "Whether a value is defined",
        1,
        1,
        false,
        exists,
    ),
];

pub(super) fn checked<'a>(
    args: &Args<'a, '_>,
    number: f64,
    code: &'static str,
) -> Result<Value<'a>> {
    if number.is_finite() {
        Ok(Value::Number(if number == 0. { 0. } else { number }))
    } else {
        Err(args.error(code, format!("The result of {} is out of range", args.name)))
    }
}

fn unary<'a>(args: &Args<'a, '_>, operation: impl Fn(f64) -> f64) -> Result<Value<'a>> {
    match args.number(0)? {
        Some(number) => checked(args, operation(number), "D1001"),
        None => Ok(Value::Undefined),
    }
}

fn number<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    match args.get(0) {
        Value::Undefined => Ok(Value::Undefined),
        Value::Number(number) => Ok(Value::Number(number)),
        Value::Bool(value) => Ok(Value::Number(if value { 1. } else { 0. })),
        Value::String(text) => parse_number(text.as_str())
            .map(Value::Number)
            .ok_or_else(|| {
                args.error(
                    "D3030",
                    format!("Unable to cast value to a number: \"{}\"", text.as_str()),
                )
            }),
        _ => Err(args.mismatch(0)),
    }
}

/// A number written as JSON, or as 0x hex, 0o octal or 0b binary.
pub(super) fn parse_number(text: &str) -> Option<f64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    let radix = |prefix: [&str; 2], radix: u32| {
        prefix
            .iter()
            .find_map(|prefix| digits.strip_prefix(prefix))
            .and_then(|digits| u64::from_str_radix(digits, radix).ok())
            .map(|number| {
                if negative {
                    -(number as f64)
                } else {
                    number as f64
                }
            })
    };
    if let Some(number) = radix(["0x", "0X"], 16)
        .or_else(|| radix(["0o", "0O"], 8))
        .or_else(|| radix(["0b", "0B"], 2))
    {
        return Some(number);
    }

    // JSON's number syntax, without leading zeros or a bare point.
    let valid = {
        let bytes = digits.as_bytes();
        let mut index = 0;
        let integer_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let integer = &digits[integer_start..index];
        let mut valid = !integer.is_empty() && (integer == "0" || !integer.starts_with('0'));
        if valid && index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            let fraction_start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            valid = index > fraction_start;
        }
        if valid && index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
            index += 1;
            if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
                index += 1;
            }
            let exponent_start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            valid = index > exponent_start;
        }
        valid && index == bytes.len()
    };

    valid
        .then(|| text.parse::<f64>().ok())
        .flatten()
        .filter(|number| number.is_finite())
}

fn abs<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    unary(args, f64::abs)
}

fn floor<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    unary(args, f64::floor)
}

fn ceil<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    unary(args, f64::ceil)
}

fn round<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(number) = args.number(0)? else {
        return Ok(Value::Undefined);
    };
    let precision = args.number(1)?.unwrap_or(0.).trunc() as i32;

    Ok(Value::Number(round_half_even(number, precision)))
}

/// Rounds half to even at `precision` decimal places, shifting the point in
/// the number's text, as JSONata does, to avoid errors of multiplying.
pub(crate) fn round_half_even(number: f64, precision: i32) -> f64 {
    let shift = |number: f64, places: i32| -> f64 {
        if places == 0 {
            return number;
        }
        let text = format_number(number);
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let exponent: i32 = exponent.parse().unwrap_or(0);
        format!("{mantissa}e{}", exponent + places)
            .parse()
            .unwrap_or(number)
    };

    let shifted = shift(number, precision);
    // Math.round rounds half up.
    let mut result = (shifted + 0.5).floor();
    if (result - shifted).abs() == 0.5 && (result % 2.).abs() == 1. {
        result -= 1.;
    }
    let result = shift(result, -precision);
    if result == 0. { 0. } else { result }
}

fn power<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(base) = args.number(0)? else {
        return Ok(Value::Undefined);
    };
    let exponent = args.number(1)?.ok_or_else(|| args.mismatch(1))?;

    checked(args, base.powf(exponent), "D3061")
}

fn sqrt<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    match args.number(0)? {
        Some(number) if number < 0. => Err(args.error(
            "D3060",
            format!(
                "The sqrt function cannot be applied to a negative number: {}",
                format_number(number)
            ),
        )),
        Some(number) => Ok(Value::Number(number.sqrt())),
        None => Ok(Value::Undefined),
    }
}

fn random<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(Value::Number(args.evaluation.random()))
}

fn format_number_<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(number) = args.number(0)? else {
        return Ok(Value::Undefined);
    };
    let picture = args.string(1)?.ok_or_else(|| args.mismatch(1))?;
    let options = args.object(2)?;

    formatting::format_number(number, picture, options.as_ref())
        .map(Value::string)
        .map_err(|error| error.or_at(args.position))
}

fn format_base<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(number) = args.number(0)? else {
        return Ok(Value::Undefined);
    };
    let radix = args.number(1)?.unwrap_or(10.);
    if !(2. ..=36.).contains(&radix) {
        return Err(args.error(
            "D3100",
            format!(
                "The radix of the formatBase function must be between 2 and 36. It was given {}",
                format_number(radix)
            ),
        ));
    }

    Ok(Value::string(formatting::to_radix(
        number.round() as i64,
        radix as u32,
    )))
}

fn format_integer<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(number) = args.number(0)? else {
        return Ok(Value::Undefined);
    };
    let picture = args.string(1)?.ok_or_else(|| args.mismatch(1))?;

    formatting::format_integer(number.floor() as i64, picture)
        .map(Value::string)
        .map_err(|error| error.or_at(args.position))
}

fn parse_integer<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };
    let picture = args.string(1)?.ok_or_else(|| args.mismatch(1))?;

    formatting::parse_integer(text, picture)
        .map(|number| number.map_or(Value::Undefined, |number| Value::Number(number as f64)))
        .map_err(|error| error.or_at(args.position))
}

fn sum<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.numbers(0)? {
        Some(numbers) => Value::Number(numbers.iter().sum()),
        None => Value::Undefined,
    })
}

fn max<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.numbers(0)? {
        Some(numbers) if !numbers.is_empty() => {
            Value::Number(numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        _ => Value::Undefined,
    })
}

fn min<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.numbers(0)? {
        Some(numbers) if !numbers.is_empty() => {
            Value::Number(numbers.iter().copied().fold(f64::INFINITY, f64::min))
        }
        _ => Value::Undefined,
    })
}

fn average<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(match args.numbers(0)? {
        Some(numbers) if !numbers.is_empty() => {
            Value::Number(numbers.iter().sum::<f64>() / numbers.len() as f64)
        }
        _ => Value::Undefined,
    })
}

fn count<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(Value::Number(
        args.array(0).map_or(0, |items| items.len()) as f64
    ))
}

fn boolean<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args.get(0).truthy().map_or(Value::Undefined, Value::Bool))
}

fn not<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args
        .get(0)
        .truthy()
        .map_or(Value::Undefined, |value| Value::Bool(!value)))
}

fn exists<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(Value::Bool(!args.get(0).is_undefined()))
}
