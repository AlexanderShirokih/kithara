//! On-demand audio signal source for testing.
//!
//! `SignalSource<S>` implements `Source` by computing WAV bytes on-the-fly
//! from a `SignalFn` — no internal data buffer, pure computation per read.

use std::{f64::consts::PI, io, io::Error as IoError, ops::Range};

use futures::executor::block_on;
use kithara_platform::time::Duration;
use kithara_storage::WaitOutcome;
use kithara_stream::{
    AudioCodec, ContainerFormat, MediaInfo, ReadOutcome, Source, SourcePhase, Stream, StreamResult,
    StreamType,
};

use crate::fixture_protocol::SAW_PERIOD;
use crate::memory_source::MemoryCoord;
use crate::wav::build_wav_header;

/// Deterministic audio signal generator.
///
/// Implementations must be pure functions: given the same `frame` and
/// `sample_rate`, they must always return the same sample value.
pub trait SignalFn: Send + 'static {
    /// Compute one 16-bit PCM sample at the given frame index.
    fn sample(&self, frame: u64, sample_rate: u32) -> i16;
}

/// Ascending sawtooth wave with period [`SAW_PERIOD`] (65 536 frames).
pub struct Sawtooth;

impl SignalFn for Sawtooth {
    fn sample(&self, frame: u64, _sample_rate: u32) -> i16 {
        ((frame as usize % SAW_PERIOD) as i32 - 32768) as i16
    }
}

/// Descending sawtooth wave with period [`SAW_PERIOD`] (65 536 frames).
pub struct SawtoothDescending;

impl SignalFn for SawtoothDescending {
    fn sample(&self, frame: u64, _sample_rate: u32) -> i16 {
        (32767 - (frame as usize % SAW_PERIOD) as i32) as i16
    }
}

/// Pure sine wave at the given frequency in Hz.
pub struct SineWave(pub f64);

impl SignalFn for SineWave {
    fn sample(&self, frame: u64, sample_rate: u32) -> i16 {
        let t = frame as f64 / sample_rate as f64;
        (f64::sin(2.0 * PI * self.0 * t) * 32767.0) as i16
    }
}

/// Digital silence — all samples are zero.
pub struct Silence;

impl SignalFn for Silence {
    fn sample(&self, _frame: u64, _sample_rate: u32) -> i16 {
        0
    }
}

const HEADER_SIZE: u64 = 44;

fn duration_to_frames(duration: Duration, sample_rate: u32) -> u64 {
    (duration.as_secs_f64() * sample_rate as f64) as u64
}

// ---------------------------------------------------------------------------
// SignalSourceError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[error("signal source error")]
pub struct SignalSourceError;

/// Audio source that computes WAV bytes on-the-fly from a [`SignalFn`].
///
/// No internal data buffer — each `read_at` call generates samples by
/// evaluating the signal function at the requested frame positions.
pub struct SignalSource<S: SignalFn> {
    signal: S,
    coord: MemoryCoord,
    sample_rate: u32,
    channels: u16,
    header: [u8; 44],
    total_frames: Option<u64>,
}

impl<S: SignalFn> SignalSource<S> {
    /// Create an infinite source (streaming WAV header, `len() = None`).
    #[must_use]
    pub fn infinite(signal: S, sample_rate: u32, channels: u16) -> Self {
        let header = build_wav_header(sample_rate, channels, None);
        Self {
            signal,
            coord: MemoryCoord::default(),
            sample_rate,
            channels,
            header,
            total_frames: None,
        }
    }

    /// Create a finite source with the given duration.
    #[must_use]
    pub fn finite(signal: S, sample_rate: u32, channels: u16, duration: Duration) -> Self {
        let total_frames = duration_to_frames(duration, sample_rate);
        let data_bytes = total_frames * channels as u64 * 2;
        let header = build_wav_header(sample_rate, channels, Some(data_bytes));
        Self {
            signal,
            coord: MemoryCoord::default(),
            sample_rate,
            channels,
            header,
            total_frames: Some(total_frames),
        }
    }

    /// Total byte length (header + PCM), or `None` for infinite sources.
    fn total_byte_len(&self) -> Option<u64> {
        self.total_frames
            .map(|f| HEADER_SIZE + f * self.channels as u64 * 2)
    }

    /// Check whether `offset` is past EOF for finite sources.
    fn is_past_eof(&self, offset: u64) -> bool {
        self.total_byte_len().is_some_and(|total| offset >= total)
    }
}

impl<S: SignalFn> Source for SignalSource<S> {
    type Error = SignalSourceError;
    type Topology = ();
    type Layout = ();
    type Coord = MemoryCoord;
    type Demand = ();

    fn topology(&self) -> &Self::Topology {
        &()
    }

    fn layout(&self) -> &Self::Layout {
        &()
    }

    fn coord(&self) -> &Self::Coord {
        &self.coord
    }

    fn wait_range(
        &mut self,
        range: Range<u64>,
        _timeout: Duration,
    ) -> StreamResult<WaitOutcome, Self::Error> {
        if self.is_past_eof(range.start) {
            Ok(WaitOutcome::Eof)
        } else {
            Ok(WaitOutcome::Ready)
        }
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> StreamResult<ReadOutcome, Self::Error> {
        if buf.is_empty() {
            return Ok(ReadOutcome::Data(0));
        }
        if self.is_past_eof(offset) {
            return Ok(ReadOutcome::Data(0));
        }

        let bytes_per_frame = self.channels as u64 * 2;
        let total_byte_len = self.total_byte_len();
        let mut written = 0usize;
        let mut pos = offset;

        // Phase 1: serve bytes from WAV header
        if pos < HEADER_SIZE {
            let header_remaining = (HEADER_SIZE - pos) as usize;
            let n = header_remaining.min(buf.len());
            buf[..n].copy_from_slice(&self.header[pos as usize..pos as usize + n]);
            written += n;
            pos += n as u64;
        }

        // Phase 2: generate PCM on-the-fly
        while written < buf.len() {
            if total_byte_len.is_some_and(|total| pos >= total) {
                break;
            }

            let pcm_offset = pos - HEADER_SIZE;
            let frame = pcm_offset / bytes_per_frame;
            let byte_in_frame = (pcm_offset % bytes_per_frame) as usize;

            // Generate one frame worth of bytes
            let sample = self.signal.sample(frame, self.sample_rate);
            let sample_bytes = sample.to_le_bytes();
            let mut frame_buf = [0u8; 32]; // max 16 channels
            for ch in 0..self.channels as usize {
                frame_buf[ch * 2] = sample_bytes[0];
                frame_buf[ch * 2 + 1] = sample_bytes[1];
            }

            let frame_bytes = bytes_per_frame as usize;
            let available = frame_bytes - byte_in_frame;
            let remaining = buf.len() - written;
            let n = available.min(remaining);

            let n = total_byte_len.map_or(n, |total| n.min((total - pos) as usize));

            buf[written..written + n].copy_from_slice(&frame_buf[byte_in_frame..byte_in_frame + n]);
            written += n;
            pos += n as u64;
        }

        Ok(ReadOutcome::Data(written))
    }

    fn phase_at(&self, range: Range<u64>) -> SourcePhase {
        if self.is_past_eof(range.start) {
            SourcePhase::Eof
        } else {
            SourcePhase::Ready
        }
    }

    fn len(&self) -> Option<u64> {
        self.total_byte_len()
    }

    fn media_info(&self) -> Option<MediaInfo> {
        Some(MediaInfo {
            channels: Some(self.channels),
            codec: Some(AudioCodec::Pcm),
            container: Some(ContainerFormat::Wav),
            sample_rate: Some(self.sample_rate),
            ..MediaInfo::default()
        })
    }
}

/// `StreamType` marker for [`SignalSource`].
pub struct SignalStream<S: SignalFn>(std::marker::PhantomData<S>);

impl<S: SignalFn> StreamType for SignalStream<S> {
    type Config = SignalStreamConfig<S>;
    type Topology = ();
    type Layout = ();
    type Coord = MemoryCoord;
    type Demand = ();
    type Source = SignalSource<S>;
    type Error = io::Error;
    async fn create(config: Self::Config) -> Result<Self::Source, Self::Error> {
        config.source.ok_or_else(|| IoError::other("no source"))
    }

    type Events = ();
}

/// Configuration for [`SignalStream`].
pub struct SignalStreamConfig<S: SignalFn> {
    pub source: Option<SignalSource<S>>,
}

impl<S: SignalFn> Default for SignalStreamConfig<S> {
    fn default() -> Self {
        Self { source: None }
    }
}

/// Create a `Stream` from a `SignalSource`.
#[must_use]
pub fn signal_stream<S: SignalFn>(source: SignalSource<S>) -> Stream<SignalStream<S>> {
    let config = SignalStreamConfig {
        source: Some(source),
    };

    block_on(Stream::new(config)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infinite_len_is_none() {
        let src = SignalSource::infinite(Silence, 44100, 2);
        assert_eq!(src.len(), None);
    }

    #[test]
    fn finite_len() {
        let src = SignalSource::finite(Silence, 44100, 2, Duration::from_secs(1));
        // 44100 frames * 2 channels * 2 bytes + 44 header
        assert_eq!(src.len(), Some(44 + 44100 * 2 * 2));
    }

    #[test]
    fn wav_header_magic() {
        let mut src = SignalSource::infinite(Silence, 44100, 2);
        let mut buf = [0u8; 44];
        let result = src.read_at(0, &mut buf).unwrap();
        assert_eq!(result, ReadOutcome::Data(44));
        assert_eq!(&buf[0..4], b"RIFF");
        assert_eq!(&buf[8..12], b"WAVE");
        assert_eq!(&buf[36..40], b"data");
    }

    #[test]
    fn wav_header_sample_rate() {
        let mut src = SignalSource::infinite(Silence, 48000, 1);
        let mut buf = [0u8; 44];
        src.read_at(0, &mut buf).unwrap();
        let rate = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);
        assert_eq!(rate, 48000);
    }

    #[test]
    fn wav_header_channels() {
        let mut src = SignalSource::infinite(Silence, 44100, 2);
        let mut buf = [0u8; 44];
        src.read_at(0, &mut buf).unwrap();
        let ch = u16::from_le_bytes([buf[22], buf[23]]);
        assert_eq!(ch, 2);
    }

    #[test]
    fn finite_header_has_real_sizes() {
        let src = SignalSource::finite(Silence, 44100, 2, Duration::from_secs(1));
        let data_size = 44100u64 * 2 * 2;
        let file_size = 36 + data_size;
        let h = src.header;
        assert_eq!(
            u32::from_le_bytes([h[4], h[5], h[6], h[7]]),
            file_size as u32
        );
        assert_eq!(
            u32::from_le_bytes([h[40], h[41], h[42], h[43]]),
            data_size as u32
        );
    }

    #[test]
    fn infinite_header_has_streaming_sizes() {
        let src = SignalSource::infinite(Silence, 44100, 2);
        let h = src.header;
        assert_eq!(u32::from_le_bytes([h[4], h[5], h[6], h[7]]), 0xFFFF_FFFF);
        assert_eq!(
            u32::from_le_bytes([h[40], h[41], h[42], h[43]]),
            0xFFFF_FFFF
        );
    }

    #[test]
    fn sawtooth_on_demand() {
        let mut src = SignalSource::infinite(Sawtooth, 44100, 1);
        // Read first 4 PCM bytes (2 frames, mono)
        let mut buf = [0u8; 4];
        src.read_at(44, &mut buf).unwrap();

        // Frame 0: (0 % 65536) - 32768 = -32768
        let s0 = i16::from_le_bytes([buf[0], buf[1]]);
        assert_eq!(s0, -32768);

        // Frame 1: (1 % 65536) - 32768 = -32767
        let s1 = i16::from_le_bytes([buf[2], buf[3]]);
        assert_eq!(s1, -32767);
    }

    #[test]
    fn sawtooth_descending() {
        let mut src = SignalSource::infinite(SawtoothDescending, 44100, 1);
        let mut buf = [0u8; 2];
        src.read_at(44, &mut buf).unwrap();
        let s0 = i16::from_le_bytes([buf[0], buf[1]]);
        assert_eq!(s0, 32767);
    }

    #[test]
    fn sine_first_sample_is_zero() {
        let mut src = SignalSource::infinite(SineWave(440.0), 44100, 1);
        let mut buf = [0u8; 2];
        src.read_at(44, &mut buf).unwrap();
        let s0 = i16::from_le_bytes([buf[0], buf[1]]);
        // sin(0) = 0
        assert_eq!(s0, 0);
    }

    #[test]
    fn silence_all_zeros() {
        let mut src = SignalSource::finite(Silence, 44100, 2, Duration::from_millis(10));
        let pcm_bytes = 44100 * 2 * 2 / 100; // 10ms
        let mut buf = vec![0xFFu8; pcm_bytes as usize];
        src.read_at(44, &mut buf).unwrap();
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn finite_eof() {
        let mut src = SignalSource::finite(Silence, 44100, 1, Duration::from_millis(1));
        let total = src.len().unwrap();
        let mut buf = [0u8; 16];
        let result = src.read_at(total, &mut buf).unwrap();
        assert_eq!(result, ReadOutcome::Data(0));
    }

    #[test]
    fn infinite_always_ready() {
        let src = SignalSource::infinite(Silence, 44100, 2);
        assert_eq!(src.phase_at(0..1), SourcePhase::Ready);
        assert_eq!(src.phase_at(u64::MAX - 1..u64::MAX), SourcePhase::Ready);
    }

    #[test]
    fn finite_phase_eof_past_end() {
        let src = SignalSource::finite(Silence, 44100, 1, Duration::from_secs(1));
        let total = src.len().unwrap();
        assert_eq!(src.phase_at(0..1), SourcePhase::Ready);
        assert_eq!(src.phase_at(total..total + 1), SourcePhase::Eof);
    }

    #[test]
    fn media_info_correct() {
        let src = SignalSource::infinite(Silence, 48000, 2);
        let info = src.media_info().unwrap();
        assert_eq!(info.codec, Some(AudioCodec::Pcm));
        assert_eq!(info.container, Some(ContainerFormat::Wav));
        assert_eq!(info.sample_rate, Some(48000));
        assert_eq!(info.channels, Some(2));
    }

    #[test]
    fn partial_frame_read() {
        let mut src = SignalSource::infinite(Sawtooth, 44100, 2);
        // Read 1 byte at offset 45 (middle of first frame, stereo = 4 bytes/frame)
        let mut buf = [0u8; 1];
        let result = src.read_at(45, &mut buf).unwrap();
        assert_eq!(result, ReadOutcome::Data(1));
        // Frame 0, byte 1 of LE i16 (-32768 = 0x8000 LE = [0x00, 0x80])
        assert_eq!(buf[0], 0x80);
    }

    #[test]
    fn stereo_duplicates_channels() {
        let mut src = SignalSource::infinite(Sawtooth, 44100, 2);
        // One stereo frame = 4 bytes (L_lo, L_hi, R_lo, R_hi)
        let mut buf = [0u8; 4];
        src.read_at(44, &mut buf).unwrap();
        // Both channels should be the same sample
        assert_eq!(buf[0], buf[2]); // lo bytes
        assert_eq!(buf[1], buf[3]); // hi bytes
    }

    #[test]
    fn read_spanning_header_and_pcm() {
        let mut src = SignalSource::infinite(Silence, 44100, 1);
        // Read 48 bytes starting at offset 40 (4 header + 4 PCM)
        let mut buf = [0xFFu8; 8];
        let result = src.read_at(40, &mut buf).unwrap();
        assert_eq!(result, ReadOutcome::Data(8));
        // Last 4 bytes of header (data chunk size 0xFFFFFFFF)
        assert_eq!(&buf[0..4], &0xFFFF_FFFFu32.to_le_bytes());
        // First 4 bytes of PCM (silence = 0)
        assert_eq!(&buf[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn signal_stream_creates_stream() {
        let src = SignalSource::infinite(SineWave(440.0), 44100, 2);
        let stream = signal_stream(src);
        assert_eq!(stream.len(), None);
    }

    #[test]
    fn custom_signal_fn() {
        struct Constant(i16);
        impl SignalFn for Constant {
            fn sample(&self, _frame: u64, _sample_rate: u32) -> i16 {
                self.0
            }
        }

        let mut src = SignalSource::infinite(Constant(1000), 44100, 1);
        let mut buf = [0u8; 2];
        src.read_at(44, &mut buf).unwrap();
        assert_eq!(i16::from_le_bytes(buf), 1000);
    }
}
