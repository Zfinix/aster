use super::*;

/// The bug this replaced: on Android every app's storage ends in `files`, so a
/// basename told the user nothing about where the agent would work.
#[test]
fn android_storage_is_not_reported_as_files() {
    let name = workspace_name(std::path::Path::new("/data/user/0/dev.aster.probe/files"));
    assert_eq!(name, "/data/user/0/dev.aster.probe/files");
    assert_ne!(name, "files");
}

#[test]
fn the_home_prefix_folds_to_a_tilde() {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap();
    assert_eq!(
        workspace_name(&home.join("projects/aster")),
        "~/projects/aster"
    );
    assert_eq!(workspace_name(&home), "~");
}

#[test]
fn a_path_outside_home_stays_whole() {
    assert_eq!(workspace_name(std::path::Path::new("/srv/api")), "/srv/api");
}

#[test]
fn approval_view_unwraps_the_run_preview() {
    let view = ApprovalView::new("run `git status`");
    assert_eq!(view.subject, "git status");
    assert_eq!(view.rule, None);
    assert_eq!(view.text(), "git status");
}

#[test]
fn approval_view_puts_the_rule_on_its_own_line() {
    let view = ApprovalView::new("run `rm -rf dist` (risky command)");
    assert_eq!(view.subject, "rm -rf dist");
    assert_eq!(view.rule.as_deref(), Some("risky command"));
    assert_eq!(view.text(), "rm -rf dist\nRule: risky command");
}

#[test]
fn approval_view_keeps_the_verb_on_a_path() {
    let view = ApprovalView::new("edit src/lib.rs (protected path)");
    assert_eq!(view.text(), "edit src/lib.rs\nRule: protected path");
}

#[test]
fn approval_view_keeps_a_long_script_whole() {
    let script = (0..300)
        .map(|n| format!("echo step {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let view = ApprovalView::new(&format!("run `bash -c \"{script}\"` (git push*)"));
    assert_eq!(view.subject, format!("bash -c \"{script}\""));
    assert_eq!(view.rule.as_deref(), Some("git push*"));
}

#[test]
fn approval_view_passes_other_previews_through() {
    let preview = "`cat` needs to read credentials outside the repository:\n  ~/.ssh (keys)";
    assert_eq!(ApprovalView::new(preview).text(), preview);
}

#[test]
fn verbatim_chunks_move_on_when_the_limit_is_under_one_character() {
    assert_eq!(verbatim_chunks("éé", 1), vec!["é", "é"]);
}

#[test]
fn approval_view_keeps_brackets_inside_the_rule() {
    let view = ApprovalView::new("run `git push` (Bash(git push (any):*))");
    assert_eq!(view.subject, "git push");
    assert_eq!(view.rule.as_deref(), Some("Bash(git push (any):*)"));
}
