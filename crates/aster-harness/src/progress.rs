use aster_models::Finding;

#[derive(Debug, Clone)]
pub enum Progress {
    Phase(String),
    /// A live token delta streamed from the model for a given stage
    /// (e.g. "hypothesize", "verify"). Emitted as the model produces output.
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
    },
    Done {
        summary: String,
        total: usize,
    },
}

pub type ProgressSink = Option<std::sync::mpsc::Sender<Progress>>;

pub(crate) fn emit(sink: &ProgressSink, event: Progress) {
    if let Some(tx) = sink {
        let _ = tx.send(event);
    }
}
