use super::*;

#[test]
fn speakable_reads_the_prose_and_skips_code() {
    let reply = "## Done\n\nI changed **two** files, see [the diff](https://x.dev/d).\n\n\
                 ```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n\n- run `make test`\n> all green";
    assert_eq!(
        speakable(reply),
        "Done I changed two files, see the diff. run make test all green"
    );
}

#[test]
fn speakable_stops_at_the_last_sentence_that_fits() {
    let sentence = "This sentence is exactly forty chars ok. ";
    let reply = sentence.repeat(MAX_SPOKEN_CHARS / sentence.len() + 5);
    let spoken = speakable(&reply);
    assert!(spoken.chars().count() <= MAX_SPOKEN_CHARS);
    assert!(spoken.ends_with("ok."));
}
