use super::*;

#[test]
fn rows_tile_the_whole_input() {
    let text = "the quick brown fox jumps";
    let ranges = rows(text, 10);
    assert_eq!(ranges.first().unwrap().start, 0);
    assert_eq!(ranges.last().unwrap().end, text.len());
    for pair in ranges.windows(2) {
        assert_eq!(pair[0].end, pair[1].start);
    }
}

#[test]
fn rows_never_exceed_the_width() {
    for row in lines("the quick brown fox jumps over the lazy dog", 12) {
        assert!(width(&row) <= 12, "{row:?}");
    }
}

#[test]
fn oversized_words_are_cut() {
    let rows = lines("supercalifragilistic", 6);
    assert_eq!(rows, ["superc", "alifra", "gilist", "ic"]);
}

#[test]
fn wide_glyphs_count_as_two_columns() {
    assert_eq!(width("日本語"), 6);
    for row in lines("日本語のテキスト", 6) {
        assert!(width(&row) <= 6);
    }
}

#[test]
fn empty_input_is_one_empty_row() {
    assert_eq!(lines("", 10), [""]);
}
