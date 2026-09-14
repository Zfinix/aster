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
    #[arg(short = 'c', long = "code", conflicts_with = "module")]
    pub code: Option<String>,
    /// Run this module from `sys.path`, like `python -m http.server 8000`.
    ///
    /// It takes the rest of the line, so the module's own flags reach it
    /// rather than being read as flags of `aster python`.
    #[arg(
        short = 'm',
        long = "module",
        num_args = 1..,
        allow_hyphen_values = true,
        value_name = "MODULE"
    )]
    pub module: Vec<String>,
    /// The script to run (`-` or nothing reads stdin), then its `sys.argv[1:]`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

/// What the interpreter was asked to run.
enum Target {
    /// Source text, and the name it answers to as `__file__`.
    Source(String, String),
    /// A module on `sys.path`, run the way `python -m` runs one.
    Module(String),
}

pub(crate) fn run(args: PythonArgs) -> Result<()> {
    let mut settings = Settings::default();
    let mut module = args.module.into_iter();
    let mut rest = args.rest.into_iter();
    let named = module.next();
    let file = if args.code.is_some() || named.is_some() {
        None
    } else {
        rest.next()
    };
    let target = match (args.code, named, file.as_deref()) {
        (Some(code), _, _) => Target::Source(code, "<string>".to_string()),
        (None, Some(name), _) => Target::Module(name),
        (None, None, None | Some("-")) => {
            let mut source = String::new();
            std::io::stdin()
                .read_to_string(&mut source)
                .context("could not read the script from stdin")?;
            Target::Source(source, "<stdin>".to_string())
        }
        (None, None, Some(path)) => {
            let source =
                std::fs::read_to_string(path).with_context(|| format!("could not read {path}"))?;
            Target::Source(source, path.to_string())
        }
    };
    // A module's `sys.argv[0]` is its own file, which only runpy can know, so
    // the name here is a placeholder runpy overwrites.
    let name = match &target {
        Target::Source(_, name) => name.clone(),
        Target::Module(name) => name.clone(),
    };
    settings.argv = std::iter::once(name.clone())
        .chain(module)
        .chain(rest)
        .collect();

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
        match &target {
            // runpy writes the module's globals into `sys.modules["__main__"]`,
            // so that module has to exist before it runs; a bare scope has no
            // `__main__` and runpy fails on the lookup.
            Target::Module(name) => {
                vm.new_scope_with_main()?;
                vm.run_module(name)
            }
            Target::Source(source, name) => {
                let scope = vm.new_scope_with_builtins();
                scope
                    .globals
                    .set_item("__name__", vm.new_pyobj("__main__"), vm)?;
                scope
                    .globals
                    .set_item("__file__", vm.new_pyobj(name.clone()), vm)?;
                vm.run_string(scope, source, name.clone()).map(|_| ())
            }
        }
    });
    if code != 0 {
        std::process::exit(code as i32);
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/python_test.rs"]
mod tests;
