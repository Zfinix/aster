use crate::{Schedule, program_args};

#[test]
fn program_args_carry_schedule_and_notify() {
    let sched = Schedule {
        name: "nightly".into(),
        cron: "0 9 * * *".into(),
        agent: "sentinel".into(),
        task: "review".into(),
        notify: true,
        notify_url: None,
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

#[test]
fn program_args_carry_notify_url() {
    let sched = Schedule {
        name: "x-post".into(),
        cron: "0 18 * * *".into(),
        agent: "x-scout".into(),
        task: "post".into(),
        notify: true,
        notify_url: Some("https://x.com/chiziaruhoma".into()),
    };
    let args = program_args(
        std::path::Path::new("/bin/aster"),
        &sched,
        std::path::Path::new("/repo"),
    );
    let url_idx = args
        .iter()
        .position(|a| a == "--notify-url")
        .expect("--notify-url present");
    assert_eq!(args[url_idx + 1], "https://x.com/chiziaruhoma");
}
