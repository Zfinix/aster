use super::*;

fn rule(text: &str) -> Rule {
    Rule::parse(text).expect(text)
}

fn exec<'a>(binary: &'a str, args: &'a [&'a str]) -> Action<'a> {
    Action::Exec { binary, args }
}

#[test]
fn a_path_rule_matches_its_glob() {
    let r = rule("Edit(src/**)");
    assert!(r.matches(&Action::Edit { path: "src/lib.rs" }));
    assert!(!r.matches(&Action::Edit { path: "docs/x.md" }));
}

#[test]
fn a_rule_only_matches_its_own_tool() {
    let r = rule("Read(**/.env)");
    assert!(r.matches(&Action::Read { path: ".env" }));
    assert!(!r.matches(&Action::Edit { path: ".env" }));
}

#[test]
fn a_bare_tool_name_covers_everything_it_gates() {
    let r = rule("Bash");
    assert!(r.matches(&exec("anything", &[])));
    assert!(!r.matches(&Action::Edit { path: "src/lib.rs" }));
}

#[test]
fn a_star_specifier_is_the_same_as_a_bare_name() {
    assert!(rule("Edit(*)").matches(&Action::Edit { path: "any/path" }));
}

#[test]
fn a_command_prefix_matches_the_rest_of_the_line() {
    let r = rule("Bash(cargo test:*)");
    assert!(r.matches(&exec("cargo", &["test", "--all"])));
    assert!(r.matches(&exec("cargo", &["test"])));
    assert!(!r.matches(&exec("cargo", &["build"])));
}

#[test]
fn a_prefix_stops_at_a_word_boundary() {
    let r = rule("Bash(rm:*)");
    assert!(r.matches(&exec("rm", &["-rf", "target"])));
    assert!(!r.matches(&exec("rmdir", &["target"])));
}

#[test]
fn an_exact_command_rule_needs_the_whole_line() {
    let r = rule("Bash(git status)");
    assert!(r.matches(&exec("git", &["status"])));
    assert!(!r.matches(&exec("git", &["status", "--short"])));
}

#[test]
fn a_command_rule_reaches_inside_a_shell_invocation() {
    let r = rule("Bash(sudo:*)");
    assert!(r.matches(&exec("bash", &["-lc", "cargo build && sudo make install"])));
    assert!(!r.matches(&exec("bash", &["-lc", "cargo build && make install"])));
}

#[test]
fn a_command_rule_sees_past_a_leading_env_assignment() {
    assert!(
        rule("Bash(curl:*)").matches(&exec("bash", &["-lc", "RUST_LOG=debug curl example.com"]))
    );
}

#[test]
fn parsing_rejects_an_unknown_tool() {
    let err = Rule::parse("Write(src/**)").unwrap_err().to_string();
    assert!(err.contains("Edit, Read, or Bash"), "{err}");
}

#[test]
fn parsing_rejects_a_missing_paren() {
    assert!(Rule::parse("Edit(src/**").is_err());
}

#[test]
fn parsing_rejects_a_broken_glob() {
    assert!(Rule::parse("Edit(src/[)").is_err());
}
