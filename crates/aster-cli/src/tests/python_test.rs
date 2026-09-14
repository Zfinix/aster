#![cfg(test)]

use super::*;
use clap::Parser;

#[derive(Debug, Parser)]
struct Cli {
    #[command(flatten)]
    args: PythonArgs,
}

fn parse(argv: &[&str]) -> Result<PythonArgs> {
    let line = std::iter::once("python").chain(argv.iter().copied());
    Ok(Cli::try_parse_from(line)?.args)
}

#[test]
fn a_module_takes_the_rest_of_the_line() {
    // `-m` used to be read as the script to open, which is what made
    // `aster python -m http.server` fail with "could not read -m".
    let args = parse(&["-m", "http.server", "--bind", "127.0.0.1", "8000"]).unwrap();

    assert_eq!(args.module, ["http.server", "--bind", "127.0.0.1", "8000"]);
    assert!(args.rest.is_empty());
    assert!(args.code.is_none());
}

#[test]
fn a_script_keeps_its_own_flags() {
    let args = parse(&["report.py", "--since", "-1d"]).unwrap();

    assert_eq!(args.rest, ["report.py", "--since", "-1d"]);
    assert!(args.module.is_empty());
}

#[test]
fn code_and_a_module_are_not_both_runnable() {
    assert!(parse(&["-c", "print(1)", "-m", "json.tool"]).is_err());
}
