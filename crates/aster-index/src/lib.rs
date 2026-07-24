#![forbid(unsafe_code)]

pub mod grep;
pub mod models;

pub use models::{GrepHit, IndexSymbol, SymbolHit};

use std::collections::HashMap;
use std::path::Path;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

pub struct SqliteCodeIndex {
    pool: SqlitePool,
}

impl SqliteCodeIndex {
    pub async fn open(db_path: &Path) -> anyhow::Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;

        let index = Self { pool };
        index.init_schema().await?;
        Ok(index)
    }

    async fn init_schema(&self) -> anyhow::Result<()> {
        let statements = [
            "CREATE TABLE IF NOT EXISTS index_meta (k TEXT PRIMARY KEY, v TEXT)",
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                lang TEXT,
                content_hash TEXT,
                size INTEGER
            )",
            "CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                kind TEXT,
                start_line INTEGER,
                end_line INTEGER,
                snippet TEXT
            )",
            "CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name)",
            "CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id)",
            "CREATE VIRTUAL TABLE IF NOT EXISTS symbol_fts USING fts5(name, snippet, tokenize='unicode61')",
        ];
        for stmt in statements {
            sqlx::query(stmt).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn build_from_symbols(
        &self,
        repo: &str,
        commit: &str,
        symbols: &[IndexSymbol],
    ) -> anyhow::Result<usize> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM symbol_fts")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM symbols").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM files").execute(&mut *tx).await?;

        let mut file_ids: HashMap<String, i64> = HashMap::new();
        let mut count = 0usize;

        for sym in symbols {
            let Some(name) = sym.name.as_deref() else {
                continue;
            };
            if name.trim().is_empty() {
                continue;
            }

            let file_path = normalize_path(&sym.file_path);
            let file_id = match file_ids.get(&file_path) {
                Some(id) => *id,
                None => {
                    let res = sqlx::query("INSERT INTO files (path, lang) VALUES (?, ?)")
                        .bind(&file_path)
                        .bind(lang_from_path(&file_path))
                        .execute(&mut *tx)
                        .await?;
                    let id = res.last_insert_rowid();
                    file_ids.insert(file_path, id);
                    id
                }
            };

            let res = sqlx::query(
                "INSERT INTO symbols (file_id, name, kind, start_line, end_line, snippet)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(file_id)
            .bind(name)
            .bind(&sym.kind)
            .bind(sym.start_line)
            .bind(sym.end_line)
            .bind(&sym.snippet)
            .execute(&mut *tx)
            .await?;

            sqlx::query("INSERT INTO symbol_fts (rowid, name, snippet) VALUES (?, ?, ?)")
                .bind(res.last_insert_rowid())
                .bind(name)
                .bind(sym.snippet.as_deref().unwrap_or(""))
                .execute(&mut *tx)
                .await?;

            count += 1;
        }

        set_meta(&mut tx, "repo", repo).await?;
        set_meta(&mut tx, "indexed_commit", commit).await?;

        tx.commit().await?;
        Ok(count)
    }

    pub async fn find_symbol(&self, name: &str, limit: i64) -> anyhow::Result<Vec<SymbolHit>> {
        let rows = sqlx::query_as::<_, SymbolHit>(
            "SELECT s.name AS name, s.kind AS kind, f.path AS path,
                    s.start_line AS start_line, s.end_line AS end_line, s.snippet AS snippet
             FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.name = ?
             ORDER BY f.path
             LIMIT ?",
        )
        .bind(name)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn lexical(&self, query: &str, limit: i64) -> anyhow::Result<Vec<SymbolHit>> {
        let rows = sqlx::query_as::<_, SymbolHit>(
            "SELECT s.name AS name, s.kind AS kind, f.path AS path,
                    s.start_line AS start_line, s.end_line AS end_line, s.snippet AS snippet
             FROM symbol_fts
             JOIN symbols s ON s.id = symbol_fts.rowid
             JOIN files f ON f.id = s.file_id
             WHERE symbol_fts MATCH ?
             ORDER BY bm25(symbol_fts)
             LIMIT ?",
        )
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn symbols_in_file(&self, path: &str) -> anyhow::Result<Vec<SymbolHit>> {
        // Match on the normalized path first, then fall back to a suffix match so
        // a relative candidate path (`crates/x.rs`) still resolves against an
        // index that stored a differently-rooted path. Bounded to keep a
        // generated/huge file from returning every symbol row.
        let norm = normalize_path(path);
        // Escape LIKE metacharacters so a literal `_` or `%` in a filename does
        // not act as a wildcard and match an unrelated file.
        let suffix = format!("%/{}", escape_like(&norm));
        let rows = sqlx::query_as::<_, SymbolHit>(
            "SELECT s.name AS name, s.kind AS kind, f.path AS path,
                    s.start_line AS start_line, s.end_line AS end_line, s.snippet AS snippet
             FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE f.path = ? OR f.path LIKE ? ESCAPE '\\'
             ORDER BY s.start_line
             LIMIT 1000",
        )
        .bind(&norm)
        .bind(&suffix)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn symbol_count(&self) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM symbols")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }
}

async fn set_meta(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO index_meta (k, v) VALUES (?, ?)
         ON CONFLICT(k) DO UPDATE SET v = excluded.v",
    )
    .bind(key)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Normalize a path key to forward slashes with any leading `./` stripped, so
/// index writes and lookups agree regardless of platform separators.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

/// Escape SQL LIKE metacharacters (`\`, `%`, `_`) for use with `ESCAPE '\'`.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn lang_from_path(path: &str) -> Option<String> {
    use std::path::Path;

    let ext = Path::new(path).extension().and_then(|s| s.to_str())?;
    let lang = match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "py" | "pyw" => "python",
        "js" | "jsx" | "mjs" | "cjs" | "es6" | "es" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "java" | "jar" | "jmod" | "class" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "h++" | "c++" | "cp" => "cpp",
        "swift" => "swift",
        "rb" | "erb" | "rake" | "gemspec" => "ruby",
        "kt" | "kts" => "kotlin",
        "cs" | "csx" => "csharp",
        "php" | "php3" | "php4" | "php5" | "phtml" | "phps" => "php",
        "scala" | "sc" | "sbt" => "scala",
        "dart" => "dart",
        "pl" | "pm" | "pod" | "t" | "psgi" => "perl",
        "sh" | "bash" | "bats" | "command" | "tmux" | "tool" | "zsh" => "shell",
        "m" | "mm" => "objective-c",
        "hs" | "lhs" => "haskell",
        "lua" => "lua",
        "r" => "r",
        "jl" => "julia",
        "oct" | "mat" | "mxm" => "matlab",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "json" => "json",
        "jsonc" => "jsonc",
        "md" | "markdown" | "mdown" | "mkdn" | "mkd" | "rmd" => "markdown",
        "html" | "htm" | "shtml" | "xhtml" | "jhtml" => "html",
        "css" | "scss" | "sass" | "less" | "styl" => "css",
        "xml" | "xsd" | "xslt" | "wsdl" | "atom" | "rss" | "svg" => "xml",
        "clj" | "cljs" | "cljc" | "edn" => "clojure",
        "lisp" | "lsp" | "cl" | "el" | "scm" | "ss" => "lisp",
        "fs" | "fsi" | "fsx" => "fsharp",
        "erl" | "hrl" => "erlang",
        "ex" | "exs" => "elixir",
        "ml" | "mli" | "mll" | "mly" => "ocaml",
        "sql" => "sql",
        "ps1" | "psm1" | "psd1" => "powershell",
        "asm" | "s" => "assembly",
        "hx" => "haxe",
        "coffee" | "cjsx" => "coffeescript",
        "vb" | "bas" | "vbs" | "vba" => "vb",
        "groovy" | "gvy" => "groovy",
        "svelte" => "svelte",
        "vue" => "vue",
        "elm" => "elm",
        "cr" => "crystal",
        "nim" => "nim",
        "zig" => "zig",
        "f" | "for" | "f90" | "f95" | "f03" | "f08" | "f77" => "fortran",
        "adb" | "ads" | "ada" => "ada",
        "d" | "di" => "d",
        "pro" | "prolog" => "prolog",
        "mk" | "mak" => "makefile",
        "bat" | "cmd" => "batch",
        "dockerfile" => "dockerfile",
        "gitignore" | "ignore" | "npmignore" => "ignore",
        "rc" | "conf" | "cfg" | "ini" => "config",
        "tex" | "sty" | "cls" => "latex",
        "rkt" => "racket",
        "sol" => "solidity",
        "wat" => "wasm",
        _ => return None,
    };
    Some(lang.to_string())
}
