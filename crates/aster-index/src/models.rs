#[derive(Debug, Clone)]
pub struct IndexSymbol {
    pub file_path: String,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SymbolHit {
    pub name: String,
    pub kind: Option<String>,
    pub path: String,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GrepHit {
    pub path: String,
    pub line: u64,
    pub text: String,
}
