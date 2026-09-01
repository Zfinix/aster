//! Built-in rules, in the same language as the user's. A security boundary,
//! distinct from the review scope filter in `aster-cli` (`DEFAULT_EXCLUDE`). User
//! rules are consulted first, so an `allow` entry overrides anything here.

/// Writes that run as code later: a git hook or CI workflow written now
/// executes outside the sandbox afterwards. Confirmed rather than refused, so
/// the answer is the user's.
pub const ASK_EDIT: &[&str] = &[
    "Edit(.git/**)",
    "Edit(**/.git/**)",
    "Edit(.github/workflows/**)",
    "Edit(.husky/**)",
];

/// Commands worth a confirmation: privilege escalation, destructive filesystem
/// operations, process and system control, and network egress. These are what
/// `auto` pauses on and `edit` does not, which is the difference between them.
pub const ASK_BASH: &[&str] = &[
    "Bash(sudo:*)",
    "Bash(doas:*)",
    "Bash(su:*)",
    "Bash(rm:*)",
    "Bash(rmdir:*)",
    "Bash(dd:*)",
    "Bash(mkfs:*)",
    "Bash(shred:*)",
    "Bash(chmod:*)",
    "Bash(chown:*)",
    "Bash(chgrp:*)",
    "Bash(kill:*)",
    "Bash(killall:*)",
    "Bash(pkill:*)",
    "Bash(shutdown:*)",
    "Bash(reboot:*)",
    "Bash(halt:*)",
    "Bash(systemctl:*)",
    "Bash(launchctl:*)",
    "Bash(curl:*)",
    "Bash(wget:*)",
    "Bash(nc:*)",
    "Bash(ssh:*)",
    "Bash(scp:*)",
    "Bash(rsync:*)",
];

/// Secrets that must not reach the model. Denied rather than confirmed: the
/// answer cannot be taken back once the file is in the context.
pub const DENY_READ: &[&str] = &[
    "Read(**/.env)",
    "Read(**/.env.*)",
    "Read(**/*.pem)",
    "Read(**/*.key)",
    "Read(**/id_rsa*)",
    "Read(**/*.p12)",
    "Read(**/*.pfx)",
    "Read(**/credentials.json)",
    "Read(**/secrets.*)",
];
