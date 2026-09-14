use super::resolve_repo_path;

#[test]
fn a_file_in_the_repository_resolves() {
    let path = std::env::current_dir().unwrap().join("Cargo.toml");
    assert!(resolve_repo_path(path.to_str().unwrap()).is_ok());
}

/// The phone writes every screenshot to its temp directory, so a picture there
/// is the agent's own work, not somebody else's file.
#[test]
fn a_file_in_the_temp_directory_resolves() {
    let path = std::env::temp_dir().join(format!("aster-send-{}.png", std::process::id()));
    std::fs::write(&path, b"not really a png").unwrap();
    let resolved = resolve_repo_path(path.to_str().unwrap());
    let _ = std::fs::remove_file(&path);
    assert!(resolved.is_ok(), "{resolved:?}");
}

#[test]
fn a_file_somewhere_else_is_refused() {
    assert!(resolve_repo_path("/etc/hosts").is_err());
}
