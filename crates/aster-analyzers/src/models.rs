use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub tool: String,
    pub rule: String,
    pub severity: Severity,
    pub file: String,
    pub line: u32,
    pub message: String,
}
