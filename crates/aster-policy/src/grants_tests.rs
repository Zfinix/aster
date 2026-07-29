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
