use super::*;

use std::io::Write;

fn session(dir: &Path, name: &str, created: &str, model: &str) {
    let mut file = std::fs::File::create(dir.join(name)).unwrap();
    writeln!(
        file,
        r#"{{"type":"session","id":"{name}","v":1,"created_at":"{created}","cwd":"/r","repo_root":"/r","model":"{model}"}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"message","role":"user","content":"go","ts":"{created}"}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"message","role":"assistant","content":"answer","ts":"{created}"}}"#
    )
    .unwrap();
}

#[test]
fn analyze_walks_nested_project_directories() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project-a");
    std::fs::create_dir(&project).unwrap();
    session(&project, "one.jsonl", "2026-08-03T09:00:00Z", "test/model");
    session(
        root.path(),
        "two.jsonl",
        "2026-08-03T09:00:00Z",
        "test/model",
    );

    let report = analyze(root.path(), &Filter::default()).unwrap();
    assert_eq!(report.sessions, 2);
    assert_eq!(report.turns, 2);
}

#[test]
fn non_jsonl_files_are_ignored() {
    let root = tempfile::tempdir().unwrap();
    session(
        root.path(),
        "one.jsonl",
        "2026-08-03T09:00:00Z",
        "test/model",
    );
    std::fs::write(root.path().join("notes.txt"), "not a transcript").unwrap();
    assert_eq!(
        analyze(root.path(), &Filter::default()).unwrap().sessions,
        1
    );
}

#[test]
fn the_model_filter_keeps_only_matching_sessions() {
    let root = tempfile::tempdir().unwrap();
    session(root.path(), "one.jsonl", "2026-08-03T09:00:00Z", "kimi");
    session(root.path(), "two.jsonl", "2026-08-03T09:00:00Z", "glm");
    let filter = Filter {
        model: Some("kimi".into()),
        ..Filter::default()
    };
    assert_eq!(analyze(root.path(), &filter).unwrap().sessions, 1);
}

#[test]
fn the_since_filter_drops_older_sessions() {
    let root = tempfile::tempdir().unwrap();
    session(
        root.path(),
        "old.jsonl",
        "2020-01-01T00:00:00Z",
        "test/model",
    );
    let recent = Utc::now().to_rfc3339();
    session(root.path(), "new.jsonl", &recent, "test/model");
    assert_eq!(
        analyze(root.path(), &Filter::since_days(7))
            .unwrap()
            .sessions,
        1
    );
}
