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
