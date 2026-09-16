use super::*;

fn two_file_diff() -> String {
    let mut files = Vec::new();
    for i in 0..40 {
        files.push(format!(
            "diff --git a/src/file{i}.rs b/src/file{i}.rs\n\
             --- a/src/file{i}.rs\n\
             +++ b/src/file{i}.rs\n\
             @@ -1 +1 @@\n\
             -let x = 1;\n\
             +let x = {i};\n"
        ));
    }
    files.join("\n")
}

#[test]
fn chunk_diff_groups_files_under_the_budget() {
    let diff = two_file_diff();
    let chunks = chunk_diff(&diff, 2000);
    assert!(chunks.len() > 1, "a 40-file diff must split");
    for chunk in &chunks {
        assert!(
            chunk.len() <= 2000 + 200,
            "only one oversized file may pass"
        );
        assert!(chunk.starts_with("diff --git "));
    }
    // Every file survives the round trip exactly once.
    let joined = chunks.join("\n");
    for i in 0..40 {
        assert_eq!(
            joined
                .matches(&format!("diff --git a/src/file{i}.rs"))
                .count(),
            1,
            "file{i} must appear in exactly one chunk"
        );
    }
}

#[test]
fn chunk_diff_never_splits_a_single_file() {
    let diff = two_file_diff();
    let chunks = chunk_diff(&diff, 10);
    assert_eq!(chunks.len(), 40, "each file becomes its own chunk");
}

#[test]
fn chunk_diff_zero_budget_returns_whole_diff() {
    let diff = two_file_diff();
    assert_eq!(chunk_diff(&diff, 0), vec![diff.clone()]);
}

#[test]
fn chunk_diff_keeps_small_diffs_whole() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n+x\n";
    assert_eq!(chunk_diff(diff, 40_000).len(), 1);
}
