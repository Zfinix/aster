use crate::launchd;
use crate::schedule::calendar_intervals;

#[test]
fn plist_renders_program_and_interval() {
    let intervals = calendar_intervals("0 9 * * *").unwrap();
    let plist = launchd::render(
        "nightly-review",
        &intervals,
        &[
            "/usr/local/bin/aster".into(),
            "run".into(),
            "scout".into(),
            "look".into(),
            "--json".into(),
        ],
        std::path::Path::new("/repo"),
        std::path::Path::new("/home/u/.aster/cron/nightly-review.log"),
    );
    assert!(plist.contains("<string>com.aster.cron.nightly-review</string>"));
    assert!(plist.contains("<key>Hour</key><integer>9</integer>"));
    assert!(plist.contains("<string>/usr/local/bin/aster</string>"));
    assert!(plist.contains("<string>/repo</string>"));
}

#[test]
fn plist_escapes_xml() {
    let intervals = calendar_intervals("0 9 * * *").unwrap();
    let plist = launchd::render(
        "esc",
        &intervals,
        &["a<b&c>".into()],
        std::path::Path::new("/repo"),
        std::path::Path::new("/log"),
    );
    assert!(plist.contains("a&lt;b&amp;c&gt;"));
    assert!(!plist.contains("a<b&c>"));
}
