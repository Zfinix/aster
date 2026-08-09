use super::*;
use aster_models::{Finding, ReviewReport};

fn finding(title: &str) -> Finding {
    Finding {
        file_path: "src/handlers.rs".into(),
        line: 4,
        start_line: None,
        side: None,
        severity: "critical".into(),
        category: "security".into(),
        title: title.into(),
        description: "desc".into(),
        suggestion: "fix it".into(),
        code_snippet: None,
        confidence: Some(0.97),
    }
}

#[test]
fn review_tui_chat_carries_findings_into_messages() {
    // Regression: findings must reach chat; context capture must not be gated on `finished`.
    let mut app = App::new(0.0);
    let report = ReviewReport::new(
        "summary".into(),
        vec![finding("SQL Injection vulnerability")],
        vec![],
    );
    app.set_report_context(&report);
    let msgs = app.build_chat("how do i fix it");
    assert!(
        msgs.iter()
            .any(|m| m.content.contains("SQL Injection vulnerability")),
        "chat messages must include the review findings as context"
    );
}
