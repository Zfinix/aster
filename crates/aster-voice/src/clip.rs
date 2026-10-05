use std::time::Duration;

/// Mono 16-bit audio, the smallest shape every provider accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

impl Clip {
    /// Raw little-endian 16-bit mono PCM, the format speech replies come in.
    pub fn from_pcm(bytes: &[u8], sample_rate: u32) -> Self {
        Self {
            samples: bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect(),
            sample_rate,
        }
    }

    pub fn duration(&self) -> Duration {
        if self.sample_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(self.sample_rate))
    }

    pub fn wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let byte_rate = self.sample_rate * 2;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in &self.samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
