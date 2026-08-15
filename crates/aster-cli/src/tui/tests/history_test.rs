use super::*;

fn text_of(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn the_welcome_closes_with_one_tip() {
    let lines = welcome(&[("model", "claude".to_string())], 80);
    let text = text_of(&lines);
    assert_eq!(
        text.iter().filter(|l| l.starts_with("✨ Tip: ")).count(),
        1,
        "{text:?}"
    );
}

#[test]
fn the_tip_is_not_pinned_to_one_entry() {
    let picks: std::collections::HashSet<&str> = (0..64).map(|_| tip()).collect();
    assert!(picks.len() > 1, "{picks:?}");
}

#[test]
fn an_error_box_frames_and_wraps_every_failure() {
    let problems = vec![
        "linkedin-mcp crashed: Cannot find package '@modelcontextprotocol/sdk'".to_string(),
        "railway needs auth: sign in at https://railway.app/auth".to_string(),
    ];
    let lines = error_box(&problems, 40);
    let text = text_of(&lines);
    assert_eq!(text[0], "");
    assert!(text[1].starts_with("  ╭─"), "{text:?}");
    assert!(text.last().unwrap().starts_with("  ╰─"), "{text:?}");
    assert!(text.iter().any(|l| l.contains("linkedin-mcp")), "{text:?}");
    assert!(text.iter().any(|l| l.contains("railway")), "{text:?}");
    // Wrapped to the terminal, not overflowing it.
    assert!(text.iter().all(|l| wrap::width(l) <= 40), "{text:?}");
    let error = Style::default().fg(theme::get().error);
    assert!(
        lines[1..]
            .iter()
            .flat_map(|l| &l.spans)
            .all(|s| s.style == error),
        "every span should carry the error color"
    );
    assert!(error_box(&[], 40).is_empty());
}

#[test]
fn long_output_is_elided_in_the_middle() {
    let body: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
    let out = tool("Ran cargo check", &body.join("\n"), false, 80);
    let rendered = text_of(&out);
    assert!(rendered.iter().any(|l| l.contains("… +22 lines")));
    assert!(rendered.iter().any(|l| l.contains("line 0")));
    assert!(rendered.iter().any(|l| l.contains("line 29")));
    assert!(!rendered.iter().any(|l| l.contains("line 15")));
}

#[test]
fn short_output_is_kept_whole() {
    let out = text_of(&tool("Ran ls", "a\nb\nc", false, 80));
    assert!(out.iter().any(|l| l.ends_with('b')));
    assert!(!out.iter().any(|l| l.contains('…')));
}

#[test]
fn a_patch_counts_its_added_and_removed_lines() {
    let out = text_of(&patch("Edited", "src/lib.rs", "- old\n- gone\n+ new\n", 80));
    assert!(out.iter().any(|l| l.contains("+1 −2")), "{out:?}");
}

#[test]
fn diff_rows_are_padded_so_the_tint_spans_the_width() {
    let rows = diff_lines("+ new", 20);
    assert_eq!(rows[0].width(), 20);
    assert_eq!(
        rows[0].spans[0].style.fg,
        Some(crate::tui::theme::get().add_mark)
    );
    assert_eq!(
        rows[0].spans[1].style.fg,
        Some(crate::tui::theme::get().add_fg)
    );
}

#[test]
fn a_key_as_wide_as_the_column_still_separates_from_its_value() {
    let fields = [
        ("model", "kimi-k3".to_string()),
        ("provider", "OpenRouter".to_string()),
    ];
    let out = text_of(&welcome(&fields, 80));
    assert!(
        out.iter().any(|l| l.contains("provider  OpenRouter")),
        "{out:?}"
    );
    assert!(
        out.iter().any(|l| l.contains("model     kimi-k3")),
        "{out:?}"
    );
}

#[test]
fn a_long_welcome_value_wraps_under_its_column() {
    let fields = [(
        "tools",
        "read_file, list_files, explore, search_files, find_files, run_command".to_string(),
    )];
    let out = text_of(&welcome(&fields, 40));
    assert!(
        out.iter().any(|l| l.starts_with("tools  read_file")),
        "{out:?}"
    );
    let hang = out
        .iter()
        .find(|l| l.trim_start().starts_with("search_files"))
        .unwrap();
    assert!(hang.starts_with("       search_files"), "{hang:?}");
}

#[test]
fn explored_rows_stack_under_one_header() {
    let mut out = text_of(&explored_row("Read a.rs", false, 80));
    out.extend(text_of(&explored_row("Read b.rs", true, 80)));
    assert_eq!(out.iter().filter(|l| l.contains("Explored")).count(), 1);
    assert_eq!(out.iter().filter(|l| l.contains("Read ")).count(), 2);
    assert!(out.iter().any(|l| l.contains("└ Read a.rs")), "{out:?}");
}

#[test]
fn continuation_lines_hang_under_the_bullet() {
    let out = text_of(&user(
        "a fairly long sentence that has to wrap somewhere",
        24,
    ));
    assert!(out[1].starts_with("▌❯ "));
    assert!(out[2].starts_with("▌  "));
}
