use std::sync::Arc;

use super::*;

fn session(mode: Mode) -> Session {
    let permissions = PermissionsConfig {
        mode,
        ..PermissionsConfig::default()
    };
    Session {
        id: "test".into(),
        repo_root: std::env::temp_dir(),
        client: Mutex::new(AiClient::new("http://localhost", "k", "m1")),
        ctx: SessionCtx::default(),
        grants: Arc::new(Grants::default()),
        history: Mutex::new(Vec::new()),
        permissions: Mutex::new(permissions.clone()),
        policy: Mutex::new(Policy::compile(&permissions).unwrap()),
        cancel_requested: std::sync::atomic::AtomicBool::new(false),
        cancel: Notify::new(),
        running: std::sync::atomic::AtomicBool::new(false),
    }
}

use std::sync::atomic::Ordering;

#[test]
fn a_fireworks_p_version_reads_as_a_point_release() {
    assert_eq!(
        model_short("fireworks/glm-5p3-flash-low"),
        "GLM 5.3 Flash Low"
    );
}

#[test]
fn a_mid_turn_prompt_steers_and_an_idle_one_does_not() {
    let session = session(Mode::Auto);
    assert!(!session.steer("hello"));
    session.running.store(true, Ordering::SeqCst);
    assert!(session.steer("hello"));
    assert_eq!(session.ctx.injected.lock().unwrap().clone(), vec!["hello"]);
}

#[test]
fn set_mode_flips_yolo_with_it() {
    let session = session(Mode::Edit);
    assert!(!session.ctx.yolo.load(std::sync::atomic::Ordering::Relaxed));

    session.set_mode(Mode::Yolo).unwrap();
    assert!(session.ctx.yolo.load(std::sync::atomic::Ordering::Relaxed));

    session.set_mode(Mode::Edit).unwrap();
    assert!(!session.ctx.yolo.load(std::sync::atomic::Ordering::Relaxed));
}
