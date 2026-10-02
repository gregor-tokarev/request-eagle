use serde_json::{Value, json};

use crate::{Bindings, evaluate};

fn check(expression: &str, expected: Value) {
    let result = evaluate(expression, Some(&json!({})), &Bindings::default())
        .unwrap_or_else(|error| panic!("{expression}: {error}"));
    assert_eq!(result, Some(expected), "{expression}");
}

fn code(expression: &str) -> &'static str {
    evaluate(expression, Some(&json!({})), &Bindings::default())
        .unwrap_err()
        .code
}

#[test]
fn formats_milliseconds_with_xpath_pictures() {
    check(
        "$fromMillis(1510067557121)",
        json!("2017-11-07T15:12:37.121Z"),
    );
    check(
        "$fromMillis(1510067557121, '[M01]/[D01]/[Y0001] [h#1]:[m01][P]')",
        json!("11/07/2017 3:12pm"),
    );
    check(
        "$fromMillis(1510067557121, '[H01]:[m01]:[s01] [z]', '-0500')",
        json!("10:12:37 GMT-05:00"),
    );
    check(
        "$fromMillis(1510067557121, '[FNn], [D1o] [MNn] [Y]')",
        json!("Tuesday, 7th November 2017"),
    );
    check(
        "$fromMillis(1510067557121, '[MNn,*-3] [Y01]')",
        json!("Nov 17"),
    );
    check(
        "$fromMillis(0, '[Y0001]-[M01]-[D01]T[H01]:[m01]:[s01][Z01:01]', '+0530')",
        json!("1970-01-01T05:30:00+05:30"),
    );
    check("$fromMillis(1510067557121, '[[Y]] [Y]')", json!("[Y] 2017"));
    assert_eq!(code("$fromMillis(0, '[Q]')"), "D3132");
    assert_eq!(code("$fromMillis(0, undefined, 'nope')"), "D3134");
}

#[test]
fn formats_milliseconds_with_unicode_patterns_as_postman_does() {
    check(
        "$fromMillis(1521801216617, 'dd/M/yyyy')",
        json!("23/3/2018"),
    );
    check(
        "$fromMillis(1521801216617, 'yyyy-MM-dd HH:mm:ss.SSS')",
        json!("2018-03-23 10:33:36.617"),
    );
    check(
        "$fromMillis(1521801216617, 'EEEE, MMMM d')",
        json!("Friday, March 23"),
    );
    check(
        "$fromMillis(1521801216617, \"h:mm a 'on' EEE\")",
        json!("10:33 AM on Fri"),
    );
}

#[test]
fn reads_timestamps() {
    check(
        "$toMillis('2017-11-07T15:07:54.972Z')",
        json!(1510067274972_i64),
    );
    check("$toMillis('2018-03-27')", json!(1522108800000_i64));
    check(
        "$toMillis('2018-03-27T10:00:00+01:00')",
        json!(1522141200000_i64),
    );
    check(
        "$toMillis('2018-03-27', 'yyyy-MM-dd')",
        json!(1522108800000_i64),
    );
    check(
        "$toMillis('27/03/2018', '[D01]/[M01]/[Y0001]')",
        json!(1522108800000_i64),
    );
    check(
        "$toMillis('March 27, 2018', '[MNn] [D], [Y]')",
        json!(1522108800000_i64),
    );
    check(
        "$toMillis('2018-03-27 3:05 pm', '[Y]-[M01]-[D01] [h]:[m01] [P]')",
        json!(1522163100000_i64),
    );
    assert_eq!(code("$toMillis('yesterday')"), "D3110");
    assert_eq!(
        code("$toMillis('2018/03/27', '[Y0001]-[M01]-[D01]')"),
        "D3110"
    );
}

#[test]
fn now_and_millis_are_the_same_instant_in_one_evaluation() {
    check("$now() = $now()", json!(true));
    check("$toMillis($now()) = $millis()", json!(true));
    check("$millis() > 1700000000000", json!(true));
}

#[test]
fn postman_date_arithmetic() {
    check(
        "$datePlus('2024-01-31T00:00:00Z', 1, 'months')",
        json!(1709164800000_i64),
    );
    check(
        "$fromMillis($datePlus('2024-01-31', 1, 'month'))",
        json!("2024-02-29T00:00:00.000Z"),
    );
    check(
        "$fromMillis($datePlus(0, -1, 'days'))",
        json!("1969-12-31T00:00:00.000Z"),
    );
    check(
        "$fromMillis($datePlus('2024-02-29', 1, 'years'))",
        json!("2025-02-28T00:00:00.000Z"),
    );
    check("$diffDate('2024-03-01', '2024-01-31', 'months')", json!(1));
    check("$diffDate('2024-03-01', '2024-01-31', 'days')", json!(30));
    check("$diffDate('2024-01-01', '2025-06-01', 'years')", json!(-1));
    check("$afterDate('2024-05-02', '2024-05-01')", json!(true));
    check("$beforeDate(0, '1970-01-02')", json!(true));
    check("$dateEquals('1970-01-01T00:00:00Z', 0)", json!(true));
    check(
        "$hasSameDate('2024-05-01T10:00:00Z', '2024-05-01T23:00:00Z', ['year', 'month', 'day'])",
        json!(true),
    );
    check(
        "$hasSameDate('2024-05-01T10:00:00Z', '2024-05-01T23:00:00Z', ['hours'])",
        json!(false),
    );
    check(
        "[$year(0), $month(0), $day(0), $hours(0), $dayOfTheWeek('2024-05-05')]",
        json!([1970, 1, 1, 0, 0]),
    );
    check("$milliSeconds(1510067557121)", json!(121));
    assert_eq!(code("$datePlus(0, 1, 'fortnights')"), "D3154");
}
