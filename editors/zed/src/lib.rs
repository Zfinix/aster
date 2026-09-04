use zed_extension_api::{
    self as zed, Command, Extension, SlashCommand, SlashCommandOutput,
    SlashCommandOutputSection, Worktree,
};

const BINARY: &str = "aster";

struct AsterExtension;

impl Extension for AsterExtension {
    fn new() -> Self {
        Self
    }

    fn run_slash_command(
        &self,
        command: SlashCommand,
        args: Vec<String>,
        worktree: Option<&Worktree>,
    ) -> Result<SlashCommandOutput, String> {
        match command.name.as_str() {
            "aster-review" => review(args, worktree),
            other => Err(format!("unknown command: {other}")),
        }
    }
}

fn review(args: Vec<String>, worktree: Option<&Worktree>) -> Result<SlashCommandOutput, String> {
    let mut cmd = Command::new(BINARY)
        .arg("review")
        .args(args.iter().map(String::as_str));
    let _ = worktree;

    let output = cmd.output().map_err(|err| {
        format!("could not run {BINARY}. Is it on your PATH? Run `make install` in the aster repo, or install it from the releases page. ({err})")
    })?;

    if output.status == Some(0) {
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(SlashCommandOutput {
            text: text.clone(),
            sections: vec![SlashCommandOutputSection {
                range: (0..text.len() as u32).into(),
                label: "aster review".to_string(),
            }],
        })
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "aster review failed. Check your changes and try again.\n\n{detail}"
        ))
    }
}

zed::register_extension!(AsterExtension);
