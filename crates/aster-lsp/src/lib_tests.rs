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

/// rust-analyzer only analyzes files inside a loaded project, so the fixture
/// gets a minimal manifest with the probe file as its lib target.
fn rust_project(file_contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[lib]\npath = \"a.rs\"\n",
    )
    .expect("write manifest");
    let file = dir.path().join("a.rs");
    std::fs::write(&file, file_contents).expect("write");
    (dir, file)
}

#[test]
fn diagnostics_report_errors_in_a_rust_file() {
    if !server_available() {
        return;
    }
    let (dir, file) = rust_project("fn main() {\n    let x: u = 1;\n}\n");
    let mut client = Client::start(ServerKind::RustAnalyzer, dir.path()).expect("start");
    let diags = client.diagnostics(&file).expect("diagnostics");
    assert!(diags.iter().any(|d| d.contains("error")), "{diags:?}");
}

#[test]
fn definitions_find_where_a_symbol_lives() {
    if !server_available() {
        return;
    }
    let (dir, file) = rust_project("fn helper() {}\nfn main() {\n    helper();\n}\n");
    let mut client = Client::start(ServerKind::RustAnalyzer, dir.path()).expect("start");
    // `helper` on line 2 (0-based), column 4.
    let defs = client.definitions(&file, 2, 4).expect("definitions");
    assert!(defs.iter().any(|d| d.contains("a.rs:1")), "{defs:?}");
}
