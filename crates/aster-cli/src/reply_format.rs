//! How the client shows a reply, which decides what the model may write in one.

/// Generated from the VS Code webview's component list by
/// `bun run prompt:openui` in `editors/vscode`; about 2.9K tokens.
const OPENUI_PROMPT: &str = include_str!("../prompts/openui.md");

/// What a client can draw in a reply beyond markdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReplyFormat {
    #[default]
    Markdown,
    /// Markdown plus ```openui blocks drawn as OpenUI components.
    OpenUi,
}

impl ReplyFormat {
    /// The system prompt section that teaches this format, when it needs one.
    pub(crate) fn prompt(self) -> Option<&'static str> {
        match self {
            Self::Markdown => None,
            Self::OpenUi => Some(OPENUI_PROMPT),
        }
    }
}
