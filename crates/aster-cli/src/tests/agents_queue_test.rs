use std::sync::Arc;

use super::BackgroundAgents;
use crate::agents::{AgentTask, TaskReport};

fn task(agent: &str, text: &str) -> AgentTask {
    AgentTask {
        agent: agent.to_string(),
        task: text.to_string(),
    }
}

fn report(agent: &str, text: &str, error: Option<String>) -> TaskReport {
    TaskReport {
        agent: agent.to_string(),
        task: text.to_string(),
        report: error.is_none().then(|| text.to_string()),
        error,
    }
}

/// A runner that calls on_complete per task, like run_swarm does, then returns
/// the same reports.
fn stub_runner(reports: Vec<TaskReport>) -> super::SwarmRunner {
    Arc::new(
        move |tasks: Vec<AgentTask>, callbacks: super::TaskCallbacks| {
            let reports = reports.clone();
            Box::pin(async move {
                for (i, t) in tasks.iter().enumerate() {
                    let r = reports
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| report(&t.agent, &t.task, None));
                    (callbacks.on_complete)(r.clone());
                }
                reports
            }) as _
        },
    )
}

#[tokio::test]
async fn reports_land_in_the_attached_injected_queue() {
    let queue = Arc::new(BackgroundAgents::new());
    let injected: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    queue.attach(injected.clone(), None);

    let id = queue
        .submit(
            vec![task("scout", "look around")],
            stub_runner(vec![report("scout", "look around", None)]),
        )
        .expect("submit");
    let _ = id;
    // The driver runs the batch and delivers the report.
    for _ in 0..100 {
        if !injected.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let lines = injected.lock().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("Background agent scout finished"),
        "{}",
        lines[0]
    );
}

#[tokio::test]
async fn reports_park_when_no_turn_is_attached_and_flush_on_attach() {
    let queue = Arc::new(BackgroundAgents::new());
    queue
        .submit(
            vec![task("scout", "look around")],
            stub_runner(vec![report("scout", "look around", None)]),
        )
        .expect("submit");
    // No attach yet: wait for the report to park.
    for _ in 0..100 {
        if queue.status_text().contains("batch history") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let injected: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    queue.attach(injected.clone(), None);
    let lines = injected.lock().unwrap();
    assert_eq!(lines.len(), 1, "parked report flushes on attach: {lines:?}");
}

#[tokio::test]
async fn failures_report_as_failed() {
    let queue = Arc::new(BackgroundAgents::new());
    let injected: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    queue.attach(injected.clone(), None);
    queue
        .submit(
            vec![task("scout", "blow up")],
            stub_runner(vec![report("scout", "blow up", Some("it broke".into()))]),
        )
        .expect("submit");
    for _ in 0..100 {
        if !injected.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let lines = injected.lock().unwrap();
    assert!(lines[0].contains("failed"), "{lines:?}");
    assert!(lines[0].contains("it broke"), "{lines:?}");
}

#[tokio::test]
async fn the_pending_queue_has_a_cap() {
    let queue = Arc::new(BackgroundAgents::new());
    // A runner that never finishes keeps the first batch running, so every
    // later submit piles into the pending queue.
    let stuck: super::SwarmRunner =
        Arc::new(|_tasks: Vec<AgentTask>, _cb: super::TaskCallbacks| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                Vec::new()
            }) as _
        });
    queue
        .submit(vec![task("scout", "first")], stuck)
        .expect("first submit");
    let filler = |i: usize| {
        let r = Arc::new(move |_tasks: Vec<AgentTask>, _cb: super::TaskCallbacks| {
            Box::pin(async move { Vec::new() }) as _
        });
        (r, vec![task("scout", &format!("filler {i}"))])
    };
    let mut last = Ok(0);
    for i in 0..40 {
        let (runner, tasks) = filler(i);
        last = queue.submit(tasks, runner);
    }
    assert!(last.is_err(), "the cap should reject eventually");
}

#[tokio::test]
async fn status_lists_running_and_done_work() {
    let queue = Arc::new(BackgroundAgents::new());
    let injected: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    queue.attach(injected.clone(), None);
    queue
        .submit(
            vec![task("scout", "look around")],
            stub_runner(vec![report("scout", "look around", None)]),
        )
        .expect("submit");
    for _ in 0..100 {
        if queue.status_text().contains("batch history") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let status = queue.status_text();
    assert!(status.contains("scout"), "{status}");
}
