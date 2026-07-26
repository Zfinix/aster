//! Built-in glob lists. A security boundary, distinct from the review scope
//! filter in `aster-cli` (`DEFAULT_EXCLUDE`); do not conflate the two.

/// Paths never writable by default: git internals and execute-on-next-action
/// files where an unattended edit is effectively remote code execution.
/// Overridable only via an explicit config `allow` entry.
pub const PROTECTED: &[&str] = &[
    ".git/**",
    "**/.git/**",
    ".github/workflows/**",
    ".git/hooks/**",
    ".husky/**",
];

/// Files never readable by default: secrets that must not reach the model.
pub const SECRET_READ: &[&str] = &[
    "**/.env",
    "**/.env.*",
    "**/*.pem",
    "**/*.key",
    "**/id_rsa*",
    "**/*.p12",
    "**/*.pfx",
    "**/credentials.json",
    "**/secrets.*",
];
