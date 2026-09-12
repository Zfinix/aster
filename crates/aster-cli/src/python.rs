//! `aster python`: Python 3 built into the binary, for the phone, where there
//! is no system python and no way to install one.

use anyhow::{Context, Result};
use clap::Args;
use rustpython::InterpreterBuilderExt;
use rustpython::vm::{InterpreterBuilder, Settings};
use std::io::Read;

#[derive(Debug, Args)]
pub(crate) struct PythonArgs {
    /// Run this code instead of a file; everything after it is `sys.argv[1:]`.
    #[arg(short = 'c', long = "code")]
    pub code: Option<String>,
    /// The script to run (`-` or nothing reads stdin), then its `sys.argv[1:]`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

pub(crate) fn run(args: PythonArgs) -> Result<()> {
    let mut settings = Settings::default();
    let mut rest = args.rest.into_iter();
    let file = match args.code {
        Some(_) => None,
        None => rest.next(),
    };
    let (source, name) = match (args.code, file.as_deref()) {
        (Some(code), _) => (code, "<string>".to_string()),
        (None, None | Some("-")) => {
            let mut source = String::new();
            std::io::stdin()
                .read_to_string(&mut source)
                .context("could not read the script from stdin")?;
            (source, "<stdin>".to_string())
        }
        (None, Some(path)) => {
            let source =
                std::fs::read_to_string(path).with_context(|| format!("could not read {path}"))?;
            (source, path.to_string())
        }
    };
    settings.argv = std::iter::once(name.clone()).chain(rest).collect();

    let interpreter = InterpreterBuilder::new()
        .settings(settings)
        .init_stdlib()
        .interpreter();
    let script_dir = file
        .as_deref()
        .and_then(|path| std::path::Path::new(path).parent())
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    let code = interpreter.run(|vm| {
        vm.insert_sys_path(vm.new_pyobj(script_dir))?;
        let scope = vm.new_scope_with_builtins();
        scope
            .globals
            .set_item("__name__", vm.new_pyobj("__main__"), vm)?;
        scope
            .globals
            .set_item("__file__", vm.new_pyobj(name.clone()), vm)?;
        vm.run_string(scope, &source, name).map(|_| ())
    });
    if code != 0 {
        std::process::exit(code as i32);
    }
    Ok(())
}
