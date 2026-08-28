use std::num::NonZeroU32;

use kithara_bufpool::{PcmBuf, PcmPool};
use kithara_resampler::{MonoStream, MonoStreamConfig, ResamplerBackend, ResamplerOptions};
use num_traits::cast::ToPrimitive;
use tracing::{debug, warn};

use crate::analysis::analyzer::BeatAnalysisConfig;

/// One contiguous covered span of the source, held as detector-rate mono
/// anchored at its own source start.
///
/// The run, not the pass, owns the resampler: `MonoStream` is sequential, so a
/// range decoded later in the track cannot be pushed through the stream that
/// produced an earlier one. A run keeps its stream only while it is the
/// frontier; the moment another segment is appended behind it, the stream is
/// flushed into `mono` and dropped.
struct Run<B>
where
    B: ResamplerBackend,
{
    start: u64,
    end: u64,
    /// Detector-rate mono from `start`, drawn from the pass's pool so a run
    /// cannot grow outside the pool's byte budget.
    mono: PcmBuf,
    stream: Option<MonoStream<B>>,
}

/// The contiguous runs a beat pass has assembled, in source order.
#[derive(fieldwork::Fieldwork)]
#[fieldwork(opt_in, get)]
pub(super) struct Runs<B>
where
    B: ResamplerBackend,
{
    runs: Vec<Run<B>>,
    config: BeatAnalysisConfig<B>,
    pcm_pool: PcmPool,
    /// Detector frames the runs may hold between them. Detection consumes the
    /// front of a run and never its back, so the budget is spent from the
    /// earliest run forward.
    budget: usize,
    /// Source ranges whose detector-rate mono the budget reclaimed. They stay
    /// covered for every other consumer; beat simply cannot analyse them again.
    #[field(get, vis = "pub(super)")]
    dropped: Vec<(u64, u64)>,
    ratio: f64,
    source_rate: u32,
    #[field(get, copy, vis = "pub(super)")]
    target_rate: u32,
}

impl<B> Runs<B>
where
    B: ResamplerBackend,
{
    pub(super) fn new(
        config: BeatAnalysisConfig<B>,
        pcm_pool: PcmPool,
        source_rate: u32,
        budget: usize,
    ) -> Self {
        let target_rate = config.target_rate().max(1);
        let source = f64::from(source_rate.max(1));
        Self {
            runs: Vec::new(),
            ratio: f64::from(target_rate) / source,
            budget,
            dropped: Vec::new(),
            config,
            pcm_pool,
            source_rate: source_rate.max(1),
            target_rate,
        }
    }

    /// Detector frames the runs hold between them.
    fn held(&self) -> usize {
        self.runs.iter().map(|run| run.mono.len()).sum()
    }

    #[cfg(test)]
    pub(super) fn held_frames(&self) -> usize {
        self.held()
    }

    /// Spend the budget from the earliest run forward, which is where
    /// detection has already been.
    fn enforce_budget(&mut self) {
        let mut held = self.held();
        while held > self.budget {
            let over = held - self.budget;
            let Some(run) = self.runs.first_mut() else {
                return;
            };
            let drop_detector = over.min(run.mono.len());
            let source = self.ratio.recip() * drop_detector.to_f64().unwrap_or(0.0);
            let source = source
                .round()
                .to_u64()
                .unwrap_or(0)
                .min(run.end - run.start);
            let at = run.start;
            let exact = (source.to_f64().unwrap_or(0.0) * self.ratio)
                .round()
                .to_usize()
                .unwrap_or(0)
                .min(run.mono.len());
            if exact == 0 {
                return;
            }

            run.mono.drain(..exact);
            run.start = at.saturating_add(source);
            held -= exact;
            self.dropped.push((at, run.start));
            debug!(
                from = at,
                to = run.start,
                "beat analysis: detector mono reclaimed; range left unanalysed"
            );
            if run.start >= run.end {
                self.runs.remove(0);
            }
        }
    }

    /// Detector frames a source span of `frames` becomes, using the same
    /// rounding `MonoStream` uses for its own output budget.
    fn detector_frames(&self, frames: u64) -> usize {
        let frames: f64 = frames.to_f64().unwrap_or(0.0);
        (frames * self.ratio)
            .round()
            .to_usize()
            .unwrap_or(usize::MAX)
    }

    /// Flush every frontier stream so each run holds its whole source span.
    pub(super) fn flush(&mut self) {
        for index in 0..self.runs.len() {
            let Some((span, stream)) = self
                .runs
                .get_mut(index)
                .map(|run| (run.end.saturating_sub(run.start), run.stream.take()))
            else {
                continue;
            };
            let expected = self.detector_frames(span);
            let Some(run) = self.runs.get_mut(index) else {
                continue;
            };
            let mono = &mut run.mono;
            if let Some(stream) = stream
                && let Err(e) = stream.finish(|samples| append(mono, samples))
            {
                warn!(?e, "beat analysis: resampler flush failed");
            }
            pad(mono, expected);
        }
    }

    /// Source-rate mono, in the same order the caller wants it analysed.
    ///
    /// The block may extend a run, sit before one, bridge two, or start its
    /// own; every shape is the same walk from the leftmost touched run to the
    /// rightmost, appending the pieces of the block that fill the gaps.
    pub(super) fn push(&mut self, mono: &[f32], at: u64) {
        let Ok(span) = u64::try_from(mono.len()) else {
            return;
        };
        if span == 0 {
            return;
        }
        let end = at.saturating_add(span);

        let first = self.runs.partition_point(|run| run.end < at);
        let last = self.runs.partition_point(|run| run.start <= end);
        if first == last {
            if let Some(run) = self.open(mono, at, end) {
                self.runs.insert(first, run);
            }
            self.enforce_budget();
            return;
        }

        let absorbed: Vec<Run<B>> = self.runs.splice(first..last, []).collect();
        if let Some(merged) = self.merge(absorbed, mono, at, end) {
            self.runs.insert(first, merged);
        }
        self.enforce_budget();
    }

    /// Detector-rate mono of every run, with the source frame each begins at.
    pub(super) fn spans(&self) -> impl Iterator<Item = (u64, &[f32])> {
        self.runs.iter().map(|run| (run.start, &run.mono[..]))
    }

    /// Detector frames a source frame maps to inside a run starting at `start`.
    pub(super) fn offset_in_run(&self, start: u64, frame: u64) -> usize {
        self.detector_frames(frame.saturating_sub(start))
    }

    /// Fold `absorbed` and the block into one run, left to right. Every stream
    /// but the frontier's is flushed and every join is pinned to the detector
    /// frame its source position implies, so a rounding difference cannot
    /// accumulate into a drift of the markers downstream of it.
    fn merge(&mut self, absorbed: Vec<Run<B>>, mono: &[f32], at: u64, end: u64) -> Option<Run<B>> {
        let base = absorbed.first().map_or(at, |run| run.start.min(at));
        let mut out = self.pcm_pool.get_with(Vec::clear);
        let mut cursor = base;
        let mut stream = None;

        for run in absorbed {
            if cursor < run.start {
                let piece = slice(mono, at, cursor, run.start)?;
                self.segment(&mut out, piece)?;
                pad(
                    &mut out,
                    self.detector_frames(run.start.saturating_sub(base)),
                );
            }
            if run.end <= cursor {
                continue;
            }
            let mut run = run;
            if let Some(inner) = run.stream.take() {
                let tail = &mut run.mono;
                if let Err(e) = inner.finish(|samples| append(tail, samples)) {
                    warn!(?e, "beat analysis: resampler flush failed");
                }
            }
            let skip = self.detector_frames(cursor.saturating_sub(run.start));
            append(&mut out, run.mono.get(skip..).unwrap_or_default());
            cursor = run.end;
            pad(&mut out, self.detector_frames(cursor.saturating_sub(base)));
        }

        if cursor < end {
            let piece = slice(mono, at, cursor, end)?;
            let opened = self.open(piece, cursor, end)?;
            append(&mut out, &opened.mono);
            stream = opened.stream;
            cursor = end;
        }

        Some(Run {
            start: base,
            end: cursor,
            mono: out,
            stream,
        })
    }

    /// Start a run at `at`, keeping its stream as the frontier.
    fn open(&mut self, mono: &[f32], at: u64, end: u64) -> Option<Run<B>> {
        let mut out = self.pcm_pool.get_with(Vec::clear);
        let stream = if self.source_rate == self.target_rate {
            append(&mut out, mono);
            None
        } else {
            let mut stream = self.stream()?;
            if let Err(e) = stream.push(mono.iter().copied(), |samples| {
                append(&mut out, samples);
            }) {
                warn!(
                    ?e,
                    "beat analysis: resample block failed; range left unanalysed"
                );
                return None;
            }
            Some(stream)
        };
        Some(Run {
            start: at,
            end,
            mono: out,
            stream,
        })
    }

    /// Resample one closed segment into `out`: its stream never becomes a
    /// frontier, so it is flushed straight away and the output is exact for its
    /// source span.
    fn segment(&mut self, out: &mut PcmBuf, mono: &[f32]) -> Option<()> {
        if self.source_rate == self.target_rate {
            append(out, mono);
            return Some(());
        }
        let mut stream = self.stream()?;
        if let Err(e) = stream.push(mono.iter().copied(), |samples| append(out, samples)) {
            warn!(
                ?e,
                "beat analysis: resample block failed; range left unanalysed"
            );
            return None;
        }
        if let Err(e) = stream.finish(|samples| append(out, samples)) {
            warn!(?e, "beat analysis: resampler flush failed");
            return None;
        }
        Some(())
    }

    fn stream(&self) -> Option<MonoStream<B>> {
        let source_sample_rate = NonZeroU32::new(self.source_rate)?;
        let target_sample_rate = NonZeroU32::new(self.target_rate)?;
        let config = MonoStreamConfig::builder()
            .backend(self.config.resampler_backend().clone())
            .source_sample_rate(source_sample_rate)
            .target_sample_rate(target_sample_rate)
            .quality(self.config.resampler_quality())
            .options(
                ResamplerOptions::builder()
                    .chunk_size(self.config.block_frames())
                    .build(),
            )
            .pcm_pool(self.pcm_pool.clone())
            .build();
        MonoStream::new(config)
            .map_err(|e| {
                warn!(
                    ?e,
                    source_rate = self.source_rate,
                    "beat analysis: resampler construction failed"
                );
            })
            .ok()
    }
}

/// Truncate or zero-extend to `expected`. The correction is under one detector
/// frame per join: `MonoStream` rounds its own budget, and rounding each
/// segment separately does not always sum to rounding the whole.
fn pad(out: &mut PcmBuf, expected: usize) {
    if out.len() > expected {
        out.truncate(expected);
    } else if let Err(e) = out.ensure_len(expected) {
        warn!(
            ?e,
            expected, "beat analysis: pooled mono could not be padded"
        );
    }
}

/// Append to a pooled buffer, which grows only inside the pool's budget.
fn append(out: &mut PcmBuf, src: &[f32]) {
    let at = out.len();
    if let Err(e) = out.ensure_len(at.saturating_add(src.len())) {
        warn!(
            ?e,
            "beat analysis: pooled mono could not grow; samples dropped"
        );
        return;
    }
    if let Some(dst) = out.get_mut(at..at + src.len()) {
        dst.copy_from_slice(src);
    }
}

/// The `[from, to)` part of a block that starts at source frame `at`.
fn slice(mono: &[f32], at: u64, from: u64, to: u64) -> Option<&[f32]> {
    let start = usize::try_from(from.saturating_sub(at)).ok()?;
    let end = usize::try_from(to.saturating_sub(at)).ok()?;
    mono.get(start..end)
}

#[cfg(test)]
mod tests {
    use kithara_bufpool::PcmPool;
    use kithara_resampler::rubato::RubatoBackend;
    use kithara_test_utils::kithara;

    use super::Runs;
    use crate::analysis::BeatAnalysisConfig;

    const SRC: u32 = 44_100;

    fn runs(source_rate: u32) -> Runs<RubatoBackend> {
        budgeted(source_rate, usize::MAX)
    }

    fn budgeted(source_rate: u32, budget: usize) -> Runs<RubatoBackend> {
        Runs::new(
            BeatAnalysisConfig::<RubatoBackend>::default(),
            PcmPool::default(),
            source_rate,
            budget,
        )
    }

    fn ramp(frames: usize, from: u64) -> Vec<f32> {
        (0..frames)
            .map(|n| {
                let t = (from + n as u64) as f32 / 1000.0;
                t.sin()
            })
            .collect()
    }

    fn layout(runs: &Runs<RubatoBackend>) -> Vec<(u64, usize)> {
        runs.spans()
            .map(|(start, mono)| (start, mono.len()))
            .collect()
    }

    #[kithara::test]
    fn adjacent_blocks_form_one_run() {
        let mut set = runs(SRC);
        set.push(&ramp(4410, 0), 0);
        set.push(&ramp(4410, 4410), 4410);
        set.flush();
        assert_eq!(layout(&set), vec![(0, 4410)], "8820 source frames at 2:1");
    }

    #[kithara::test]
    fn a_gap_keeps_two_runs_until_it_is_filled() {
        let mut set = runs(SRC);
        set.push(&ramp(4410, 0), 0);
        set.push(&ramp(4410, 88_200), 88_200);
        set.flush();
        assert_eq!(layout(&set), vec![(0, 2205), (88_200, 2205)]);

        set.push(&ramp(83_790, 4410), 4410);
        set.flush();
        assert_eq!(
            layout(&set),
            vec![(0, 46_305)],
            "filling the gap joins the runs and pins the total length"
        );
    }

    #[kithara::test]
    fn a_block_before_a_run_extends_it_backwards() {
        let mut set = runs(SRC);
        set.push(&ramp(4410, 4410), 4410);
        set.push(&ramp(4410, 0), 0);
        set.flush();
        assert_eq!(layout(&set), vec![(0, 4410)]);
    }

    #[kithara::test]
    fn shuffled_blocks_land_at_the_same_detector_offsets() {
        let blocks: Vec<(u64, Vec<f32>)> = (0..8u64)
            .map(|i| (i * 4410, ramp(4410, i * 4410)))
            .collect();

        let mut ascending = runs(SRC);
        for (at, pcm) in &blocks {
            ascending.push(pcm, *at);
        }
        ascending.flush();

        let mut shuffled = runs(SRC);
        for index in [5usize, 0, 7, 2, 1, 6, 3, 4] {
            let Some((at, pcm)) = blocks.get(index) else {
                continue;
            };
            shuffled.push(pcm, *at);
        }
        shuffled.flush();

        assert_eq!(layout(&ascending), layout(&shuffled));
        let (_, want) = ascending.spans().next().expect("one run");
        let (_, got) = shuffled.spans().next().expect("one run");
        let drift = want
            .iter()
            .zip(got.iter())
            .filter(|(a, b)| (*a - *b).abs() > 1e-3)
            .count();
        assert!(
            drift * 200 < want.len(),
            "shuffled assembly must track the ascending one, {drift} of {} samples differ",
            want.len()
        );
    }

    #[kithara::test]
    fn the_budget_reclaims_the_earliest_mono_and_reports_it() {
        let mut set = budgeted(SRC, 20_000);
        for block in 0..10u64 {
            set.push(&ramp(4410, block * 4410), block * 4410);
        }
        assert!(
            set.held_frames() <= 20_000,
            "held detector frames must stay under the budget, got {}",
            set.held_frames()
        );

        let dropped = set.dropped();
        assert!(!dropped.is_empty(), "the reclaimed ranges must be reported");
        let (from, _) = dropped.first().copied().unwrap_or((1, 1));
        assert_eq!(from, 0, "the earliest source frames go first");
        let reclaimed: u64 = dropped.iter().map(|(from, to)| to - from).sum();
        assert!(
            reclaimed > 0 && reclaimed < 44_100,
            "the budget reclaims the overflow, not the track: {reclaimed}"
        );
    }

    #[kithara::test]
    fn a_non_integer_ratio_keeps_joins_on_position() {
        // 48 kHz -> 22.05 kHz is not a whole ratio, so a per-segment rounding
        // error would show up as a length drift at every join.
        let mut set = runs(48_000);
        for block in 0..10u64 {
            set.push(&ramp(4801, block * 4801), block * 4801);
        }
        set.flush();
        let total = set.offset_in_run(0, 48_010);
        assert_eq!(layout(&set), vec![(0, total)]);
    }
}
