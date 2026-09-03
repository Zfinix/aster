---
name: gh-pr-workflow
description: Working with GitHub pull requests, issues, and the API through the gh CLI. Use whenever the task involves reviewing, creating, or commenting on a PR or issue, or calling the GitHub API.
---

# GitHub PR workflow

1. **Use `gh` for GitHub operations.** Structured queries beat scraping:
   `gh pr view 12 --json title,body,files,comments`, `gh pr checks 12`,
   `gh issue list --limit 10`.
2. **Big diffs go to a file, not the conversation.**
   `gh pr diff 12 > /tmp/pr12.diff`, then search and read slices of that file.
   Never print a whole PR diff into the chat.
3. **Draft first, post on approval.** Write the review or comment body to a
   scratch file, show it to the user, and only after they approve post it:
   `gh api repos/{owner}/{repo}/pulls/12/reviews --method POST --input review.json`.
   Never post to GitHub as a side effect of being asked to "review".
4. **One PR at a time, one comment per issue found.** Do not batch several
   PRs' feedback into one pass, and do not stack multiple problems into one
   comment.
5. **Auth errors are a full stop.** A 401 or `gh auth status` failure means
   tell the user to run `gh auth login`. Retrying, or switching to raw git
   pushes, will not help.
6. **PR bodies: summary plus test plan.** Short paragraphs over prose walls.
   No attribution footers unless asked.
7. **Know which subcommands write.** Reads (`view`, `list`, `diff`, `checks`,
   `api` GET, `search`) run freely; writes (`create`, `edit`, `merge`, `close`,
   `comment`, `review`, any `--method POST/PATCH/PUT/DELETE`, `pr checkout`,
   `release create`) change state and will ask the user for approval. Batch
   reads first, then propose the writes as one step instead of interleaving.
