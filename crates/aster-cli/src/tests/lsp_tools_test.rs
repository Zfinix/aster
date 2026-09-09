use std::time::{Duration, Instant};

use crate::lsp_tools::after_edit;

fn server_available() -> bool {
    aster_lsp::installed(aster_lsp::ServerKind::RustAnalyzer)
}

/// The first edit only starts the server, so poll until the warmed one
/// answers.
fn wait_for_report(root: &std::path::Path) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(report) = after_edit(root, "a.rs") {
            return Some(report);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn an_edit_reports_the_problems_it_left_behind() {
    if !server_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[lib]\npath = \"a.rs\"\n",
    )
    .expect("write manifest");
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "pub fn f() {\n    let x: u = 1;\n}\n").expect("write");

    let report = wait_for_report(dir.path()).expect("a report before the deadline");
    assert!(report.starts_with("The edit was applied."), "{report}");
    assert!(report.contains("error 2:"), "{report}");

    std::fs::write(&file, "pub fn f() -> u32 {\n    1\n}\n").expect("rewrite");
    assert_eq!(after_edit(dir.path(), "a.rs"), None);
}
