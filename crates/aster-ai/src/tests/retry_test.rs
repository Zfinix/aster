use super::*;

#[test]
fn reset_delay_reads_epoch_milliseconds_as_a_short_wait() {
    let now = 1_760_000_000;
    // OpenRouter's form: epoch ms, two seconds out.
    let delay = reset_delay(now * 1_000 + 2_000, now).expect("a delay");
    assert_eq!(delay, Duration::from_secs(2));
}

#[test]
fn reset_delay_reads_epoch_seconds() {
    let now = 1_760_000_000;
    let delay = reset_delay(now + 5, now).expect("a delay");
    assert_eq!(delay, Duration::from_secs(5));
}

#[test]
fn reset_delay_treats_a_small_value_as_relative_seconds() {
    let delay = reset_delay(3, 1_760_000_000).expect("a delay");
    assert_eq!(delay, Duration::from_secs(3));
}

#[test]
fn reset_delay_ignores_a_reset_in_the_past() {
    assert!(reset_delay(1_700_000_000, 1_760_000_000).is_none());
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.insert(
            reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    h
}

#[test]
fn retry_after_seconds_is_honored() {
    let d = header_delay(&headers(&[("retry-after", "5")])).unwrap();
    assert_eq!(d, Duration::from_secs(5));
}

#[test]
fn ratelimit_reset_waits_until_reset() {
    let future = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 30;
    let d = header_delay(&headers(&[("x-ratelimit-reset", &future.to_string())])).unwrap();
    assert!(d.as_secs() > 25 && d.as_secs() <= 30);
}

#[test]
fn http_date_retry_after_falls_through_to_none() {
    let d = header_delay(&headers(&[(
        "retry-after",
        "Wed, 21 Oct 2026 07:28:00 GMT",
    )]));
    assert!(d.is_none());
}

#[test]
fn retry_hint_distinguishes_rate_limit_from_permanent_error() {
    assert!(has_retry_hint(&headers(&[("retry-after", "60")])));
    assert!(has_retry_hint(&headers(&[("x-ratelimit-remaining", "0")])));
    assert!(has_retry_hint(&headers(&[(
        "x-ratelimit-reset-requests",
        "1s"
    )])));
    assert!(!has_retry_hint(&headers(&[])));
    assert!(!has_retry_hint(&headers(&[(
        "content-type",
        "application/json"
    )])));
}
