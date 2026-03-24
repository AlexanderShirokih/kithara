//! WAV file generation helpers for tests.

use crate::fixture_protocol::SAW_PERIOD;

/// Build a 44-byte PCM WAV header.
///
/// - `data_size = None` → streaming header (sizes = `0xFFFFFFFF`).
/// - `data_size = Some(n)` → standard header with real sizes.
#[must_use]
pub fn build_wav_header(sample_rate: u32, channels: u16, data_size: Option<u64>) -> [u8; 44] {
    let bytes_per_sample: u16 = 2;
    let byte_rate = sample_rate * channels as u32 * bytes_per_sample as u32;
    let block_align = channels * bytes_per_sample;
    let (file_size_val, data_size_val) = data_size
        .map_or((0xFFFF_FFFFu32, 0xFFFF_FFFFu32), |size| {
            (36 + size as u32, size as u32)
        });

    let mut buf = Vec::with_capacity(44);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size_val.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&(bytes_per_sample * 8).to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size_val.to_le_bytes());

    let mut h = [0u8; 44];
    h.copy_from_slice(&buf);
    h
}

/// Create a WAV file with sine wave samples.
///
/// Parameters:
/// - `sample_count`: number of audio frames
/// - `sample_rate`: e.g. 44100
/// - `channels`: e.g. 2 for stereo
#[must_use]
pub fn create_test_wav(sample_count: usize, sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_size = sample_count * channels as usize * 2;
    let header = build_wav_header(sample_rate, channels, Some(data_size as u64));

    let mut wav = Vec::with_capacity(44 + data_size);
    wav.extend_from_slice(&header);

    for i in 0..sample_count {
        let sample = ((i as f32 * 0.1).sin() * 32767.0) as i16;
        for _ in 0..channels {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }

    wav
}

/// Create WAV with saw-tooth pattern, sized exactly to `total_bytes`.
///
/// Stereo 44100 Hz, 16-bit PCM. L and R channels get the same value per frame.
#[must_use]
pub fn create_saw_wav(total_bytes: usize) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44100;
    const CHANNELS: u16 = 2;

    let bytes_per_frame = CHANNELS as usize * 2;
    let data_size = total_bytes - 44;
    let frame_count = data_size / bytes_per_frame;
    let data_size = frame_count * bytes_per_frame;
    let header = build_wav_header(SAMPLE_RATE, CHANNELS, Some(data_size as u64));

    let mut wav = Vec::with_capacity(total_bytes);
    wav.extend_from_slice(&header);

    for i in 0..frame_count {
        let sample = ((i % SAW_PERIOD) as i32 - 32768) as i16;
        for _ in 0..CHANNELS {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }

    wav.resize(total_bytes, 0);
    wav
}
