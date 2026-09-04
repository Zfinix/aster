//! Actions a tool wants to perform, evaluated by [`crate::Policy`].

/// A file operation the policy decides on. Paths are repo-relative and already
/// validated against escape upstream (`edits::resolve_in_repo`).
#[derive(Debug)]
pub enum Action<'a> {
    Edit {
        path: &'a str,
    },
    Read {
        path: &'a str,
    },
    Exec {
        binary: &'a str,
        args: &'a [&'a str],
    },
}
