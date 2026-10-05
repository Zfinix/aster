//! Capture from the default input device. The stream lives on its own thread
//! because a cpal stream is not `Send` on every host.

use std::sync::mpsc;
use std::thread::JoinHandle;

use crate::{Clip, VoiceError};

/// A microphone that is listening. [`Recording::finish`] closes it and
/// returns what it heard; dropping it discards the audio.
pub struct Recording {
    stop: mpsc::Sender<()>,
    worker: JoinHandle<Clip>,
}

impl Recording {
    pub fn start() -> Result<Self, VoiceError> {
        let (stop, stopped) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let worker = std::thread::spawn(move || imp::capture(&ready, &stopped));
        match started.recv() {
            Ok(Ok(())) => Ok(Self { stop, worker }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(VoiceError::NoMicrophone("capture thread exited".into())),
        }
    }

    /// Blocks until the capture thread hands back its audio.
    pub fn finish(self) -> Clip {
        let _ = self.stop.send(());
        self.worker.join().unwrap_or(Clip {
            samples: Vec::new(),
            sample_rate: 0,
        })
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use std::sync::{Arc, Mutex, mpsc};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

    use crate::{Clip, MAX_RECORDING, VoiceError};

    type Samples = Arc<Mutex<Vec<i16>>>;

    pub(super) fn capture(
        ready: &mpsc::Sender<Result<(), VoiceError>>,
        stopped: &mpsc::Receiver<()>,
    ) -> Clip {
        let samples = Samples::default();
        let opened = open(&samples);
        let (stream, sample_rate) = match opened {
            Ok(opened) => opened,
            Err(err) => {
                let _ = ready.send(Err(err));
                return Clip {
                    samples: Vec::new(),
                    sample_rate: 0,
                };
            }
        };
        let _ = ready.send(Ok(()));
        let _ = stopped.recv_timeout(MAX_RECORDING);
        drop(stream);
        let samples = std::mem::take(&mut *samples.lock().unwrap_or_else(|e| e.into_inner()));
        Clip {
            samples,
            sample_rate,
        }
    }

    fn open(samples: &Samples) -> Result<(cpal::Stream, u32), VoiceError> {
        let no_mic = |e: &dyn std::fmt::Display| VoiceError::NoMicrophone(e.to_string());
        let device = cpal::default_host()
            .default_input_device()
            .ok_or_else(|| VoiceError::NoMicrophone("no input device".into()))?;
        let supported = device.default_input_config().map_err(|e| no_mic(&e))?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let rate = config.sample_rate;
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, &config, samples),
            SampleFormat::I16 => build::<i16>(&device, &config, samples),
            SampleFormat::I32 => build::<i32>(&device, &config, samples),
            SampleFormat::U16 => build::<u16>(&device, &config, samples),
            other => {
                return Err(VoiceError::NoMicrophone(format!(
                    "unsupported sample format {other}"
                )));
            }
        }
        .map_err(|e| no_mic(&e))?;
        stream.play().map_err(|e| no_mic(&e))?;
        Ok((stream, rate))
    }

    fn build<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        samples: &Samples,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample,
        i16: FromSample<T>,
    {
        let channels = usize::from(config.channels.max(1));
        let cap = (MAX_RECORDING.as_secs() * u64::from(config.sample_rate)) as usize;
        let samples = Arc::clone(samples);
        device.build_input_stream::<T, _, _>(
            *config,
            move |data: &[T], _: &_| {
                let mut buf = samples.lock().unwrap_or_else(|e| e.into_inner());
                let room = cap.saturating_sub(buf.len());
                buf.extend(
                    data.iter()
                        .step_by(channels)
                        .take(room)
                        .map(|s| s.to_sample::<i16>()),
                );
            },
            |err| tracing::warn!("microphone stream error: {err}"),
            None,
        )
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use std::sync::mpsc;

    use crate::{Clip, VoiceError};

    pub(super) fn capture(
        ready: &mpsc::Sender<Result<(), VoiceError>>,
        _stopped: &mpsc::Receiver<()>,
    ) -> Clip {
        let _ = ready.send(Err(VoiceError::Unsupported));
        Clip {
            samples: Vec::new(),
            sample_rate: 0,
        }
    }
}
