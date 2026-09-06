use super::*;

const W: u16 = 40;

fn with(text: &str) -> Composer {
    let mut c = Composer::default();
    c.insert_str(text);
    c
}

#[test]
fn typing_and_backspace_track_the_cursor() {
    let mut c = Composer::default();
    for ch in "héllo".chars() {
        c.insert(ch);
    }
    c.backspace();
    assert_eq!(c.text(), "héll");
    c.left();
    c.backspace();
    assert_eq!(c.text(), "hél");
}

#[test]
fn word_motion_skips_whole_words() {
    let mut c = with("alpha beta gamma");
    c.word_left();
    assert_eq!(c.cursor, "alpha beta ".len());
    c.word_left();
    assert_eq!(c.cursor, "alpha ".len());
    c.word_right();
    assert_eq!(c.cursor, "alpha beta".len());
}

#[test]
fn delete_word_back_removes_one_word() {
    let mut c = with("cargo check --all");
    c.delete_word_back();
    assert_eq!(c.text(), "cargo check ");
}

#[test]
fn kill_operates_on_the_current_line_only() {
    let mut c = with("first\nsecond line");
    c.home();
    c.kill_to_end();
    assert_eq!(c.text(), "first\n");
}

#[test]
fn vertical_motion_reports_when_it_runs_out_of_rows() {
    let mut c = with("one\ntwo");
    assert!(c.up(W));
    assert!(!c.up(W));
    assert!(c.down(W));
    assert!(!c.down(W));
}

#[test]
fn recall_walks_sent_messages_and_restores_the_draft() {
    let mut c = Composer::default();
    c.insert_str("first");
    c.take();
    c.insert_str("second");
    c.take();
    c.insert_str("draft");

    c.recall_prev();
    assert_eq!(c.text(), "second");
    c.recall_prev();
    assert_eq!(c.text(), "first");
    c.recall_next();
    assert_eq!(c.text(), "second");
    c.recall_next();
    assert_eq!(c.text(), "draft");
}

#[test]
fn a_big_paste_folds_and_expands_on_send() {
    let mut c = Composer::default();
    let big = "x\n".repeat(400);
    c.insert_str("look: ");
    c.paste(&big);
    assert!(c.text().contains("[pasted 400 lines]"));
    assert!(c.text().len() < 60);
    assert_eq!(c.take(), format!("look: {big}"));
}

#[test]
fn the_cursor_lands_on_the_row_the_caret_is_in() {
    let mut c = with("aaaa bbbb cccc dddd eeee ffff gggg");
    c.home();
    let (_, (row, col)) = c.render(20, "");
    assert_eq!((row, col), (0, 2));
    c.end();
    let (lines, (row, _)) = c.render(20, "");
    assert!((row as usize) < lines.len());
}

#[test]
fn the_draft_grows_the_composer_but_stops_at_eight_rows() {
    let mut c = Composer::default();
    assert_eq!(c.height(W), 1);
    c.insert_str(&"line\n".repeat(3));
    assert_eq!(c.height(W), 4);
    c.insert_str(&"line\n".repeat(40));
    assert_eq!(c.height(W), 8);
}

#[test]
fn mention_context_finds_at_cursor() {
    let c = with("fix @src/mai");
    let (start, query) = c.mention_context().unwrap();
    assert_eq!(start, "fix ".len());
    assert_eq!(query, "src/mai");
}

#[test]
fn mention_context_empty_just_after_at() {
    let c = with("look at @");
    let (start, query) = c.mention_context().unwrap();
    assert_eq!(start, "look at ".len());
    assert_eq!(query, "");
}

#[test]
fn mention_context_none_when_at_in_middle_of_word() {
    let c = with("email@example.com");
    assert!(c.mention_context().is_none());
}

#[test]
fn mention_context_none_when_cursor_not_after_at() {
    let c = with("fix @src/main.rs bug");
    assert!(c.mention_context().is_none());
}

#[test]
fn complete_mention_replaces_query_and_adds_space() {
    let mut c = with("check @sr");
    let (start, _) = c.mention_context().unwrap();
    c.complete_mention(start, "src/main.rs");
    assert_eq!(c.text(), "check @src/main.rs ");
    assert_eq!(c.cursor, "check @src/main.rs ".len());
}

#[test]
fn cancel_mention_removes_at_and_query() {
    let mut c = with("fix @src/mai");
    let (start, _) = c.mention_context().unwrap();
    c.cancel_mention(start);
    assert_eq!(c.text(), "fix ");
    assert_eq!(c.cursor, "fix ".len());
}

#[test]
fn an_escaped_absolute_path_folds_and_records_a_clean_reference() {
    let mut c = Composer::default();
    c.insert_str(
        "look at /Users/me/Desktop/Screen\\ Recording\\ 2026-08-01\\ at\\ 10.52.58\\ AM.mov ok",
    );
    let text = c.take();
    assert!(text.contains("[@"), "got: {text}");
    assert!(
        !text.contains("Screen\\"),
        "escape leaked into sent text: {text}"
    );
    let refs = c.take_refs();
    assert_eq!(refs.len(), 1);
    let (mark, path) = &refs[0];
    assert_eq!(
        path,
        "/Users/me/Desktop/Screen Recording 2026-08-01 at 10.52.58 AM.mov"
    );
    assert!(
        text.contains(mark),
        "sent text missing its own token: {text}"
    );
}

#[test]
fn a_short_repo_path_is_left_alone() {
    let mut c = Composer::default();
    c.insert_str("see src/main.rs");
    let text = c.take();
    assert_eq!(text, "see src/main.rs");
    assert!(c.take_refs().is_empty());
}

#[test]
fn a_non_path_token_is_left_alone() {
    let mut c = Composer::default();
    c.insert_str("the ratio a/b is fine");
    let text = c.take();
    assert_eq!(text, "the ratio a/b is fine");
    assert!(c.take_refs().is_empty());
}

#[test]
fn identical_paths_fold_to_one_reference() {
    let mut c = Composer::default();
    let p = "/a/very/long/directory/that/keeps/going/deep/file.rs";
    c.insert_str(&format!("{p} then {p}"));
    let text = c.take();
    assert_eq!(c.take_refs().len(), 1);
    assert_eq!(text.matches("[@file.rs]").count(), 2);
}

#[test]
fn a_windows_path_folds_to_its_basename() {
    let mut c = Composer::default();
    c.insert_str("open C:\\Users\\Alice\\Documents\\report.docx now");
    let text = c.take();
    assert!(text.contains("[@report.docx]"), "got: {text}");
    let (mark, path) = c.take_refs().pop().unwrap();
    assert_eq!(mark, "[@report.docx]");
    assert_eq!(path, "C:\\Users\\Alice\\Documents\\report.docx");
}

#[test]
fn take_refs_is_drained_after_use() {
    let mut c = Composer::default();
    c.insert_str("open /some/long/directory/prefix/report.pdf now");
    c.take();
    assert_eq!(c.take_refs().len(), 1);
    assert!(c.take_refs().is_empty());
}
