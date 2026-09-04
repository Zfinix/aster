use super::*;

use image::{ImageBuffer, Rgb};

fn write_image(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let buffer = ImageBuffer::from_fn(width, height, |_, _| Rgb([120u8, 40, 200]));
    let path = dir.join(name);
    buffer.save(&path).unwrap();
    path
}

fn parts(content: &MessageContent) -> &[ContentPart] {
    match content {
        MessageContent::Parts(parts) => parts,
        MessageContent::Text(text) => panic!("expected parts, got text: {text}"),
    }
}

fn decoded(content: &MessageContent, at: usize) -> image::DynamicImage {
    let ContentPart::ImageUrl { image_url } = &parts(content)[at] else {
        panic!("expected an image at {at}");
    };
    let raw = image_url
        .url
        .strip_prefix("data:image/png;base64,")
        .unwrap();
    image::load_from_memory(&STANDARD.decode(raw).unwrap()).unwrap()
}

#[test]
fn a_mentioned_image_is_attached_beside_the_text_that_asked_about_it() {
    let dir = tempfile::tempdir().unwrap();
    write_image(dir.path(), "shot.png", 8, 8);

    let content = attach("what is in @shot.png", dir.path());

    let parts = parts(&content);
    assert_eq!(parts.len(), 2);
    assert!(matches!(&parts[0], ContentPart::Text { text } if text == "what is in @shot.png"));
    assert!(matches!(&parts[1], ContentPart::ImageUrl { .. }));
}

#[test]
fn a_turn_mentioning_no_image_stays_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    write_image(dir.path(), "shot.png", 8, 8);

    let content = attach("read @notes.txt and @src/main.rs", dir.path());

    assert_eq!(
        content,
        MessageContent::Text("read @notes.txt and @src/main.rs".into())
    );
}

#[test]
fn a_mention_of_an_image_that_is_not_there_leaves_the_turn_alone() {
    let dir = tempfile::tempdir().unwrap();

    let content = attach("what is in @missing.png", dir.path());

    assert_eq!(
        content,
        MessageContent::Text("what is in @missing.png".into())
    );
}

#[test]
fn several_images_attach_once_each_in_the_order_written() {
    let dir = tempfile::tempdir().unwrap();
    write_image(dir.path(), "before.png", 4, 4);
    write_image(dir.path(), "after.png", 4, 4);

    let content = attach(
        "compare @before.png with @after.png and @before.png",
        dir.path(),
    );

    assert_eq!(parts(&content).len(), 3);
}

#[test]
fn an_oversized_image_is_fitted_to_the_long_edge() {
    let dir = tempfile::tempdir().unwrap();
    write_image(dir.path(), "wide.png", 4000, 1000);

    let content = attach("@wide.png", dir.path());

    let fitted = decoded(&content, 1);
    assert_eq!(fitted.width(), MAX_EDGE);
    assert_eq!(fitted.height(), MAX_EDGE / 4);
}

#[test]
fn an_image_within_the_limit_is_sent_at_its_own_size() {
    let dir = tempfile::tempdir().unwrap();
    write_image(dir.path(), "small.png", 320, 200);

    let content = attach("@small.png", dir.path());

    let sent = decoded(&content, 1);
    assert_eq!((sent.width(), sent.height()), (320, 200));
}

#[test]
fn punctuation_around_a_mention_is_not_part_of_the_path() {
    let dir = tempfile::tempdir().unwrap();
    write_image(dir.path(), "shot.png", 8, 8);

    let content = attach("look at (@shot.png), please", dir.path());

    assert_eq!(parts(&content).len(), 2);
}

#[test]
fn an_absolute_path_resolves_outside_the_repo() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = write_image(outside.path(), "shot.png", 8, 8);

    let content = attach(&format!("@{}", path.display()), dir.path());

    assert_eq!(parts(&content).len(), 2);
}

#[test]
fn a_mention_under_a_folder_with_a_space_in_its_name_still_attaches() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let folder = outside.path().join("Application Support");
    std::fs::create_dir_all(&folder).unwrap();
    let path = write_image(&folder, "mtn9lm4c-image.png", 8, 8);

    let content = attach(
        &format!("im tired of this @{} sub agents need icons", path.display()),
        dir.path(),
    );

    assert_eq!(parts(&content).len(), 2);
}

#[test]
fn a_spaced_mention_and_a_plain_one_attach_once_each_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("my shots");
    std::fs::create_dir_all(&folder).unwrap();
    let first = write_image(&folder, "first.png", 4, 4);
    write_image(dir.path(), "second.png", 6, 6);

    let content = attach(
        &format!(
            "@{} then @second.png then @{}",
            first.display(),
            first.display()
        ),
        dir.path(),
    );

    assert_eq!(parts(&content).len(), 3);
    assert_eq!(decoded(&content, 1).width(), 4);
    assert_eq!(decoded(&content, 2).width(), 6);
}
