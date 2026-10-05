use aster_models::Finding;

#[derive(Debug, Clone)]
pub enum Progress {
    Phase(String),
    Token {
        stage: String,
        delta: String,
    },
    Hypothesized {
        count: usize,
    },
    Verifying {
        index: usize,
        total: usize,
        title: String,
    },
    Confirmed(Box<Finding>),
    Refuted {
        title: String,
        reason: String,
        file: String,
        line: i32,
        severity: String,
        category: String,
        why: RuledOut,
    },
    Done {
        summary: String,
        total: usize,
    },
}

/// Why a candidate never became a finding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuledOut {
    /// The check read the evidence and decided the problem is not real.
    NotReal,
    /// The check thought it real, but below the confidence cut.
    Unsure { confidence: f32 },
    /// The check itself errored, so the candidate was dropped unjudged.
    CheckFailed,
}

pub type ProgressSink = Option<std::sync::mpsc::Sender<Progress>>;

pub(crate) fn emit(sink: &ProgressSink, event: Progress) {
    if let Some(tx) = sink {
        let _ = tx.send(event);
    }
}
