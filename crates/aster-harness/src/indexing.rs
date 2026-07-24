use std::fs;
use std::path::Path;

use aster_index::{IndexSymbol, SqliteCodeIndex};
use ignore::WalkBuilder;

pub async fn build_repo_index(
    root: &Path,
    db_path: &Path,
) -> anyhow::Result<(SqliteCodeIndex, usize)> {
    let index = SqliteCodeIndex::open(db_path).await?;
    let symbols = collect_symbols(root);
    let count = index.build_from_symbols("local", "HEAD", &symbols).await?;
    Ok((index, count))
}

pub fn collect_symbols(root: &Path) -> Vec<IndexSymbol> {
    let mut out = Vec::new();
    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel = rel.to_string_lossy().to_string();
        for s in symbol_extractor::extract_symbols(&content, &rel) {
            out.push(IndexSymbol {
                file_path: s.file_path,
                name: s.symbol_name,
                kind: s.symbol_kind,
                start_line: s.start_line.map(i64::from),
                end_line: s.end_line.map(i64::from),
                snippet: s.code_snippet,
            });
        }
    }
    out
}
