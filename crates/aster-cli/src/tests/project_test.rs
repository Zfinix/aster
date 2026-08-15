use super::*;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn a_snapshot_names_the_repo_and_what_its_readme_claims() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "README.md",
        "# Aster\n\n[![ci](badge.svg)](ci)\n\nAster reviews code and edits it.\nIt runs locally.\n\nSecond paragraph.\n",
    );

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(note.starts_with("## Project\n"), "{note}");
    assert!(
        note.contains("- About: Aster reviews code and edits it. It runs locally.\n"),
        "{note}"
    );
    assert!(!note.contains("Second paragraph"), "{note}");
    assert!(!note.contains("badge"), "{note}");
}

#[test]
fn a_readme_is_found_however_the_repo_capitalizes_it() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Readme",
        "A build tool for nothing in particular.\n",
    );

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("- About: A build tool for nothing in particular.\n"),
        "{note}"
    );
}

#[test]
fn without_a_readme_the_manifest_says_what_this_is() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"thing\"\ndescription = \"A thing that does things\"\n",
    );

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("- About: A thing that does things\n"),
        "{note}"
    );
}

#[test]
fn a_decoration_only_readme_falls_through_to_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "README.md",
        "# thing\n\n<img src=\"logo.png\">\n",
    );
    write(
        dir.path(),
        "package.json",
        "{\"description\": \"A thing that does things\"}",
    );

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("- About: A thing that does things\n"),
        "{note}"
    );
}

#[test]
fn a_readme_of_nothing_but_decoration_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "README.md",
        "# Title\n\n<p align=\"center\">art</p>\n",
    );

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(!note.contains("- About:"), "{note}");
}

#[test]
fn an_ecosystem_reports_its_size_before_its_packages() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Cargo.toml", "[workspace]\n");
    write(dir.path(), "crates/one/Cargo.toml", "[package]\n");
    write(dir.path(), "crates/one/src/lib.rs", "pub fn a() {}");
    write(dir.path(), "crates/two/Cargo.toml", "[package]\n");
    write(dir.path(), "crates/two/src/lib.rs", "pub fn b() {}");
    write(dir.path(), "crates/three/Cargo.toml", "[package]\n");
    write(dir.path(), "crates/three/src/lib.rs", "pub fn c() {}");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("- Rust: 3 files, 3 packages under crates/ (one, three, two)\n"),
        "{note}"
    );
}

#[test]
fn packages_that_share_no_parent_are_named_where_they_sit() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "desktop/package.json", "{}");
    write(dir.path(), "desktop/App.tsx", "export default 1");
    write(dir.path(), "editors/vscode/package.json", "{}");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("- JavaScript/TypeScript: 1 file, packages in desktop, editors/vscode\n"),
        "{note}"
    );
}

#[test]
fn a_package_outside_the_workspace_is_named_after_it() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Cargo.toml", "[workspace]\n");
    write(dir.path(), "crates/one/Cargo.toml", "[package]\n");
    write(dir.path(), "crates/two/Cargo.toml", "[package]\n");
    write(dir.path(), "crates/three/Cargo.toml", "[package]\n");
    write(dir.path(), "desktop/src-tauri/Cargo.toml", "[package]\n");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("3 packages under crates/ (one, three, two), plus desktop/src-tauri\n"),
        "{note}"
    );
}

#[test]
fn a_language_with_a_handful_of_files_is_not_a_stack() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..25 {
        write(dir.path(), &format!("src/m{i:02}.rs"), "");
    }
    write(dir.path(), "scripts/release.sh", "");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(note.contains("- Rust: 25 files\n"), "{note}");
    assert!(!note.contains("Shell"), "{note}");
}

#[test]
fn the_biggest_ecosystem_leads_and_the_tail_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.rb", "");
    write(dir.path(), "b.py", "");
    write(dir.path(), "c.py", "");
    write(dir.path(), "d.go", "");
    write(dir.path(), "e.go", "");
    write(dir.path(), "f.go", "");
    write(dir.path(), "g.sh", "");
    write(dir.path(), "h.sh", "");
    write(dir.path(), "i.sh", "");
    write(dir.path(), "j.sh", "");
    write(dir.path(), "k.sql", "");

    let note = snapshot(dir.path()).expect("a snapshot");
    let stacks: Vec<&str> = note
        .lines()
        .filter(|l| l.contains(" files") || l.contains(" file,") || l.ends_with(" file"))
        .collect();
    assert_eq!(stacks.len(), MAX_STACKS, "{note}");
    assert!(stacks[0].starts_with("- Shell: 4 files"), "{note}");
    assert!(!note.contains("SQL"), "{note}");
}

#[test]
fn a_wide_monorepo_is_summarized_rather_than_listed_in_full() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..MAX_PACKAGE_NAMES + 3 {
        write(
            dir.path(),
            &format!("crates/c{i:02}/Cargo.toml"),
            "[package]\n",
        );
    }

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(note.contains("9 packages under crates/"), "{note}");
    assert!(note.contains("and 3 more"), "{note}");
}

#[test]
fn the_docs_directory_is_listed_by_page() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/ARCHITECTURE.md", "# how it fits");
    write(dir.path(), "docs/CONFIG.md", "# every key");
    write(dir.path(), "docs/diagram.png", "");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(
        note.contains("- Docs in docs/: ARCHITECTURE.md, CONFIG.md\n"),
        "{note}"
    );
    assert!(!note.contains("diagram.png"), "{note}");
}

#[test]
fn an_ignored_directory_is_not_part_of_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".gitignore", "generated\n");
    write(dir.path(), "generated/thing.rs", "");
    write(dir.path(), "src/main.rs", "fn main() {}");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(!note.contains("generated"), "{note}");
    assert!(note.contains("- Rust: 1 file\n"), "{note}");
    assert!(note.contains("- Top level: src\n"), "{note}");
}

#[test]
fn a_dependency_directory_is_skipped_even_when_the_repo_tracks_it() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "node_modules/left-pad/package.json", "{}");
    write(dir.path(), "node_modules/left-pad/index.js", "");
    write(dir.path(), "app/package.json", "{}");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(!note.contains("node_modules"), "{note}");
    assert!(!note.contains("file"), "{note}");
    assert!(
        note.contains("- JavaScript/TypeScript: packages in app\n"),
        "{note}"
    );
}

#[test]
fn with_nothing_to_describe_it_the_layout_still_stands() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/main.rs", "fn main() {}");
    write(dir.path(), "Cargo.toml", "[package]\nname = \"thing\"\n");

    let note = snapshot(dir.path()).expect("a snapshot");
    assert!(!note.contains("- About:"), "{note}");
    assert!(note.contains("- Name: "), "{note}");
    assert!(note.contains("- Rust: 1 file\n"), "{note}");
    assert!(note.contains("- Top level: src\n"), "{note}");
}
