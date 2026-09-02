use super::*;

fn grants(roots: &[&str]) -> Grants {
    Grants::new(roots.iter().map(PathBuf::from))
}

#[test]
fn a_grant_covers_everything_beneath_it() {
    let g = grants(&["/Users/x/Desktop"]);
    assert!(g.allows(Path::new("/Users/x/Desktop")));
    assert!(g.allows(Path::new("/Users/x/Desktop/notes/todo.md")));
}

#[test]
fn siblings_and_parents_stay_out() {
    let g = grants(&["/Users/x/Desktop"]);
    assert!(!g.allows(Path::new("/Users/x")));
    assert!(!g.allows(Path::new("/Users/x/Downloads/a.txt")));
    assert!(!g.allows(Path::new("/Users/x/DesktopOld/a.txt")));
}

#[test]
fn granting_under_an_existing_grant_is_a_no_op() {
    let g = grants(&["/Users/x/Desktop"]);
    g.grant(PathBuf::from("/Users/x/Desktop/notes"));
    assert_eq!(g.granted(), [PathBuf::from("/Users/x/Desktop")]);
}

#[test]
fn granting_at_runtime_opens_the_directory() {
    let g = Grants::default();
    assert!(!g.allows(Path::new("/tmp/work/a.txt")));
    g.grant(PathBuf::from("/tmp/work"));
    assert!(g.allows(Path::new("/tmp/work/a.txt")));
    assert_eq!(g.granted(), [PathBuf::from("/tmp/work")]);
}

#[test]
fn a_credential_grant_is_scoped_to_one_command() {
    let grants = CommandGrants::default();
    let gh_config = PathBuf::from("/home/u/.config/gh");
    grants.grant("gh", gh_config.clone());

    assert!(grants.allows("gh", &gh_config));
    assert!(!grants.allows("cat", &gh_config));
    assert!(!grants.allows("curl", &gh_config));
}

#[test]
fn a_credential_grant_covers_paths_under_it() {
    let grants = CommandGrants::default();
    grants.grant("gh", PathBuf::from("/home/u/.config/gh"));
    assert!(grants.allows("gh", Path::new("/home/u/.config/gh/hosts.yml")));
    assert!(!grants.allows("gh", Path::new("/home/u/.config/other")));
}

#[test]
fn granting_twice_does_not_grow_the_set() {
    let grants = CommandGrants::default();
    let dir = PathBuf::from("/home/u/.aws");
    grants.grant("aws", dir.clone());
    grants.grant("aws", dir.clone());
    grants.grant("aws", dir.join("cli"));
    assert_eq!(grants.granted().len(), 1);
}

#[test]
fn dirs_for_returns_only_that_commands_directories() {
    let grants = CommandGrants::new([
        ("gh".to_string(), PathBuf::from("/home/u/.config/gh")),
        ("aws".to_string(), PathBuf::from("/home/u/.aws")),
    ]);
    assert_eq!(grants.dirs_for("gh"), [PathBuf::from("/home/u/.config/gh")]);
    assert_eq!(grants.dirs_for("kubectl"), Vec::<PathBuf>::new());
}
