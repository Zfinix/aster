use super::*;

#[test]
fn merge_keeps_what_the_new_home_already_has() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    fs::create_dir_all(old.join("sessions")).unwrap();
    fs::write(old.join("credentials.json"), "old").unwrap();
    fs::write(old.join("sessions/a.jsonl"), "a").unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(new.join("credentials.json"), "new").unwrap();

    merge_dir(&old, &new).unwrap();

    assert_eq!(
        fs::read_to_string(new.join("credentials.json")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(new.join("sessions/a.jsonl")).unwrap(),
        "a"
    );
    assert!(old.exists(), "the old directory is left alone");
}

#[test]
fn migration_moves_data_but_leaves_config_behind() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    fs::create_dir_all(old.join("sessions")).unwrap();
    fs::write(old.join("sessions/a.jsonl"), "a").unwrap();
    fs::write(old.join("credentials.json"), "cred").unwrap();
    fs::write(old.join("aster.yaml"), "review: {}").unwrap();
    fs::write(old.join("mcp.json"), "{}").unwrap();

    migrate_data(&old, &new).unwrap();

    assert!(new.join("sessions/a.jsonl").exists());
    assert!(new.join("credentials.json").exists());
    assert!(!new.join("aster.yaml").exists(), "config stays in ~/.aster");
    assert!(!new.join("mcp.json").exists());
}
