use super::*;

fn store(dir: &Path) -> GrantStore {
    GrantStore::new(dir.join("grants").join("proj.json"))
}

#[test]
fn missing_file_reads_as_no_grants() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(store(home.path()).load(), Vec::<PathBuf>::new());
}

#[test]
fn added_directories_survive_a_reload() {
    let home = tempfile::tempdir().unwrap();
    let s = store(home.path());
    s.add(Path::new("/Users/x/Desktop")).unwrap();
    s.add(Path::new("/Users/x/Downloads")).unwrap();

    assert_eq!(
        store(home.path()).load(),
        [
            PathBuf::from("/Users/x/Desktop"),
            PathBuf::from("/Users/x/Downloads")
        ]
    );
}

#[test]
fn adding_the_same_directory_twice_is_idempotent() {
    let home = tempfile::tempdir().unwrap();
    let s = store(home.path());
    s.add(Path::new("/Users/x/Desktop")).unwrap();
    s.add(Path::new("/Users/x/Desktop")).unwrap();
    assert_eq!(s.load(), [PathBuf::from("/Users/x/Desktop")]);
}

#[test]
fn corrupt_file_reads_as_no_grants() {
    let home = tempfile::tempdir().unwrap();
    let s = store(home.path());
    s.add(Path::new("/Users/x/Desktop")).unwrap();
    std::fs::write(&s.path, "{not json").unwrap();
    assert_eq!(s.load(), Vec::<PathBuf>::new());
}
