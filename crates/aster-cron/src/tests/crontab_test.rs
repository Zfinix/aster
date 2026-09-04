use crate::crontab;

#[test]
fn line_carries_marker() {
    let line = crontab::line("nightly", "0 9 * * *", "aster run scout look --json");
    assert!(line.starts_with("0 9 * * * aster run scout look --json"));
    assert!(line.contains("# ASTER-CRON:nightly"));
}
