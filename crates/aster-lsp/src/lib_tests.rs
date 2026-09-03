use std::path::Path;

use crate::{Client, ServerKind, installed, supported};

fn server_available() -> bool {
    installed(ServerKind::RustAnalyzer)
}

#[test]
fn extension_maps_to_servers() {
    assert_eq!(supported(Path::new("a.rs")), Some(ServerKind::RustAnalyzer));
    assert_eq!(
        supported(Path::new("a.ts")),
        Some(ServerKind::TypeScriptLanguageServer)
    );
    assert_eq!(supported(Path::new("a.md")), None);
}

#[test]
fn uri_round_trip_keeps_the_path() {
    let uri = crate::path_to_uri(Path::new("/tmp/x/a.rs"));
    assert!(uri.starts_with("file://"), "{uri}");
}

#[test]
fn diagnostics_report_errors_in_a_rust_file() {
    if !server_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "fn main() {\n    let x: u = 1;\n}\n").expect("write");
    let mut client = Client::start(ServerKind::RustAnalyzer, dir.path()).expect("start");
    let diags = client.diagnostics(&file).expect("diagnostics");
    assert!(diags.iter().any(|d| d.contains("error")), "{diags:?}");
}

#[test]
fn definitions_find_where_a_symbol_lives() {
    if !server_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "fn helper() {}\nfn main() {\n    helper();\n}\n").expect("write");
    let mut client = Client::start(ServerKind::RustAnalyzer, dir.path()).expect("start");
    // `helper` on line 2 (0-based), column 4.
    let defs = client.definitions(&file, 2, 4).expect("definitions");
    assert!(defs.iter().any(|d| d.contains("a.rs:1")), "{defs:?}");
}
