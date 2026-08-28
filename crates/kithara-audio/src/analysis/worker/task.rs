use std::num::NonZeroU32;

use kithara_bufpool::PcmBuf;
use kithara_decode::duration_for_frames;
use kithara_platform::{CancelToken, tokio::sync::watch};
use kithara_resampler::ResamplerBackend;
use tracing::{debug, warn};

use super::schedule::{Extent, Schedule};
use crate::{
    ChunkOutcome, PcmReader, SeekOutcome,
    analysis::{
        analyzer::{
            AnalysisToken, AnalyzerBuilder, Detector, Ingest, TrackAnalysis, TrackAnalyzers,
        },
        producer::ring,
    },
    coverage::{Coverage, FrameRange},
    runtime::TickResult,
};

/// Source seconds between publications while decoding. Keyed to decoded
/// frames rather than wall-clock time so a run publishes the same revision
/// sequence every time.
const PUBLISH_SECONDS: u64 = 5;

pub(crate) struct Job {
    pub(crate) reader: Box<dyn PcmReader>,
    pub(crate) cancel: CancelToken,
    /// The consumer half of this pass's producer transport.
    pub(crate) ingest: ring::Reader,
    /// The axis every range of this pass is measured on.
    pub(crate) rate: NonZeroU32,
    pub(crate) token: AnalysisToken,
    pub(crate) tx: watch::Sender<Option<TrackAnalysis>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskPhase {
    Decode,
    Ending,
    Done,
}

/// One decoded run: the position it was scheduled to, where its first chunk
/// came from, how far it has decoded, and whether any of that was new.
///
/// `at` is seeded with the scheduled position and replaced by the first chunk
/// the run decodes: a seek reports where it was asked to go, not where the
/// decoder resumed, so only a chunk says where the run really starts.
struct Run {
    chosen: u64,
    at: u64,
    frontier: u64,
    started: bool,
    grew: bool,
}

pub(crate) struct AnalysisTask<B>
where
    B: ResamplerBackend,
{
    reader: Box<dyn PcmReader>,
    cancel: CancelToken,
    analyzers: Option<TrackAnalyzers<B>>,
    ingest: ring::Reader,
    /// Where a drained range is read into. Held across ticks so draining
    /// costs no allocation.
    scratch: Option<PcmBuf>,
    rate: NonZeroU32,
    token: AnalysisToken,
    tx: watch::Sender<Option<TrackAnalysis>>,
    phase: TaskPhase,
    /// Covered frames at the last publication.
    published_at: u64,
    /// Source length the schedule plans against, on the pass's axis. Read
    /// from the reader rather than from the pass, which pins its own extent
    /// only at end of stream. Until the source names one, the reader is left
    /// where it is and decoded in order.
    extent: Extent,
    schedule: Schedule,
    run: Option<Run>,
}

impl<B> AnalysisTask<B>
where
    B: ResamplerBackend,
{
    pub(crate) fn new(job: Job) -> Self {
        Self {
            analyzers: None,
            cancel: job.cancel,
            extent: Extent::default(),
            ingest: job.ingest,
            phase: TaskPhase::Decode,
            published_at: 0,
            rate: job.rate,
            reader: job.reader,
            run: None,
            schedule: Schedule::default(),
            scratch: None,
            token: job.token,
            tx: job.tx,
        }
    }

    /// Whether the pass already holds `range` in one covered run.
    fn is_covered(&self, range: FrameRange) -> bool {
        self.analyzers
            .as_ref()
            .is_some_and(|analyzers| analyzers.coverage().contains(range))
    }

    /// Whether the whole known extent sits in one covered run. Asked before
    /// the run in flight is, because a run waiting on its reader is never the
    /// one that notices a producer covered the last of the track.
    fn is_complete(&self) -> bool {
        self.extent
            .frames()
            .is_some_and(|extent| self.is_covered(FrameRange::new(0, extent)))
    }

    /// The position to decode from next, chosen from what no producer covered.
    fn choose(&self) -> Option<u64> {
        let empty = Coverage::default();
        let coverage = self
            .analyzers
            .as_ref()
            .map_or(&empty, TrackAnalyzers::coverage);
        self.schedule.next(coverage, self.extent.frames())
    }

    fn decode(
        &mut self,
        builder: &AnalyzerBuilder<B>,
        detector: Option<&mut Detector>,
    ) -> TickResult {
        match self.reader.next_chunk() {
            Ok(ChunkOutcome::Chunk(chunk)) => {
                let range = FrameRange::from(&chunk.meta);
                let analyzers = open(&mut self.analyzers, builder, self.rate, &self.token);
                let before = analyzers.covered_frames();
                let outcome = analyzers.push(&chunk, detector);
                if outcome != Ingest::Accepted {
                    debug!(?outcome, "analysis: range not folded in");
                }
                let grew = analyzers.covered_frames() > before;
                if let Some(run) = &mut self.run {
                    if !run.started {
                        run.started = true;
                        run.at = range.start();
                    }
                    run.frontier = range.end();
                    run.grew |= grew;
                }
                TickResult::Progress
            }
            Ok(ChunkOutcome::Pending { .. }) => TickResult::UpstreamPending,
            Ok(ChunkOutcome::Eof { .. }) => {
                // End of stream ends the run, and bounds the extent: the
                // source has just proved where it ends, whatever length it
                // reports. It ends the pass only when there is nothing to
                // schedule in its place - a run that reached the end of the
                // source may still have gaps behind it, and a pass with no
                // extent has no schedule at all.
                if self.extent.frames().is_none() {
                    self.finish();
                } else {
                    if let Some(run) = &self.run {
                        self.extent.unreachable(run.frontier);
                    }
                    self.retire();
                }
                TickResult::Progress
            }
            Err(error) => {
                // What was covered is still worth publishing: the reader
                // failed, the ranges it already delivered did not.
                warn!(?error, "analysis: decode error; pass ended");
                self.finish();
                TickResult::Progress
            }
        }
    }

    /// Fold everything a producer left in the transport, one range per
    /// descriptor.
    ///
    /// Contiguous ranges are deliberately not joined into one block: the beat
    /// pass segments its resampler at every block boundary, so joining them
    /// would make the artifacts depend on how many ranges happened to be
    /// waiting. Returns whether anything was folded.
    fn drain(&mut self, builder: &AnalyzerBuilder<B>, detector: Option<&mut Detector>) -> bool {
        let scratch = self
            .scratch
            .get_or_insert_with(|| builder.pcm_pool().get_with(Vec::clear));
        let analyzers = open(&mut self.analyzers, builder, self.rate, &self.token);
        let mut detector = detector;
        let mut folded = false;

        while let Some(at) = self.ingest.pop(scratch) {
            let outcome = analyzers.push_mono(scratch, at, detector.as_deref_mut());
            if outcome != Ingest::Accepted {
                debug!(?outcome, at, "analysis: offered range not folded in");
            }
            folded = true;
        }
        folded
    }

    /// Whether enough new source has been covered to be worth publishing.
    fn due(&self) -> bool {
        let Some(analyzers) = &self.analyzers else {
            return false;
        };
        let interval =
            u64::from(analyzers.source_sample_rate().get()).saturating_mul(PUBLISH_SECONDS);
        analyzers.covered_frames().saturating_sub(self.published_at) >= interval
    }

    /// End the pass, handing the length it planned against to the snapshot so
    /// a range it gave up on is still reported missing. Safe here and not
    /// earlier: nothing is ingested once the phase leaves `Decode`, so an
    /// extent the source under-reports cannot refuse its own tail.
    fn finish(&mut self) {
        let planned = self.extent.frames();
        let Some(analyzers) = &mut self.analyzers else {
            self.phase = TaskPhase::Done;
            return;
        };
        if let Some(frames) = planned {
            analyzers.plan_extent(frames);
        }
        self.phase = TaskPhase::Ending;
    }

    pub(crate) fn is_done(&self) -> bool {
        self.phase == TaskPhase::Done
    }

    /// Publish what the pass holds. `ending` marks end of stream, which pins
    /// the extent and evaluates every run's trailing detector window.
    ///
    /// A pass that covered nothing publishes nothing, the same way a stream
    /// that decoded nothing is not an analysis. It is reachable when every
    /// range the reader produced was measured on another axis.
    fn publish(&mut self, detector: Option<&mut Detector>, ending: bool) {
        let Some(analyzers) = &mut self.analyzers else {
            return;
        };
        if analyzers.covered_frames() == 0 {
            return;
        }
        let snapshot = analyzers.snapshot(detector, ending);
        self.published_at = analyzers.covered_frames();
        self.tx.send(Some(snapshot)).ok();
    }

    /// Seek to the next scheduled position and open a run there, or end the
    /// pass when there is nothing left to schedule: the extent is covered, or
    /// what is left of it has already proved unreachable.
    fn reschedule(&mut self) -> TickResult {
        self.retire();
        let Some(at) = self.choose() else {
            self.finish();
            return TickResult::Progress;
        };

        match self.reader.seek(duration_for_frames(self.rate.get(), at)) {
            // Where the seek says it landed is only an echo of the target on
            // the readers this runs against, so the run opens on the position
            // it asked for and takes its real start from its first chunk.
            Ok(SeekOutcome::Landed { .. }) => {
                self.run = Some(Run {
                    chosen: at,
                    at,
                    frontier: at,
                    started: false,
                    grew: false,
                });
            }
            // The source cannot deliver the position the schedule planned
            // against, which bounds where it ends however long it says it is.
            Ok(SeekOutcome::PastEof { duration, .. }) => {
                debug!(at, ?duration, "analysis: scheduled position past the end");
                self.extent.unreachable(at);
            }
            Err(error) => {
                warn!(?error, at, "analysis: seek failed; position retired");
                self.schedule.barren(at);
            }
        }
        TickResult::Progress
    }

    /// Close the current run, retiring the position it was scheduled to when
    /// it decoded nothing the pass did not already hold.
    ///
    /// Whether the run itself grew the coverage, not whether the coverage
    /// grew: a producer folds ranges from anywhere in the track on every
    /// tick, and counting those would keep an unreachable position eligible.
    fn retire(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        if !run.grew {
            debug!(at = run.chosen, "analysis: position added nothing; retired");
            self.schedule.barren(run.chosen);
        }
    }

    /// Whether the current run has done its work: nothing open, the next
    /// frame already covered, the extent reached, or a whole detector window
    /// decoded.
    fn run_over(&self, run_frames: Option<u64>) -> bool {
        let Some(run) = &self.run else {
            return true;
        };
        if self
            .extent
            .frames()
            .is_some_and(|extent| run.frontier >= extent)
        {
            return true;
        }
        // Only once it has decoded something: a run that has not yet has not
        // run into anything either. A seek lands on a frame boundary of its
        // own choosing, so the position it parks at can be just inside
        // covered audio while the range it was scheduled for is still ahead
        // of it, and abandoning it there would retire that range unread.
        if run.frontier > run.at && self.is_covered(FrameRange::new(run.frontier, 1)) {
            return true;
        }
        run_frames.is_some_and(|window| run.frontier.saturating_sub(run.at) >= window)
    }

    /// One decode step: linear while the source reports no length, and
    /// scheduled once it does.
    fn step(
        &mut self,
        builder: &AnalyzerBuilder<B>,
        detector: Option<&mut Detector>,
    ) -> TickResult {
        if self.extent.frames().is_none() {
            return self.decode(builder, detector);
        }
        if self.is_complete() {
            self.finish();
            return TickResult::Progress;
        }
        if self.run_over(builder.run_frames(self.rate)) {
            return self.reschedule();
        }
        self.decode(builder, detector)
    }

    pub(crate) fn tick(
        &mut self,
        builder: &AnalyzerBuilder<B>,
        detector: Option<&mut Detector>,
    ) -> TickResult {
        if self.cancel.is_cancelled() {
            debug!("analysis cancelled");
            self.phase = TaskPhase::Done;
            return TickResult::Progress;
        }

        match self.phase {
            TaskPhase::Decode => {
                let mut detector = detector;
                let drained = self.drain(builder, detector.as_deref_mut());
                // Re-read rather than cached: the decode path refines a
                // duration upward as it learns more, and a subdivision
                // computed against a short extent leaves the tail unplanned.
                self.extent.report(self.reader.duration(), self.rate);
                let result = self.step(builder, detector.as_deref_mut());
                if self.phase == TaskPhase::Decode && self.due() {
                    self.publish(detector, false);
                }
                if drained {
                    TickResult::Progress
                } else {
                    result
                }
            }
            TaskPhase::Ending => {
                self.publish(detector, true);
                self.phase = TaskPhase::Done;
                TickResult::Progress
            }
            TaskPhase::Done => TickResult::Done,
        }
    }
}

/// The pass, opened on its axis by whichever range reaches it first.
fn open<'a, B>(
    slot: &'a mut Option<TrackAnalyzers<B>>,
    builder: &AnalyzerBuilder<B>,
    rate: NonZeroU32,
    token: &AnalysisToken,
) -> &'a mut TrackAnalyzers<B>
where
    B: ResamplerBackend,
{
    slot.get_or_insert_with(|| builder.build(rate, token.clone()))
}
