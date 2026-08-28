use std::num::NonZeroU32;

use kithara_decode::PcmChunk;
use kithara_resampler::ResamplerBackend;
use num_traits::cast::ToPrimitive;
use tracing::warn;

use super::{
    snapshot::{BeatSnapshot, GridState},
    track::{AnalysisFingerprint, AnalysisToken, TrackAnalysis},
};
use crate::{
    analysis::slots::{
        beat::{self, Slot},
        waveform,
    },
    coverage::{Coverage, FrameRange},
};

/// What a pass did with an offered range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ingest {
    /// Folded into the artifacts and added to the coverage.
    Accepted,
    /// Already covered: the artifacts and the coverage are unchanged.
    Covered,
    /// Measured on another sample-rate axis than the pass was opened with.
    ForeignRate,
    /// Reaches past the source extent the pass knows.
    OutOfExtent,
}

#[derive(fieldwork::Fieldwork)]
#[fieldwork(opt_in, get)]
pub(crate) struct TrackAnalyzers<B>
where
    B: ResamplerBackend,
{
    pub(super) beat: Slot<B>,
    pub(super) waveform: waveform::Slot,
    /// Source frame ranges this pass has observed, in `source_sample_rate`.
    pub(super) coverage: Coverage,
    pub(super) fingerprint: AnalysisFingerprint,
    /// Source length in frames. Unknown until the pass is told, and pinned to
    /// the covered frontier at end of stream, which is the decoder's own
    /// ground truth rather than a second estimate of it.
    pub(super) extent: Option<u64>,
    pub(super) revision: u64,
    /// Sample-rate axis frozen from the first decoded chunk of this pass.
    #[field(get, copy, vis = "pub(crate)")]
    pub(super) source_sample_rate: NonZeroU32,
    pub(super) token: AnalysisToken,
}

impl<B> TrackAnalyzers<B>
where
    B: ResamplerBackend,
{
    /// What this pass has observed, which is what a decode schedule plans
    /// against. Read by the worker, which the wasm build does not have.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) const fn coverage(&self) -> &Coverage {
        &self.coverage
    }

    /// The source length the caller planned against, which the final snapshot
    /// measures against instead of the covered frontier. A pass that gave up
    /// on a range past its last covered frame must still report it missing.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn plan_extent(&mut self, frames: u64) {
        self.extent = Some(self.extent.map_or(frames, |held| held.max(frames)));
    }

    /// Covered source frames, counting an overlap or a repeat once.
    pub(crate) fn covered_frames(&self) -> u64 {
        self.coverage.frames()
    }

    /// Fold one interleaved chunk, positioned by its own metadata.
    pub(crate) fn push(
        &mut self,
        chunk: &PcmChunk,
        detector: Option<&mut beat::Detector>,
    ) -> Ingest {
        let rate = chunk.spec().sample_rate;
        if rate != self.source_sample_rate {
            warn!(
                axis = self.source_sample_rate.get(),
                rate = rate.get(),
                "analysis: chunk rate differs from the pass axis; range dropped"
            );
            return Ingest::ForeignRate;
        }

        let channels = usize::from(chunk.spec().channels.max(1));
        let range = FrameRange::from(&chunk.meta);
        self.ingest(&chunk.samples[..], channels, range, detector)
    }

    /// Fold one range that a producer already downmixed.
    ///
    /// The transport carries mono measured on the pass's own axis, so there is
    /// no spec to check here: the producer refused anything else before it was
    /// written.
    pub(crate) fn push_mono(
        &mut self,
        mono: &[f32],
        at: u64,
        detector: Option<&mut beat::Detector>,
    ) -> Ingest {
        let frames = mono.len().to_u64().unwrap_or(0);
        self.ingest(mono, 1, FrameRange::new(at, frames), detector)
    }

    fn ingest(
        &mut self,
        pcm: &[f32],
        channels: usize,
        range: FrameRange,
        detector: Option<&mut beat::Detector>,
    ) -> Ingest {
        if self.extent.is_some_and(|extent| range.end() > extent) {
            warn!(
                start = range.start(),
                end = range.end(),
                extent = self.extent,
                "analysis: range lies beyond the source extent; dropped"
            );
            return Ingest::OutOfExtent;
        }
        if self.coverage.contains(range) {
            return Ingest::Covered;
        }
        self.coverage.insert(range);

        waveform::push(&mut self.waveform, pcm, channels, range.start());
        Slot::push(&mut self.beat, pcm, channels, range.start(), detector);
        Ingest::Accepted
    }

    /// Publish what the pass holds without ending it. `ending` marks end of
    /// stream: the extent is pinned to the covered frontier and every run's
    /// trailing detector window is evaluated.
    pub(crate) fn snapshot(
        &mut self,
        detector: Option<&mut beat::Detector>,
        ending: bool,
    ) -> TrackAnalysis {
        if ending {
            // The frontier is the extent only for a pass that grew from the
            // start; one that planned against a longer source keeps that
            // length, so the tail it never reached stays missing.
            let frontier = self.coverage.frontier();
            self.extent = Some(
                self.extent
                    .map_or(frontier, |planned| planned.max(frontier)),
            );
        }
        self.revision = self.revision.saturating_add(1);

        let waveform = waveform::snapshot(&mut self.waveform, self.extent);
        let state = self.grid_state();
        let beat = Slot::snapshot(&mut self.beat, detector, ending, self.extent)
            .map(|(grid, unanalysed)| BeatSnapshot::new(grid, state, unanalysed));

        TrackAnalysis::builder()
            .token(self.token.clone())
            .revision(self.revision)
            .source_sample_rate(self.source_sample_rate)
            .maybe_extent(self.extent)
            .coverage(self.coverage.clone())
            .fingerprint(self.fingerprint.clone())
            .maybe_waveform(waveform)
            .maybe_beat(beat)
            .build()
    }

    /// A grid is final only once the whole known extent is one covered run.
    fn grid_state(&self) -> GridState {
        let covered = self
            .extent
            .is_some_and(|extent| self.coverage.contains(FrameRange::new(0, extent)));
        if covered {
            GridState::Final
        } else {
            GridState::Provisional
        }
    }
}
