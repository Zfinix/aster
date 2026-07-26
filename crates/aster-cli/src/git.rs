use anyhow::{Context, Result, bail};

fn git(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .output()
        .context("failed to run git; is it installed and on PATH?")?;
    if !out.status.success() {
        bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// None = working tree (staged + unstaged) against HEAD, not `git diff` with no args.
pub fn diff(range: Option<&str>) -> Result<String> {
    match range {
        Some(range) => {
            // A range starting with `-` lands before `--` and is read as a git flag.
            if range.starts_with('-') {
                bail!("invalid diff range {range:?}: ranges must not start with '-'");
            }
            git(&["diff", range, "--"])
        }
        None => git(&["diff", "HEAD"]),
    }
}

pub fn default_range() -> String {
    for base in ["origin/main", "main", "origin/master", "master"] {
        if let Ok(mb) = git(&["merge-base", base, "HEAD"]) {
            let mb = mb.trim();
            if !mb.is_empty() {
                return format!("{mb}..HEAD");
            }
        }
    }
    "HEAD~1..HEAD".to_string()
}

pub fn origin_repo() -> Result<(String, String)> {
    let url = git(&["remote", "get-url", "origin"])?.trim().to_string();
    let path = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .unwrap_or(&url);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.splitn(2, '/');
    match (parts.next(), parts.next()) {
        (Some(owner), Some(repo)) if !owner.is_empty() && !repo.is_empty() => {
            Ok((owner.to_string(), repo.to_string()))
        }
        _ => bail!("could not parse owner/repo from origin url: {url}"),
    }
}
