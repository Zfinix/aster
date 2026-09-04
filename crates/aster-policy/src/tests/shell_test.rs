use super::*;

fn lines(binary: &str, args: &[&str]) -> Vec<String> {
    segments(binary, args)
}

#[test]
fn a_plain_command_is_one_line() {
    assert_eq!(lines("cargo", &["test", "--all"]), ["cargo test --all"]);
}

#[test]
fn a_shell_script_contributes_each_of_its_commands() {
    let out = lines("bash", &["-lc", "cargo build && cargo test | head -5"]);
    assert!(out.contains(&"cargo build".to_string()), "{out:?}");
    assert!(out.contains(&"cargo test".to_string()), "{out:?}");
    assert!(out.contains(&"head -5".to_string()), "{out:?}");
}

#[test]
fn semicolons_and_newlines_separate_commands_too() {
    let out = lines("sh", &["-c", "one; two\nthree"]);
    for expected in ["one", "two", "three"] {
        assert!(out.contains(&expected.to_string()), "{out:?}");
    }
}

#[test]
fn an_operator_inside_quotes_is_not_a_separator() {
    let out = lines("bash", &["-lc", "echo 'a && b'"]);
    assert!(out.contains(&"echo a && b".to_string()), "{out:?}");
}

#[test]
fn a_leading_env_assignment_is_dropped() {
    assert_eq!(lines("FOO=1", &["sudo", "rm"])[0], "sudo rm");
}

#[test]
fn an_argument_with_an_equals_sign_is_kept() {
    assert_eq!(
        lines("cargo", &["test", "--features=a"])[0],
        "cargo test --features=a"
    );
}

#[test]
fn a_shell_inside_a_shell_is_followed() {
    let out = lines("bash", &["-lc", "sh -c 'sudo rm -rf /'"]);
    assert!(out.contains(&"sudo rm -rf /".to_string()), "{out:?}");
}

#[test]
fn nesting_stops_before_it_can_run_away() {
    let mut script = "echo done".to_string();
    for _ in 0..12 {
        script = format!("bash -c \"{}\"", script.replace('"', "'"));
    }
    let out = lines("bash", &["-lc", &script]);
    assert!(out.len() <= 8, "{}", out.len());
}

#[test]
fn a_flag_bundle_still_finds_the_script() {
    let out = lines("bash", &["-lc", "whoami"]);
    assert!(out.contains(&"whoami".to_string()), "{out:?}");
}

#[test]
fn a_shell_without_a_script_flag_is_left_alone() {
    assert_eq!(lines("bash", &["script.sh"]), ["bash script.sh"]);
}
