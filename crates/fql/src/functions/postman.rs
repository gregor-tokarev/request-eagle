//! Functions Postman's FQL adds to JSONata's: JSON text, UUIDs, more math,
//! and date arithmetic on ISO 8601 timestamps or epoch milliseconds.

use chrono::{DateTime, Datelike, Duration, Months, Timelike, Utc};

use super::dates::{instant, utc};
use super::numbers::checked;
use super::registry::{Args, Builtin, stringify};
use super::strings::{builtin, owned};
use crate::error::Result;
use crate::value::Value;

pub(super) static FUNCTIONS: &[Builtin] = &[
    builtin(
        "jsonParse",
        "$jsonParse(str)",
        "Parses JSON text",
        1,
        1,
        true,
        json_parse,
    ),
    builtin(
        "json",
        "$json(value[, prettify])",
        "The value as JSON text",
        1,
        2,
        true,
        json,
    ),
    builtin(
        "partition",
        "$partition(array, size)",
        "Splits an array into arrays of size items",
        2,
        2,
        false,
        partition,
    ),
    builtin("uuid", "$uuid()", "A random UUID v4", 0, 0, false, uuid),
    builtin(
        "constant",
        "$constant(name)",
        "A mathematical constant: e, pi (π), \"ln 2\", \"log2 e\" or \"log10 e\"",
        1,
        1,
        false,
        constant,
    ),
    builtin(
        "isFinite",
        "$isFinite(number)",
        "Whether a number is finite",
        1,
        1,
        true,
        is_finite,
    ),
    builtin("cbrt", "$cbrt(number)", "The cube root", 1, 1, true, cbrt),
    builtin(
        "exp",
        "$exp(number)",
        "e raised to a number",
        1,
        1,
        true,
        exp,
    ),
    builtin(
        "log",
        "$log(number)",
        "The natural logarithm",
        1,
        1,
        true,
        log,
    ),
    builtin(
        "log10",
        "$log10(number)",
        "The base 10 logarithm",
        1,
        1,
        true,
        log10,
    ),
    builtin(
        "log2",
        "$log2(number)",
        "The base 2 logarithm",
        1,
        1,
        true,
        log2,
    ),
    builtin("sin", "$sin(radians)", "The sine", 1, 1, true, sin),
    builtin("cos", "$cos(radians)", "The cosine", 1, 1, true, cos),
    builtin("tan", "$tan(radians)", "The tangent", 1, 1, true, tan),
    builtin(
        "asin",
        "$asin(number)",
        "The arcsine, in radians",
        1,
        1,
        true,
        asin,
    ),
    builtin(
        "acos",
        "$acos(number)",
        "The arccosine, in radians",
        1,
        1,
        true,
        acos,
    ),
    builtin(
        "atan",
        "$atan(number)",
        "The arctangent, in radians",
        1,
        1,
        true,
        atan,
    ),
    builtin(
        "atan2",
        "$atan2(y, x)",
        "The angle of the point (x, y), in radians",
        2,
        2,
        false,
        atan2,
    ),
    builtin(
        "sinh",
        "$sinh(number)",
        "The hyperbolic sine",
        1,
        1,
        true,
        sinh,
    ),
    builtin(
        "cosh",
        "$cosh(number)",
        "The hyperbolic cosine",
        1,
        1,
        true,
        cosh,
    ),
    builtin(
        "tanh",
        "$tanh(number)",
        "The hyperbolic tangent",
        1,
        1,
        true,
        tanh,
    ),
    builtin(
        "asinh",
        "$asinh(number)",
        "The inverse hyperbolic sine",
        1,
        1,
        true,
        asinh,
    ),
    builtin(
        "acosh",
        "$acosh(number)",
        "The inverse hyperbolic cosine",
        1,
        1,
        true,
        acosh,
    ),
    builtin(
        "atanh",
        "$atanh(number)",
        "The inverse hyperbolic tangent",
        1,
        1,
        true,
        atanh,
    ),
    builtin(
        "afterDate",
        "$afterDate(timestamp1, timestamp2)",
        "Whether the first time is after the second",
        2,
        2,
        false,
        after_date,
    ),
    builtin(
        "beforeDate",
        "$beforeDate(timestamp1, timestamp2)",
        "Whether the first time is before the second",
        2,
        2,
        false,
        before_date,
    ),
    builtin(
        "dateEquals",
        "$dateEquals(timestamp1, timestamp2)",
        "Whether two times are the same instant",
        2,
        2,
        false,
        date_equals,
    ),
    builtin(
        "datePlus",
        "$datePlus(timestamp, amount, unit)",
        "The time amount units later, in epoch milliseconds; units are years, months, days, hours, minutes, seconds or milliseconds",
        3,
        3,
        false,
        date_plus,
    ),
    builtin(
        "diffDate",
        "$diffDate(timestamp1, timestamp2, unit)",
        "How many whole units the first time is after the second",
        3,
        3,
        false,
        diff_date,
    ),
    builtin(
        "hasSameDate",
        "$hasSameDate(timestamp1, timestamp2, units)",
        "Whether two times agree in each of the units, in UTC",
        3,
        3,
        false,
        has_same_date,
    ),
    builtin(
        "year",
        "$year(timestamp)",
        "The year, in UTC",
        1,
        1,
        true,
        year,
    ),
    builtin(
        "month",
        "$month(timestamp)",
        "The month from 1 to 12, in UTC",
        1,
        1,
        true,
        month,
    ),
    builtin(
        "day",
        "$day(timestamp)",
        "The day of the month, in UTC",
        1,
        1,
        true,
        day,
    ),
    builtin(
        "hours",
        "$hours(timestamp)",
        "The hour, in UTC",
        1,
        1,
        true,
        hours,
    ),
    builtin(
        "minutes",
        "$minutes(timestamp)",
        "The minute, in UTC",
        1,
        1,
        true,
        minutes,
    ),
    builtin(
        "seconds",
        "$seconds(timestamp)",
        "The second, in UTC",
        1,
        1,
        true,
        seconds,
    ),
    builtin(
        "milliSeconds",
        "$milliSeconds(timestamp)",
        "The millisecond, in UTC",
        1,
        1,
        true,
        milliseconds,
    ),
    builtin(
        "dayOfTheWeek",
        "$dayOfTheWeek(timestamp)",
        "The day of the week from 0 for Sunday, in UTC",
        1,
        1,
        true,
        day_of_the_week,
    ),
];

fn json_parse<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(text) = args.string(0)? else {
        return Ok(Value::Undefined);
    };

    serde_json::from_str::<serde_json::Value>(text)
        .map(|json| owned(&json))
        .map_err(|error| args.error("D3150", format!("The text is not JSON: {error}")))
}

fn json<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let value = args.get(0);
    if value.is_undefined() {
        return Ok(Value::Undefined);
    }
    let pretty = args.boolean(1)?.unwrap_or(false);

    // Unlike $string, strings are quoted.
    match &value {
        Value::String(text) => Ok(Value::string(
            serde_json::to_string(text.as_str()).unwrap_or_default(),
        )),
        value => stringify(value, pretty).map(Value::string),
    }
}

fn partition<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(items) = args.array(0) else {
        return Ok(Value::Undefined);
    };
    let size = args.number(1)?.ok_or_else(|| args.mismatch(1))?;
    if size < 1. {
        return Err(args.error("D3151", "The size of $partition must be at least 1"));
    }

    Ok(Value::array(
        items
            .chunks(size as usize)
            .map(|chunk| Value::array(chunk.to_vec()))
            .collect(),
    ))
}

fn uuid<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        *byte = (args.evaluation.random() * 256.) as u8;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Value::string(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )))
}

fn constant<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let name = args.string(0)?.ok_or_else(|| args.mismatch(0))?;

    Ok(Value::Number(match name {
        "e" => std::f64::consts::E,
        "pi" | "π" => std::f64::consts::PI,
        "ln 2" => std::f64::consts::LN_2,
        "ln 10" => std::f64::consts::LN_10,
        "log2 e" => std::f64::consts::LOG2_E,
        "log10 e" => std::f64::consts::LOG10_E,
        "sqrt 2" => std::f64::consts::SQRT_2,
        _ => return Err(args.error("D3152", format!("Unknown constant \"{name}\""))),
    }))
}

fn is_finite<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    Ok(args
        .number(0)?
        .map_or(Value::Undefined, |number| Value::Bool(number.is_finite())))
}

fn math<'a>(args: &Args<'a, '_>, operation: fn(f64) -> f64) -> Result<Value<'a>> {
    match args.number(0)? {
        Some(number) => checked(args, operation(number), "D3153"),
        None => Ok(Value::Undefined),
    }
}

fn cbrt<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::cbrt)
}

fn exp<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::exp)
}

fn log<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::ln)
}

fn log10<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::log10)
}

fn log2<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::log2)
}

fn sin<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::sin)
}

fn cos<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::cos)
}

fn tan<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::tan)
}

fn asin<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::asin)
}

fn acos<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::acos)
}

fn atan<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::atan)
}

fn sinh<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::sinh)
}

fn cosh<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::cosh)
}

fn tanh<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::tanh)
}

fn asinh<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::asinh)
}

fn acosh<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::acosh)
}

fn atanh<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    math(args, f64::atanh)
}

fn atan2<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    match (args.number(0)?, args.number(1)?) {
        (Some(y), Some(x)) => checked(args, y.atan2(x), "D3153"),
        _ => Ok(Value::Undefined),
    }
}

fn compare<'a>(args: &Args<'a, '_>, test: fn(i64, i64) -> bool) -> Result<Value<'a>> {
    Ok(match (instant(args, 0)?, instant(args, 1)?) {
        (Some(a), Some(b)) => Value::Bool(test(a, b)),
        _ => Value::Undefined,
    })
}

fn after_date<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    compare(args, |a, b| a > b)
}

fn before_date<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    compare(args, |a, b| a < b)
}

fn date_equals<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    compare(args, |a, b| a == b)
}

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Years,
    Months,
    Days,
    Hours,
    Minutes,
    Seconds,
    Milliseconds,
}

fn unit(args: &Args, index: usize) -> Result<Unit> {
    let name = args.string(index)?.ok_or_else(|| args.mismatch(index))?;

    Ok(match name.trim_end_matches('s') {
        "year" => Unit::Years,
        "month" => Unit::Months,
        "day" => Unit::Days,
        "hour" => Unit::Hours,
        "minute" => Unit::Minutes,
        "second" => Unit::Seconds,
        "millisecond" => Unit::Milliseconds,
        _ => return Err(args.error("D3154", format!("Unknown date unit \"{name}\""))),
    })
}

fn date(args: &Args, millis: i64) -> Result<DateTime<Utc>> {
    utc(millis).ok_or_else(|| args.error("D3110", "The timestamp is out of range"))
}

fn date_plus<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let Some(start) = instant(args, 0)? else {
        return Ok(Value::Undefined);
    };
    let amount = args.number(1)?.ok_or_else(|| args.mismatch(1))?.trunc() as i64;
    let time = date(args, start)?;

    // Amounts too large for a duration are out of range rather than a panic.
    let add = |duration: Option<Duration>| {
        duration.and_then(|duration| time.checked_add_signed(duration))
    };
    let shifted = match unit(args, 2)? {
        Unit::Years | Unit::Months => {
            let months = amount.saturating_mul(if unit(args, 2)? == Unit::Years { 12 } else { 1 });
            let months_abs = Months::new(months.unsigned_abs().min(u32::MAX as u64) as u32);
            if months >= 0 {
                time.checked_add_months(months_abs)
            } else {
                time.checked_sub_months(months_abs)
            }
        }
        Unit::Days => add(Duration::try_days(amount)),
        Unit::Hours => add(Duration::try_hours(amount)),
        Unit::Minutes => add(Duration::try_minutes(amount)),
        Unit::Seconds => add(Duration::try_seconds(amount)),
        Unit::Milliseconds => add(Duration::try_milliseconds(amount)),
    }
    .ok_or_else(|| args.error("D3110", "The date is out of range"))?;

    Ok(Value::Number(shifted.timestamp_millis() as f64))
}

fn diff_date<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let (Some(a), Some(b)) = (instant(args, 0)?, instant(args, 1)?) else {
        return Ok(Value::Undefined);
    };
    let unit = unit(args, 2)?;

    let difference = match unit {
        Unit::Years | Unit::Months => {
            let (first, second) = (date(args, a)?, date(args, b)?);
            let mut months = (first.year() - second.year()) as i64 * 12 + first.month() as i64
                - second.month() as i64;
            // A month counts once its day and time are reached.
            let rest = |time: &DateTime<Utc>| {
                (
                    time.day(),
                    time.num_seconds_from_midnight(),
                    time.timestamp_subsec_millis(),
                )
            };
            if months > 0 && rest(&first) < rest(&second) {
                months -= 1;
            } else if months < 0 && rest(&first) > rest(&second) {
                months += 1;
            }
            if unit == Unit::Years {
                months / 12
            } else {
                months
            }
        }
        unit => {
            let millis = a - b;
            let size = match unit {
                Unit::Days => 86_400_000,
                Unit::Hours => 3_600_000,
                Unit::Minutes => 60_000,
                Unit::Seconds => 1000,
                _ => 1,
            };
            millis / size
        }
    };

    Ok(Value::Number(difference as f64))
}

fn has_same_date<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    let (Some(a), Some(b)) = (instant(args, 0)?, instant(args, 1)?) else {
        return Ok(Value::Undefined);
    };
    let (first, second) = (date(args, a)?, date(args, b)?);
    let units = args.array(2).ok_or_else(|| args.mismatch(2))?;

    for item in units {
        let Value::String(name) = item else {
            return Err(args.mismatch(2));
        };
        let same = match name.as_str().trim_end_matches('s') {
            "year" => first.year() == second.year(),
            "month" => first.month() == second.month(),
            "day" => first.day() == second.day(),
            "hour" => first.hour() == second.hour(),
            "minute" => first.minute() == second.minute(),
            "second" => first.second() == second.second(),
            "millisecond" => first.timestamp_subsec_millis() == second.timestamp_subsec_millis(),
            _ => {
                return Err(args.error("D3154", format!("Unknown date unit \"{}\"", name.as_str())));
            }
        };
        if !same {
            return Ok(Value::Bool(false));
        }
    }

    Ok(Value::Bool(true))
}

fn part<'a>(args: &Args<'a, '_>, part: fn(&DateTime<Utc>) -> i64) -> Result<Value<'a>> {
    let Some(millis) = instant(args, 0)? else {
        return Ok(Value::Undefined);
    };
    Ok(Value::Number(part(&date(args, millis)?) as f64))
}

fn year<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.year() as i64)
}

fn month<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.month() as i64)
}

fn day<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.day() as i64)
}

fn hours<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.hour() as i64)
}

fn minutes<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.minute() as i64)
}

fn seconds<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.second() as i64)
}

fn milliseconds<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.timestamp_subsec_millis() as i64)
}

fn day_of_the_week<'a>(args: &Args<'a, '_>) -> Result<Value<'a>> {
    part(args, |time| time.weekday().num_days_from_sunday() as i64)
}
