use thiserror::Error;

#[cfg(feature = "beat-nn")]
#[path = "backend.rs"]
pub(super) mod backend;

/// One detected beat or downbeat: where it is, and how sure the detector was.
///
/// Declared here rather than taken from a backend crate, because the trait
/// below is what a backend answers to and no backend owns the shape of the
/// answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BeatMark {
    /// Seconds from the start of the analysed window.
    pub(crate) at: f32,
    /// Probability the detector assigned this mark, in `(0, 1)`.
    pub(crate) confidence: f32,
}

#[cfg(test)]
impl BeatMark {
    /// A mark at `at`, detected surely enough to keep, for the tests that
    /// only care where it sits.
    pub(crate) const fn at(at: f32) -> Self {
        Self {
            at,
            confidence: 0.9,
        }
    }
}

/// Raw detector output: beat / downbeat marks in seconds from track start.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBeats {
    pub(crate) beats: Vec<BeatMark>,
    pub(crate) downbeats: Vec<BeatMark>,
}

/// Failure of a beat detector backend.
#[derive(Debug, Error)]
pub(crate) enum BeatDetectError {
    #[error("beat analysis buffer budget exhausted")]
    Buffer,
    /// Only the `beat-nn` factory constructs this; gated with it.
    #[cfg(feature = "beat-nn")]
    #[error("beat detector init failed: {reason}")]
    Init { reason: String },
    /// Detection can only fail when a detector backend runs (`beat-nn`) or a
    /// test scripts a failure; without either it is unconstructable.
    #[cfg(any(test, feature = "beat-nn"))]
    #[error("beat detection failed: {reason}")]
    Detect { reason: String },
}

/// Swappable beat/downbeat detector over one mono analysis window.
#[cfg_attr(test, kithara_test_macros::mock(api = [BeatDetectorMock]))]
pub(crate) trait BeatDetector: Send {
    /// # Errors
    /// [`BeatDetectError::Detect`] when the backend fails on this input.
    fn detect(&mut self, mono_window: &[f32]) -> Result<RawBeats, BeatDetectError>;
}
