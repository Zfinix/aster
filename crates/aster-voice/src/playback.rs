//! Play a [`Clip`] on the default output device, resampled to the device rate.

use crate::speech::Hush;
use crate::{Clip, VoiceError};

pub(crate) fn play(clip: &Clip, hush: &Hush) -> Result<(), VoiceError> {
    imp::play(clip, hush)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

    use super::{Clip, Hush, VoiceError};

    pub(super) fn play(clip: &Clip, hush: &Hush) -> Result<(), VoiceError> {
        let no_speaker = |e: &dyn std::fmt::Display| VoiceError::NoSpeaker(e.to_string());
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| VoiceError::NoSpeaker("no output device".into()))?;
        let supported = device.default_output_config().map_err(|e| no_speaker(&e))?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let samples = Arc::new(resample(clip, config.sample_rate));
        let done = Arc::new(AtomicBool::new(false));
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, &config, &samples, &done),
            SampleFormat::I16 => build::<i16>(&device, &config, &samples, &done),
            SampleFormat::I32 => build::<i32>(&device, &config, &samples, &done),
            SampleFormat::U16 => build::<u16>(&device, &config, &samples, &done),
            other => {
                return Err(VoiceError::NoSpeaker(format!(
                    "unsupported sample format {other}"
                )));
            }
        }
        .map_err(|e| no_speaker(&e))?;
        stream.play().map_err(|e| no_speaker(&e))?;
        while !done.load(Ordering::Relaxed) && !hush.is_set() {
            std::thread::sleep(crate::speech::POLL);
        }
        Ok(())
    }

    fn resample(clip: &Clip, rate: u32) -> Vec<f32> {
        if clip.sample_rate == 0 || clip.samples.is_empty() {
            return Vec::new();
        }
        let step = f64::from(clip.sample_rate) / f64::from(rate);
        let len = (clip.samples.len() as f64 / step) as usize;
        (0..len)
            .map(|i| {
                let at = i as f64 * step;
                let lo = at as usize;
                let hi = (lo + 1).min(clip.samples.len() - 1);
                let frac = (at - lo as f64) as f32;
                let a = f32::from(clip.samples[lo]) / f32::from(i16::MAX);
                let b = f32::from(clip.samples[hi]) / f32::from(i16::MAX);
                a + (b - a) * frac
            })
            .collect()
    }

    fn build<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        samples: &Arc<Vec<f32>>,
        done: &Arc<AtomicBool>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = usize::from(config.channels.max(1));
        let samples = Arc::clone(samples);
        let done = Arc::clone(done);
        let next = AtomicUsize::new(0);
        device.build_output_stream::<T, _, _>(
            *config,
            move |out: &mut [T], _: &_| {
                for frame in out.chunks_mut(channels) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let value = samples.get(i).copied().unwrap_or_else(|| {
                        done.store(true, Ordering::Relaxed);
                        0.0
                    });
                    frame.fill(T::from_sample(value));
                }
            },
            |err| tracing::warn!("speaker stream error: {err}"),
            None,
        )
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::{Clip, Hush, VoiceError};

    pub(super) fn play(_clip: &Clip, _hush: &Hush) -> Result<(), VoiceError> {
        Err(VoiceError::Unsupported)
    }
}
