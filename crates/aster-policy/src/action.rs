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
    /// Run a command. `binary` is the program name (e.g. "cargo", "rg"),
    /// `args` are the arguments. The policy decides whether to allow,
    /// prompt, or deny based on the command.
    Exec {
        binary: &'a str,
        args: &'a [&'a str],
    },
}
