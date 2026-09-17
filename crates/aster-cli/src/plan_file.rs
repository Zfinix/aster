//! The plan as a file the agent drafts and revises while it reads the code,
//! then presents with `exit_plan_mode`, instead of a document composed in one
//! tool call at the end.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::chat::SessionCtx;

/// `None` for a run that keeps no sessions; the plan then lives in memory only.
pub(crate) fn plan_path(ctx: &SessionCtx, repo_root: &Path) -> Option<PathBuf> {
    let store = ctx.store.as_ref()?;
    let id = ctx.recorder.as_ref()?.lock().ok()?.id().to_string();
    Some(store.plan_path(repo_root, &id))
}

/// The file wins over memory, so an edit the user made to it is what gets
/// presented.
pub(crate) fn read_plan(ctx: &SessionCtx, repo_root: &Path) -> String {
    if let Some(text) = plan_path(ctx, repo_root).and_then(|p| std::fs::read_to_string(p).ok()) {
        return text;
    }
    ctx.plan
        .lock()
        .map(|plan| plan.document.clone())
        .unwrap_or_default()
}

pub(crate) fn write_plan(
    ctx: &SessionCtx,
    repo_root: &Path,
    content: Option<&str>,
    old_str: Option<&str>,
    new_str: Option<&str>,
) -> Result<String> {
    let next = match (content, old_str) {
        (Some(_), Some(_)) => bail!("pass either `content` or `old_str` with `new_str`, not both"),
        (Some(content), None) => content.to_string(),
        (None, Some(old)) => {
            let new = new_str.context("`old_str` needs a `new_str` (empty to delete the text)")?;
            let current = read_plan(ctx, repo_root);
            if current.trim().is_empty() {
                bail!("there is no plan yet; write the first draft with `content`");
            }
            match current.matches(old).count() {
                1 => current.replacen(old, new, 1),
                0 => bail!("`old_str` is not in the plan. The plan as it stands:\n\n{current}"),
                n => bail!(
                    "`old_str` appears {n} times in the plan; include more of the surrounding text so it matches once"
                ),
            }
        }
        (None, None) => bail!("write_plan needs `content`, or `old_str` and `new_str`"),
    };
    if next.trim().is_empty() {
        bail!("the plan cannot be empty");
    }

    {
        let mut plan = ctx
            .plan
            .lock()
            .map_err(|_| anyhow::anyhow!("plan state lock poisoned"))?;
        plan.document = next.clone();
        plan.approved = false;
    }

    let lines = next.lines().count();
    let Some(path) = plan_path(ctx, repo_root) else {
        return Ok(format!("plan saved ({lines} lines)"));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, &next).with_context(|| format!("writing {}", path.display()))?;
    Ok(format!("plan saved to {} ({lines} lines)", path.display()))
}

#[cfg(test)]
#[path = "tests/plan_file_test.rs"]
mod tests;
