#!/bin/zsh
# Routing-accuracy eval: labeled prompts vs the live mom router.
cd "$(dirname "$0")/.."

typeset -a easy hard
easy=(
  "fix the typo in the readme"
  "what does the version command print"
  "add a unit test for openrouter_slug"
  "rename the variable count to failed_steps in eval.rs"
  "bump the chrono dependency to the latest version"
  "what file defines the Signals struct"
  "format this file with rustfmt"
  "add a --json flag that prints the overview as json"
  "delete the unused import in manifest.rs"
  "write a one line description for the cli help text"
  "does the settings loader read from the repo root or home first"
  "change the default hold from 3 to 4"
)
hard=(
  "plan a migration of the persistence layer to a new schema with rollback support"
  "redesign the retry and repetition handling across aster-ai so streaming and tool turns share one backoff policy"
  "debug why the tui deadlocks when an mcp server times out during an approval prompt"
  "refactor the permission system so grants compose across sub-agents without widening scope"
  "design a plugin api that lets third parties add switch conditions safely"
  "why does memory usage grow unbounded in long sessions and how would you fix it"
  "architect offline support for the session store with conflict resolution"
  "the release binary segfaults on startup only on intel macs, investigate"
  "propose how to make the eval harness statistically sound for model comparisons"
  "restructure the crates so aster-ai has no dependency on the cli settings"
  "implement sandboxed execution for untrusted skills with landlock and seatbelt"
  "our token cost estimates drift 30 percent from provider invoices, find the systemic cause"
)

correct=0; total=0
run_set() {
  local label=$1; shift
  for p in "$@"; do
    pick=$(aster mom route "$p" 2>/dev/null | head -1 | cut -d' ' -f1)
    total=$((total+1))
    mark="✗"
    if [[ "$pick" == "$label" ]]; then correct=$((correct+1)); mark="✓"; fi
    printf "%s want=%-8s got=%-8s | %s\n" "$mark" "$label" "${pick:-none}" "$p"
  done
}
run_set everyday "${easy[@]}"
run_set deep "${hard[@]}"
echo "---"
echo "accuracy: $correct/$total"
