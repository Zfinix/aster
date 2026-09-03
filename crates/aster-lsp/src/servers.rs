use std::path::Path;

/// Which language server a file maps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServerKind {
    RustAnalyzer,
    TypeScriptLanguageServer,
}

impl ServerKind {
    pub fn binary(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::TypeScriptLanguageServer => "typescript-language-server",
        }
    }

    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => &[],
            Self::TypeScriptLanguageServer => &["--stdio"],
        }
    }

    pub fn language_id(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust",
            Self::TypeScriptLanguageServer => "typescript",
        }
    }
}

pub fn installed(kind: ServerKind) -> bool {
    // A rustup shim can sit on PATH with the real server never installed, so
    // probe the binary rather than trusting the name lookup.
    which::which(kind.binary()).is_ok()
        && std::process::Command::new(kind.binary())
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
}

/// The server that can answer queries about `path`, if any.
pub fn supported(path: &Path) -> Option<ServerKind> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Some(ServerKind::RustAnalyzer),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs") => Some(ServerKind::TypeScriptLanguageServer),
        _ => None,
    }
}
