---
name: lsp-navigation
description: Semantic code navigation and fast error checks with the lsp_diagnostics, lsp_references, and lsp_definitions tools. Use instead of a full build to check whether an edit broke something, or to find real usages of a symbol.
---

# LSP navigation

1. **lsp_diagnostics beats a full build for "did my edit compile".** After
   editing a file, one diagnostics call answers it in milliseconds. A full
   `cargo check` or `tsc` run is for the end of a task, not every edit.
2. **lsp_references finds real usages.** Text search finds the name in
   comments, strings, and unrelated scopes; references finds call sites.
   Use it before renaming or deleting anything. It takes a file plus a
   position, so search for the symbol first, then pass the position of one
   hit.
3. **lsp_definitions jumps to the source.** Use it when a call site's
   types are unclear and you need the signature, instead of guessing the
   file path and reading it.
4. **Line and character are zero-based.** The positions come from the
   file itself; if you just read the file, the line you saw is already
   the right number minus one.
5. **The first call in a language is slow.** The server starts and
   indexes on first use; rust-analyzer can take seconds on a large crate.
   That is startup cost, not a hang. Subsequent calls in the same session
   reuse the running server.
6. **Missing servers are expected.** If a tool says the language server
   is not installed, fall back to text search and a build; do not retry
   or tell the user to install anything unless they ask.