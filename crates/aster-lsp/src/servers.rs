use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

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

static PROBED: LazyLock<Mutex<HashMap<ServerKind, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn installed(kind: ServerKind) -> bool {
    let mut probed = PROBED.lock().expect("probe cache");
    // A rustup shim can sit on PATH with the real server never installed, so
    // probe the binary rather than trusting the name lookup. The probe spawns
    // a process, so it is answered once per run.
    *probed.entry(kind).or_insert_with(|| {
        which::which(kind.binary()).is_ok()
            && std::process::Command::new(kind.binary())
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
    })
}

/// The server that can answer queries about `path`, if any.
pub fn supported(path: &Path) -> Option<ServerKind> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Some(ServerKind::RustAnalyzer),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs") => Some(ServerKind::TypeScriptLanguageServer),
        _ => None,
    }
}
