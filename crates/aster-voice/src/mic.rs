//! Capture from the default input device. The stream lives on its own thread
//! because a cpal stream is not `Send` on every host.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use crate::{Clip, VoiceError};

/// What the capture thread shares while it runs: the latest loudness, and
/// whether it stopped by itself because the speaker went quiet.
#[derive(Default)]
struct Meter {
    level: AtomicU32,
    ended: AtomicBool,
}

impl Meter {
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    fn set_level(&self, level: f32) {
        self.level.store(level.to_bits(), Ordering::Relaxed);
    }
}

/// A microphone that is listening. It stops by itself once the speaker has
/// finished; [`Recording::finish`] stops it sooner and returns what it heard,
/// and dropping it discards the audio.
pub struct Recording {
    stop: mpsc::Sender<()>,
    worker: JoinHandle<Clip>,
    meter: Arc<Meter>,
}

impl Recording {
    pub fn start() -> Result<Self, VoiceError> {
        let (stop, stopped) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let meter = Arc::new(Meter::default());
        let shared = Arc::clone(&meter);
        let worker = std::thread::spawn(move || imp::capture(&ready, &stopped, &shared));
        match started.recv() {
            Ok(Ok(())) => Ok(Self {
                stop,
                worker,
                meter,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(VoiceError::NoMicrophone("capture thread exited".into())),
        }
    }

    /// Loudness of the latest audio, from 0 to 1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.meter.level.load(Ordering::Relaxed))
    }

    /// True once the microphone has closed by itself.
    pub fn ended(&self) -> bool {
        self.meter.ended.load(Ordering::Relaxed)
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
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

    use super::Meter;
    use crate::vad::Endpoint;
    use crate::{Clip, MAX_RECORDING, VoiceError};

    const SLICE: Duration = Duration::from_millis(50);

    type Samples = Arc<Mutex<Vec<i16>>>;

    pub(super) fn capture(
        ready: &mpsc::Sender<Result<(), VoiceError>>,
        stopped: &mpsc::Receiver<()>,
        meter: &Meter,
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
        let started = Instant::now();
        let mut endpoint = Endpoint::default();
        let mut read = 0;
        while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(SLICE) {
            let level = {
                let buf = samples.lock().unwrap_or_else(|e| e.into_inner());
                let level = rms(&buf[read.min(buf.len())..]);
                read = buf.len();
                level
            };
            meter.set_level(level);
            if endpoint.update(level, SLICE) || started.elapsed() >= MAX_RECORDING {
                meter.ended.store(true, Ordering::Relaxed);
                break;
            }
        }
        meter.set_level(0.0);
        drop(stream);
        let samples = std::mem::take(&mut *samples.lock().unwrap_or_else(|e| e.into_inner()));
        Clip {
            samples,
            sample_rate,
        }
    }

    fn rms(samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples
            .iter()
            .map(|&s| (f64::from(s) / f64::from(i16::MAX)).powi(2))
            .sum();
        (sum / samples.len() as f64).sqrt() as f32
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

    use super::Meter;
    use crate::{Clip, VoiceError};

    pub(super) fn capture(
        ready: &mpsc::Sender<Result<(), VoiceError>>,
        _stopped: &mpsc::Receiver<()>,
        _meter: &Meter,
    ) -> Clip {
        let _ = ready.send(Err(VoiceError::Unsupported));
        Clip {
            samples: Vec::new(),
            sample_rate: 0,
        }
    }
}
