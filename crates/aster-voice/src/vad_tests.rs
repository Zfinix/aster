use super::*;

const STEP: Duration = Duration::from_millis(50);

/// Feeds `levels` in order and returns the slice index where it stopped.
fn stops_at(levels: impl IntoIterator<Item = f32>) -> Option<usize> {
    let mut endpoint = Endpoint::default();
    levels
        .into_iter()
        .position(|level| endpoint.update(level, STEP))
}

fn slices(level: f32, duration: Duration) -> impl Iterator<Item = f32> {
    std::iter::repeat_n(level, (duration.as_millis() / STEP.as_millis()) as usize)
}

#[test]
fn stops_after_speech_then_silence() {
    let levels = slices(0.002, LEARN)
        .chain(slices(0.2, Duration::from_secs(2)))
        .chain(slices(0.002, Duration::from_secs(3)));
    let stopped = stops_at(levels);
    assert_eq!(stopped, Some(5 + 40 + 30 - 1));
}

#[test]
fn keeps_listening_before_anyone_speaks() {
    assert_eq!(stops_at(slices(0.002, Duration::from_secs(10))), None);
}

#[test]
fn a_short_click_is_not_speech() {
    let levels = slices(0.002, LEARN)
        .chain(slices(0.3, Duration::from_millis(100)))
        .chain(slices(0.002, Duration::from_secs(5)));
    assert_eq!(stops_at(levels), None);
}

#[test]
fn a_pause_mid_sentence_does_not_stop_it() {
    let levels = slices(0.002, LEARN)
        .chain(slices(0.2, Duration::from_secs(1)))
        .chain(slices(0.002, Duration::from_secs(1)))
        .chain(slices(0.2, Duration::from_secs(1)));
    assert_eq!(stops_at(levels), None);
}

#[test]
fn steady_room_noise_is_learned_not_heard() {
    assert_eq!(stops_at(slices(0.03, Duration::from_secs(10))), None);
}

#[test]
fn speech_already_underway_is_not_learned_as_noise() {
    let levels = slices(0.002, Duration::from_millis(50))
        .chain(slices(0.2, Duration::from_secs(1)))
        .chain(slices(0.002, Duration::from_secs(3)));
    assert_eq!(stops_at(levels), Some(1 + 20 + 30 - 1));
}
