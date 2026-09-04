use crate::{Schedule, program_args};

#[test]
fn program_args_carry_schedule_and_notify() {
    let sched = Schedule {
        name: "nightly".into(),
        cron: "0 9 * * *".into(),
        agent: "sentinel".into(),
        task: "review".into(),
        notify: true,
    };
    let args = program_args(
        std::path::Path::new("/bin/aster"),
        &sched,
        std::path::Path::new("/repo"),
    );
    assert_eq!(args[1], "run");
    assert!(args.contains(&"--schedule".to_string()));
    assert!(args.contains(&"nightly".to_string()));
    assert!(args.contains(&"--notify".to_string()));
    assert!(args.contains(&"--cwd".to_string()));
}
