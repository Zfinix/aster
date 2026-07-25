//! Actions a tool wants to perform, evaluated by [`crate::Policy`].

/// A file operation the policy decides on. Paths are repo-relative and already
/// validated against escape (`..`, absolute, symlinked parents) upstream by the
/// caller (`edits::resolve_in_repo`); the policy only reasons about the path
/// string, never touches disk.
pub enum Action<'a> {
    /// Write to a repo file.
    Edit { path: &'a str },
    /// Read a repo file.
    Read { path: &'a str },
}
