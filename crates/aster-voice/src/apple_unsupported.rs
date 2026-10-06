//! On-device recognition is only wired up for macOS so far.

use std::sync::mpsc;

use crate::{Heard, VoiceError};

pub fn relaunch_as_own_app() -> std::io::Result<Option<i32>> {
    Ok(None)
}

pub fn ask_for_microphone() -> Result<(), VoiceError> {
    Ok(())
}

pub fn listen_on_device(
    _audio: mpsc::Receiver<Vec<i16>>,
    _sample_rate: u32,
) -> Result<mpsc::Receiver<Heard>, VoiceError> {
    Err(VoiceError::Unsupported)
}
