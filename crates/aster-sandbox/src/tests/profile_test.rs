use super::*;

#[test]
fn profile_defaults() {
    let p = SandboxProfile::new(Path::new("/repo"));
    assert_eq!(p.repo_root, PathBuf::from("/repo"));
    assert!(p.readable_dirs.is_empty());
    assert!(!p.network);
    assert_eq!(p.timeout_secs, 30);
}

#[test]
fn profile_builder() {
    let p = SandboxProfile::new(Path::new("/repo"))
        .readable(PathBuf::from("/opt"))
        .writable(PathBuf::from("/tmp/out"))
        .network(true)
        .timeout(60);
    assert_eq!(p.readable_dirs, vec![PathBuf::from("/opt")]);
    assert_eq!(p.writable_dirs, vec![PathBuf::from("/tmp/out")]);
    assert!(p.network);
    assert_eq!(p.timeout_secs, 60);
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_profile_contains_repo() {
    let repo = tempfile::tempdir().unwrap();
    let p = SandboxProfile::new(repo.path());
    let profile = p.seatbelt_profile();
    let root = repo.path().canonicalize().unwrap();
    assert!(profile.contains(&root.display().to_string()), "{profile}");
    assert!(profile.contains("(allow default)"));
    assert!(profile.contains("(deny file-write*)"), "{profile}");
    assert!(profile.contains("(deny network*)"), "{profile}");
}

/// Seatbelt rejects the whole profile when a `subpath` cannot be resolved,
/// and on macOS `/tmp` and `/var` are symlinks into `/private`.
#[cfg(target_os = "macos")]
#[test]
fn seatbelt_writable_paths_are_resolved_and_existing() {
    let repo = tempfile::tempdir().unwrap();
    let p = SandboxProfile::new(repo.path()).writable(PathBuf::from("/nope/missing"));
    let profile = p.seatbelt_profile();
    assert!(!profile.contains("\"/tmp\""), "{profile}");
    assert!(profile.contains("/private/tmp"), "{profile}");
    assert!(!profile.contains("/nope/missing"), "{profile}");
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_profile_protects_git_dirs() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".git/hooks")).unwrap();
    std::fs::create_dir_all(repo.path().join(".github/workflows")).unwrap();
    std::fs::write(repo.path().join(".git/config"), "").unwrap();
    let p = SandboxProfile::new(repo.path());
    let profile = p.seatbelt_profile();
    assert!(profile.contains(".git/hooks"), "{profile}");
    assert!(profile.contains(".git/config"), "{profile}");
    assert!(profile.contains(".github/workflows"), "{profile}");
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_profile_allows_network_when_requested() {
    let p = SandboxProfile::new(Path::new("/repo")).network(true);
    let profile = p.seatbelt_profile();
    assert!(!profile.contains("(deny network*)"), "{profile}");
}

#[cfg(target_os = "linux")]
#[test]
fn bwrap_args_contain_repo() {
    let p = SandboxProfile::new(Path::new("/repo"));
    let args = p.bwrap_args();
    assert!(args.contains(&"--bind".to_string()));
    assert!(args.contains(&"/repo".to_string()));
    assert!(args.contains(&"--unshare-net".to_string()));
}

#[test]
fn credentials_are_matched_by_command_name_not_path() {
    assert_eq!(command_name("/opt/homebrew/bin/gh"), "gh");
    assert_eq!(command_name("gh.exe"), "gh");
    assert_eq!(command_name("gh"), "gh");
}

#[test]
fn only_the_tools_that_need_a_credential_dir_ask_for_one() {
    assert!(credentials_for("cat", &[]).is_empty());
    assert!(credentials_for("curl", &["https://example.com".into()]).is_empty());
    assert!(credentials_for("rg", &["token".into()]).is_empty());
}

#[test]
fn git_asks_only_when_it_reaches_a_remote() {
    let sub = |s: &str| vec![s.to_string()];
    // Local plumbing never needs a key.
    assert!(credentials_for("git", &sub("status")).is_empty());
    assert!(credentials_for("git", &sub("diff")).is_empty());
    assert!(credentials_for("git", &sub("log")).is_empty());
    // Flags before the subcommand must not hide it.
    let flagged = vec!["-C".to_string(), "/repo".to_string(), "status".to_string()];
    assert!(credentials_for("git", &flagged).is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn an_approved_credential_dir_leaves_the_deny_list() {
    let Some(home) = dirs::home_dir() else { return };
    let gh = home.join(".config/gh");
    if !gh.exists() {
        return;
    }
    let repo = std::env::temp_dir();
    let denied = SandboxProfile::new(&repo).seatbelt_profile();
    assert!(denied.contains(".config/gh"), "denied by default");

    let allowed = SandboxProfile::new(&repo)
        .allow_credentials(vec![gh.clone()])
        .seatbelt_profile();
    assert!(!allowed.contains(".config/gh"), "{allowed}");
}

#[cfg(target_os = "macos")]
#[test]
fn keychains_can_never_be_approved() {
    let Some(home) = dirs::home_dir() else { return };
    let keychains = home.join("Library/Keychains");
    if !keychains.exists() {
        return;
    }
    let profile = SandboxProfile::new(&std::env::temp_dir())
        .allow_credentials(vec![keychains])
        .seatbelt_profile();
    assert!(profile.contains("Keychains"), "{profile}");
}

#[cfg(target_os = "macos")]
#[test]
fn approving_one_credential_dir_leaves_the_others_denied() {
    let Some(home) = dirs::home_dir() else { return };
    let (gh, ssh) = (home.join(".config/gh"), home.join(".ssh"));
    if !gh.exists() || !ssh.exists() {
        return;
    }
    let profile = SandboxProfile::new(&std::env::temp_dir())
        .allow_credentials(vec![gh])
        .seatbelt_profile();
    assert!(!profile.contains(".config/gh"));
    assert!(profile.contains(".ssh"), "{profile}");
}
