use std::time::Duration;

use chrono::{Local, NaiveDate};

use crate::history_panel::{day_label, short_address, until_midnight};

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[test]
fn names_recent_days_and_dates() {
    let today = date(2026, 3, 1);

    assert_eq!(day_label(today, today), "Today");
    assert_eq!(day_label(date(2026, 2, 28), today), "Yesterday");
    assert_eq!(day_label(date(2026, 2, 27), today), "February 27");
    assert_eq!(day_label(date(2025, 12, 31), today), "December 31, 2025");
}

#[test]
fn shortens_addresses_by_their_scheme() {
    assert_eq!(short_address("https://api.test/users"), "api.test/users");
    assert_eq!(
        short_address("grpc://grpcb.in:9000/hello.HelloService/SayHello"),
        "grpcb.in:9000/hello.HelloService/SayHello"
    );
    assert_eq!(
        short_address("{{host}}/users?next=https://a.test"),
        "{{host}}/users?next=https://a.test"
    );
    assert_eq!(
        short_address("{{scheme}}://api.test"),
        "{{scheme}}://api.test"
    );
    assert_eq!(short_address(""), "");
}

#[test]
fn wakes_just_after_midnight() {
    let now = Local::now();
    let wait = until_midnight();
    let woken = now + chrono::Duration::from_std(wait).unwrap();

    assert!(wait <= Duration::from_secs(25 * 60 * 60));
    assert!(woken.date_naive() > now.date_naive());
}
