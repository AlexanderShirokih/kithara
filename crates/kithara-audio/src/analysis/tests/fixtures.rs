use std::{collections::VecDeque, num::NonZeroU32};

use kithara_bufpool::PcmPool;
use kithara_decode::{DecodeError, PcmChunk, PcmMeta, PcmSpec, TrackMetadata};
use kithara_events::EventBus;
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
use kithara_platform::sync::Arc;
use kithara_platform::time::Duration;
use num_traits::cast::{AsPrimitive, ToPrimitive};
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
use unimock::{MockFn, Unimock, matching};

#[cfg(feature = "analysis-waveform")]
use crate::traits::PendingReason;
use crate::traits::{ChunkOutcome, PcmControl, PcmRead, PcmSession, ReadOutcome, SeekOutcome};
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
use crate::{
    analysis::{
        analyzer::TrackAnalysis,
        beat::{BeatDetector, BeatDetectorMock, BeatMark, RawBeats},
    },
    waveform::bucket::Waveform,
};

/// Source frames a marker may move by across the routes a track can be
/// covered through. Each covered run is resampled under its own stream, so a
/// splice is worth well under a millisecond; this is generous against that.
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
pub(super) const MARKER_TOLERANCE: u64 = 64;

/// The two artifacts of a snapshot, in the form every route comparison uses.
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
pub(super) type Artifacts = (Waveform, Vec<u64>);

/// Every window reports one beat a quarter of the way in, so a marker's
/// position is a pure function of where its window sits.
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
pub(super) fn beat_detector() -> Box<dyn BeatDetector> {
    Box::new(Unimock::new(
        BeatDetectorMock
            .each_call(matching!(_))
            .answers_arc(Arc::new(|_, _| {
                Ok(RawBeats {
                    beats: vec![BeatMark::at(0.25)],
                    downbeats: vec![BeatMark::at(0.25)],
                })
            })),
    ))
}

#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
pub(super) fn artifacts(snapshot: &TrackAnalysis) -> Artifacts {
    (
        snapshot.waveform().cloned().unwrap_or_default(),
        snapshot
            .beat()
            .map(|beat| beat.grid().beats().to_vec())
            .unwrap_or_default(),
    )
}

/// Two routes over the same source must produce the same waveform, and
/// markers no further apart than the ingestion contract already allows.
#[cfg(all(feature = "analysis-beat", feature = "analysis-waveform"))]
pub(super) fn assert_agrees(want: &Artifacts, got: &Artifacts, what: &str) {
    assert_eq!(
        Vec::<u8>::from(&got.0),
        Vec::<u8>::from(&want.0),
        "{what}: the waveform must be identical"
    );
    assert_eq!(
        got.1.len(),
        want.1.len(),
        "{what}: the same markers must be found"
    );
    for (a, b) in want.1.iter().zip(got.1.iter()) {
        assert!(
            a.abs_diff(*b) <= MARKER_TOLERANCE,
            "{what}: marker moved from {a} to {b}"
        );
    }
}

pub(super) const SR: u32 = 44_100;
pub(super) const CH: u16 = 2;

pub(super) fn spec() -> PcmSpec {
    PcmSpec {
        channels: CH,
        sample_rate: NonZeroU32::new(SR).unwrap(),
    }
}

/// Interleaved stereo sine over the source frames `[0, frames)`.
pub(super) fn sine(frames: usize) -> Vec<f32> {
    sine_from(0, frames)
}

/// Interleaved stereo sine over the source frames `[at, at + frames)`, so the
/// same source frame carries the same sample whichever order, and from
/// whichever position, it was decoded.
pub(super) fn sine_from(at: u64, frames: usize) -> Vec<f32> {
    let inc = std::f64::consts::TAU * 440.0 / f64::from(SR);
    let mut out = Vec::with_capacity(frames * usize::from(CH));
    for index in 0..frames {
        let frame = at.saturating_add(index.to_u64().unwrap_or(0));
        let sample_f64 = 0.5 * (inc * frame.to_f64().unwrap_or(0.0)).sin();
        let sample: f32 = sample_f64.as_();
        out.push(sample);
        out.push(sample);
    }
    out
}

pub(super) fn chunk(samples: &[f32], frame_offset: u64) -> PcmChunk {
    let frames = samples.len() / usize::from(CH);
    PcmChunk::new(
        PcmMeta {
            spec: spec(),
            frames: u32::try_from(frames).unwrap_or(0),
            frame_offset,
            ..Default::default()
        },
        PcmPool::default().attach(samples.to_vec()),
    )
}

/// Scripted `PcmReader` for analysis tests: pops pre-built `next_chunk`
/// outcomes; the playback-oriented methods are unreachable on this path.
pub(super) struct FakeReader {
    bus: EventBus,
    metadata: TrackMetadata,
    outcomes: VecDeque<Result<ChunkOutcome, DecodeError>>,
}

impl FakeReader {
    pub(super) fn new(outcomes: VecDeque<Result<ChunkOutcome, DecodeError>>) -> Self {
        Self {
            outcomes,
            bus: EventBus::default(),
            metadata: TrackMetadata::default(),
        }
    }

    /// Split `samples` into `parts` chunks followed by EOF.
    pub(super) fn chunked(samples: &[f32], parts: usize) -> Self {
        let per = samples.len().div_ceil(parts.max(1)) / usize::from(CH) * usize::from(CH);
        let mut frame_offset = 0;
        let mut outcomes: VecDeque<_> = samples
            .chunks(per.max(usize::from(CH)))
            .map(|part| {
                let at = frame_offset;
                frame_offset += u64::try_from(part.len() / usize::from(CH)).unwrap_or(0);
                Ok(ChunkOutcome::Chunk(chunk(part, at)))
            })
            .collect();
        outcomes.push_back(Ok(eof()));
        Self::new(outcomes)
    }

    /// Like [`Self::chunked`] with a `Pending` tick between every chunk.
    #[cfg(feature = "analysis-waveform")]
    pub(super) fn chunked_with_pending(samples: &[f32], parts: usize) -> Self {
        let mut with_pending = VecDeque::new();
        for outcome in Self::chunked(samples, parts).outcomes {
            with_pending.push_back(Ok(pending()));
            with_pending.push_back(outcome);
        }
        Self::new(with_pending)
    }

    /// `stalls` pending ticks, then EOF: room for a producer to offer while
    /// the reader itself contributes nothing.
    #[cfg(feature = "analysis-waveform")]
    pub(super) fn stalled(stalls: usize) -> Self {
        let mut outcomes: VecDeque<_> = (0..stalls).map(|_| Ok(pending())).collect();
        outcomes.push_back(Ok(eof()));
        Self::new(outcomes)
    }

    #[cfg(feature = "analysis-waveform")]
    pub(super) fn empty() -> Self {
        Self::new(VecDeque::from([Ok(eof())]))
    }

    #[cfg(feature = "analysis-waveform")]
    pub(super) fn failing() -> Self {
        Self::new(VecDeque::from([Err(DecodeError::InvalidData {
            detail: "scripted failure",
        })]))
    }
}

/// A transport no producer writes to: the pass is fed by its reader alone.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn idle_ingest() -> crate::analysis::producer::ring::Reader {
    crate::analysis::producer::ring::open_for(spec().sample_rate).1
}

pub(super) fn eof() -> ChunkOutcome {
    ChunkOutcome::Eof {
        position: Duration::ZERO,
    }
}

#[cfg(feature = "analysis-waveform")]
pub(super) fn pending() -> ChunkOutcome {
    ChunkOutcome::Pending {
        reason: PendingReason::Buffering,
        position: Duration::ZERO,
    }
}

impl PcmSession for FakeReader {
    fn duration(&self) -> Option<Duration> {
        None
    }

    fn event_bus(&self) -> &EventBus {
        &self.bus
    }

    fn metadata(&self) -> &TrackMetadata {
        &self.metadata
    }
}

impl PcmRead for FakeReader {
    fn next_chunk(&mut self) -> Result<ChunkOutcome, DecodeError> {
        self.outcomes.pop_front().unwrap_or_else(|| Ok(eof()))
    }

    fn position(&self) -> Duration {
        Duration::ZERO
    }

    fn read(&mut self, _buf: &mut [f32]) -> Result<ReadOutcome, DecodeError> {
        unreachable!("analysis uses next_chunk")
    }

    fn read_planar<'a>(
        &mut self,
        _output: &'a mut [&'a mut [f32]],
    ) -> Result<ReadOutcome, DecodeError> {
        unreachable!("analysis uses next_chunk")
    }

    fn spec(&self) -> PcmSpec {
        spec()
    }
}

impl PcmControl for FakeReader {
    fn seek(&mut self, _position: Duration) -> Result<SeekOutcome, DecodeError> {
        unreachable!("analysis never seeks")
    }
}
