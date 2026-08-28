use kithara_resampler::ResamplerBackend;
use tracing::warn;

use super::{
    analyzer::{BeatAnalyzer, BeatPassConfig},
    detector::BeatDetector,
    grid::extend_over,
};
use crate::{coverage::FrameRange, waveform::BeatGrid};

pub(crate) struct BeatPass<B>
where
    B: ResamplerBackend,
{
    analyzer: BeatAnalyzer<B>,
}

impl<B> BeatPass<B>
where
    B: ResamplerBackend,
{
    pub(crate) fn new(config: BeatPassConfig<B>) -> Self {
        Self {
            analyzer: BeatAnalyzer::new(config),
        }
    }

    pub(crate) fn push(
        &mut self,
        pcm: &[f32],
        channels: usize,
        at: u64,
        detector: &mut dyn BeatDetector,
    ) {
        self.analyzer.push_interleaved(pcm, channels, at, detector);
    }

    /// The grid as it stands, with the ranges the pass could not analyse.
    ///
    /// Once the extent is known the grid is spread across it at its own tempo,
    /// so a track is usable from its first covered piece rather than from the
    /// end of its decode.
    pub(crate) fn snapshot(
        &mut self,
        detector: &mut dyn BeatDetector,
        ending: bool,
        extent: Option<u64>,
    ) -> Option<(BeatGrid, Vec<FrameRange>)> {
        match self.analyzer.snapshot(detector, ending) {
            Ok(grid) => {
                let rate = self.analyzer.source_rate();
                let grid = match extent {
                    Some(extent) => extend_over(grid, extent, rate),
                    None => grid,
                };
                Some((grid, self.analyzer.unanalysed()))
            }
            Err(e) => {
                warn!(?e, "beat analysis failed; leaving the beat slot empty");
                None
            }
        }
    }
}
