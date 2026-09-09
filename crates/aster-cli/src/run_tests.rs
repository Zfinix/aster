use super::split_notify_url;

#[test]
fn extracts_trailing_notify_url() {
    let report = "Picked Draft 2 (reply to @riyazz_ai).\nnotify-url: https://x.com/intent/tweet?in_reply_to=1&text=hi";
    let (url, body) = split_notify_url(report);
    assert_eq!(
        url,
        Some("https://x.com/intent/tweet?in_reply_to=1&text=hi")
    );
    assert_eq!(body, Some("Picked Draft 2 (reply to @riyazz_ai)."));
}

#[test]
fn keeps_reports_without_notify_url() {
    let report = "Picked Draft 2.\nnote: nothing here";
    let (url, body) = split_notify_url(report);
    assert_eq!(url, None);
    assert_eq!(body, Some(report));
}

#[test]
fn ignores_empty_notify_url() {
    let report = "done\nnotify-url:";
    let (url, body) = split_notify_url(report);
    assert_eq!(url, None);
    assert_eq!(body, Some("done\nnotify-url:"));
}

#[test]
fn handles_single_line_report() {
    let (url, body) = split_notify_url("just the picks");
    assert_eq!(url, None);
    assert_eq!(body, Some("just the picks"));
}
