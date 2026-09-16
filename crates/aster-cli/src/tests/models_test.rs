use super::*;

fn current_rows(rows: &[serde_json::Value]) -> Vec<&str> {
    rows.iter()
        .filter(|row| row["current"] == true)
        .map(|row| row["name"].as_str().unwrap_or_default())
        .collect()
}

#[test]
fn a_catalog_endpoint_is_the_only_row_marked_current() {
    let rows = provider_rows("https://api.fireworks.ai/inference/v1");
    assert_eq!(current_rows(&rows), ["Fireworks AI"]);
    let slashed = provider_rows("https://api.fireworks.ai/inference/v1/");
    assert_eq!(
        rows.len(),
        slashed.len(),
        "a trailing slash is not a new row"
    );
}

#[test]
fn an_endpoint_outside_the_catalog_gets_a_row_named_for_its_host() {
    let catalog = provider_rows("https://api.fireworks.ai/inference/v1");
    let rows = provider_rows("https://api.atria-asi.ai/v1");
    assert_eq!(rows.len(), catalog.len() + 1);
    assert_eq!(current_rows(&rows), ["api.atria-asi.ai"]);
    let row = rows.last().expect("the custom row is appended");
    assert_eq!(row["base_url"], "https://api.atria-asi.ai/v1");
    // An empty model leaves the one in use alone when a picker switches back.
    assert_eq!(row["example_model"], "");
}

#[test]
fn no_endpoint_at_all_invents_no_row() {
    let rows = provider_rows("");
    assert!(current_rows(&rows).is_empty());
}
