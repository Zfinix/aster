---
name: structural-edits
description: Structural search and rewrite with the ast_grep and ast_edit tools. Use when the same pattern appears in many places, when text search matches comments or strings, or when a rename or codemod spans multiple files.
---

# Structural edits

1. **ast_grep over search_files when syntax matters.** `ast_grep` with a
   pattern like `dbg!($X)` or `fn $NAME($$$ARGS)` matches real code, not
   comments or string literals. Plain text search is fine for identifiers
   that appear nowhere else. Scope with `path` to one directory when the
   change is local.
2. **ast_edit over repeated edit_file when the same change repeats.** One
   pattern plus rewrite applies everywhere at once and returns a diff.
   Three or more identical edits means ast_edit; one or two means edit_file.
   Example: pattern `dbg!($X)` with rewrite `eprintln!("{:?}", $X)` removes
   every debug print in the scope in one approval.
3. **Metavariables carry the capture.** `$X` captures a single node,
   `$$$ARGS` captures a list. Reuse the same name in the rewrite; a name that
   appears only in the rewrite is an error, not an insertion.
4. **Patterns must parse as the target language.** A pattern that is not
   valid syntax in the file's language matches nothing, silently. Write the
   pattern in the language you are editing, and keep it narrow with context
   (`$OBJ.foo($$$ARGS)` rather than `foo($$$ARGS)`) so unrelated calls do not
   match.
5. **Patterns exclude trailing semicolons.** A pattern for `dbg!($X)`
   matches the call, not the `;` after it. Do not put a semicolon in the
   pattern or the rewrite unless the statement itself is the target.
6. **Run ast_grep first when unsure.** A dry search with the same pattern
   shows exactly what ast_edit would touch, with no approval needed. If the
   match list surprises you, fix the pattern before rewriting.
7. **Read the diff at approval time.** A too-broad pattern rewrites code you
   did not intend, and the diff is the only place you will see that. Every
   changed file goes through the same approval as edit_file; a Deny on one
   file stops the whole batch.