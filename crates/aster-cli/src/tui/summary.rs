use aster_models::ReviewReport;

/// Reprint the outcome to the real terminal after the TUI closes, so results
/// survive in scrollback instead of vanishing with the alternate screen.
pub(super) fn print_summary(resp: &ReviewReport, min_confidence: f32) {
    let findings: Vec<_> = resp
        .findings
        .iter()
        .filter(|f| f.confidence.unwrap_or(1.0) >= min_confidence)
        .collect();

    println!("\n{}\n", resp.summary);
    for (i, f) in findings.iter().enumerate() {
        let conf = f
            .confidence
            .map(|c| format!("  {:.0}%", c * 100.0))
            .unwrap_or_default();
        println!(
            "[{}] {}  ({}/{})  {}:{}{}",
            i + 1,
            f.title,
            f.severity,
            f.category,
            f.file_path,
            f.line,
            conf
        );
        println!("    {}", f.description);
        println!("    fix: {}\n", f.suggestion);
    }
}
