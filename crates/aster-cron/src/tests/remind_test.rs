use crate::remind::{fire_args, one_shot_cron, parse_when};
use chrono::{Datelike, Timelike};

#[test]
fn parses_durations() {
    let t = parse_when("in 30m").unwrap();
    assert!(t > chrono::Local::now());
    let t = parse_when("in 10s").unwrap();
    assert!(t > chrono::Local::now());
    assert!(parse_when("in 0m").is_err());
    assert!(parse_when("in 5x").is_err());
    assert!(parse_when("whenever").is_err());
}

#[test]
fn at_time_is_future() {
    let t = parse_when("at 00:01").unwrap();
    assert!(t > chrono::Local::now());
    assert!(parse_when("at 25:00").is_err());
    assert!(parse_when("at 9pm").is_err());
}

#[test]
fn one_shot_pins_every_field_but_year() {
    let t = parse_when("in 90m").unwrap();
    let cron = one_shot_cron(t);
    let fields: Vec<&str> = cron.split_whitespace().collect();
    assert_eq!(fields[0], t.minute().to_string());
    assert_eq!(fields[1], t.hour().to_string());
    assert_eq!(fields[2], t.day().to_string());
    assert_eq!(fields[3], t.month().to_string());
    assert_eq!(fields[4], "*");
}

#[test]
fn fire_args_remove_themselves() {
    let args = fire_args("/bin/aster", "remind-123", "stand up");
    assert_eq!(args[1], "remind");
    assert!(args.contains(&"--fire".to_string()));
    assert!(args.contains(&"remind-123".to_string()));
}
