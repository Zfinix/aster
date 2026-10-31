use super::*;

#[test]
fn slug_reduces_a_name_to_one_segment() {
    assert_eq!(slug("Pricing Page!"), "pricing-page");
    assert_eq!(slug("  a__b  "), "a-b");
    assert_eq!(slug("!!!"), "");
}

#[test]
fn an_artifact_round_trips_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let page = write_artifact(dir.path(), "Pricing Page", "Pricing", "<h1>Hi</h1>").unwrap();
    assert_eq!(page, artifact_page(dir.path(), "Pricing Page"));
    assert_eq!(std::fs::read_to_string(&page).unwrap(), "<h1>Hi</h1>");
    let items = list_artifacts(dir.path());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "pricing-page");
    assert_eq!(items[0].title, "Pricing");
}

#[test]
fn a_canvas_keeps_board_order_and_escapes_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    create_canvas(dir.path(), "chat", "Chat").unwrap();
    add_board(
        dir.path(),
        "chat",
        "main",
        "Main <board>",
        "<h1>main</h1>",
        680,
        900,
    )
    .unwrap();
    add_board(
        dir.path(),
        "chat",
        "states",
        "States",
        "<h1>states</h1>",
        680,
        900,
    )
    .unwrap();
    let viewer = write_viewer(dir.path(), "chat").unwrap();
    let html = std::fs::read_to_string(viewer).unwrap();
    assert!(html.contains("src=\"project/main.html\""));
    assert!(html.contains("src=\"project/states.html\""));
    assert!(html.contains("Main &lt;board&gt;"));
    let canvases = list_canvases(dir.path());
    assert_eq!(canvases.len(), 1);
    assert_eq!(canvases[0].1.boards.len(), 2);
}

#[test]
fn replacing_a_board_keeps_its_place() {
    let dir = tempfile::tempdir().unwrap();
    create_canvas(dir.path(), "chat", "Chat").unwrap();
    add_board(dir.path(), "chat", "a", "A", "<p>a</p>", 680, 900).unwrap();
    add_board(dir.path(), "chat", "b", "B", "<p>b</p>", 680, 900).unwrap();
    add_board(dir.path(), "chat", "a", "A2", "<p>a2</p>", 680, 900).unwrap();
    let canvases = list_canvases(dir.path());
    let boards = &canvases[0].1.boards;
    assert_eq!(
        boards.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(boards[0].title, "A2");
}
