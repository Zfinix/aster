//! Built-in rules, in the same language as the user's. A security boundary,
//! distinct from the review scope filter in `aster-cli` (`DEFAULT_EXCLUDE`). User
//! rules are consulted first, so an `allow` entry overrides anything here.

pub const ASK_EDIT: &[&str] = &[
    "Edit(.git/**)",
    "Edit(**/.git/**)",
    "Edit(.github/workflows/**)",
    "Edit(.husky/**)",
];

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
