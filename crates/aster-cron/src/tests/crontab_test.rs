use crate::crontab;

#[test]
fn line_carries_marker() {
    let line = crontab::line("nightly", "0 9 * * *", "aster run scout look --json");
    assert!(line.starts_with("0 9 * * * aster run scout look --json"));
    assert!(line.contains("# ASTER-CRON:nightly"));
}

#[test]
fn a_machine_without_cron_says_so() {
    let absent = std::io::Error::from(std::io::ErrorKind::NotFound);
    let msg = crontab::missing_cron(absent).to_string();
    assert!(msg.contains("no cron"), "{msg}");
    assert!(msg.contains("crontab"), "{msg}");

    let other = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
    assert_eq!(crontab::missing_cron(other).to_string(), "running crontab");
}
