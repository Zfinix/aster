use super::*;

#[test]
fn wav_is_a_mono_16_bit_pcm_file() {
    let clip = Clip {
        samples: vec![0, 1, -1, i16::MAX],
        sample_rate: 16_000,
    };
    let mut expected = Vec::new();
    expected.extend_from_slice(b"RIFF");
    expected.extend_from_slice(&44u32.to_le_bytes());
    expected.extend_from_slice(b"WAVEfmt ");
    expected.extend_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
    expected.extend_from_slice(&16_000u32.to_le_bytes());
    expected.extend_from_slice(&32_000u32.to_le_bytes());
    expected.extend_from_slice(&[2, 0, 16, 0]);
    expected.extend_from_slice(b"data");
    expected.extend_from_slice(&8u32.to_le_bytes());
    expected.extend_from_slice(&[0, 0, 1, 0, 0xff, 0xff, 0xff, 0x7f]);
    assert_eq!(clip.wav(), expected);
}

#[test]
fn duration_follows_the_sample_rate() {
    let clip = Clip {
        samples: vec![0; 24_000],
        sample_rate: 48_000,
    };
    assert_eq!(clip.duration(), Duration::from_millis(500));
}
