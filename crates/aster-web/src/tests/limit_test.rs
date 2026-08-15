#![cfg(test)]

use super::*;

#[tokio::test]
async fn calls_within_capacity_do_not_wait() {
    let limit = RateLimit::new(3);
    let start = Instant::now();
    for _ in 0..3 {
        limit.acquire().await;
    }
    assert!(start.elapsed() < Duration::from_millis(50));
}

#[tokio::test(start_paused = true)]
async fn the_call_past_capacity_waits_for_the_window_to_roll() {
    let limit = RateLimit::new(2);
    limit.acquire().await;
    limit.acquire().await;

    let start = Instant::now();
    limit.acquire().await;
    assert!(start.elapsed() >= WINDOW, "{:?}", start.elapsed());
}

#[tokio::test(start_paused = true)]
async fn slots_are_reusable_once_they_age_out() {
    let limit = RateLimit::new(1);
    limit.acquire().await;
    tokio::time::sleep(WINDOW + Duration::from_secs(1)).await;

    let start = Instant::now();
    limit.acquire().await;
    assert!(start.elapsed() < Duration::from_millis(50));
}
