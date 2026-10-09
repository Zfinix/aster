use super::*;

#[test]
fn parse_markdown_splits_sections_and_bullets() {
    let raw = "# Claude Memory\n\n## Work\n\n- ships on main\n- no PRs\n\n## Taste\n\n- terse replies\n\nsome prose line\n";
    let entries = parse_markdown(raw);
    assert_eq!(
        entries,
        vec![
            ImportEntry {
                section: "Work".into(),
                body: "ships on main".into()
            },
            ImportEntry {
                section: "Work".into(),
                body: "no PRs".into()
            },
            ImportEntry {
                section: "Taste".into(),
                body: "terse replies".into()
            },
        ]
    );
}

#[test]
fn parse_markdown_strips_trailing_metadata() {
    let entries = parse_markdown("- prefers bun [source: session-1; added: 2026-10-01]");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "prefers bun");
    assert_eq!(entries[0].section, "Notes");
}

#[test]
fn parse_markdown_skips_empty_bullets_and_headings_without_bullets() {
    let entries = parse_markdown("## Empty\n\n- \n- kept\n");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "kept");
}
