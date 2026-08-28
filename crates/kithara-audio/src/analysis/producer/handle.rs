use std::num::NonZeroU32;

use kithara_decode::PcmSpec;
use num_traits::cast::ToPrimitive;
use tracing::debug;

use super::ring::Writer;
use crate::analysis::analyzer::AnalysisToken;

/// What a pass did with an offered range, as far as its producer can tell.
/// Whether the range was already covered is the pass's business and is not
/// reported here: the producer must not wait to find out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Offer {
    /// Copied into the pass's transport. It joins the coverage when the
    /// analysis worker drains it.
    Taken,
    /// The transport is full. The range stays uncovered and so stays missing,
    /// which is what makes it eligible to be produced again.
    Full,
    /// Measured on another sample-rate axis than the pass was opened with.
    ForeignRate,
    /// The pass ended and no longer holds the transport. Nothing was written,
    /// and nothing written from here on could be read: the caller should let
    /// the handle go.
    Closed,
}

/// The producer side of one analysis pass.
///
/// The pass is named once, when the handle is made, so offering costs no
/// lookup and two producers never contend for one transport. Holding a handle
/// is the only way in; a track with no open pass has none, which is what makes
/// that case free rather than merely cheap.
pub struct AnalysisProducer {
    ring: Writer,
    /// The axis the pass was opened on. Every range offered here is measured
    /// in it.
    rate: NonZeroU32,
    token: AnalysisToken,
}

impl AnalysisProducer {
    pub(crate) const fn new(ring: Writer, rate: NonZeroU32, token: AnalysisToken) -> Self {
        Self { ring, rate, token }
    }

    /// The pass this handle feeds.
    #[must_use]
    pub const fn token(&self) -> &AnalysisToken {
        &self.token
    }

    /// Offer one interleaved range starting at source frame `at`.
    ///
    /// Downmixes to mono by the channel mean - the same reduction both
    /// analyzers apply - and copies it into the pass's transport. Returns
    /// without blocking, without allocating, and without retaining `pcm`, so
    /// the caller may recycle its buffer the moment this returns.
    pub fn offer(&mut self, pcm: &[f32], spec: PcmSpec, at: u64) -> Offer {
        if !self.ring.is_open() {
            return Offer::Closed;
        }
        if spec.sample_rate != self.rate {
            debug!(
                token = self.token.as_str(),
                axis = self.rate.get(),
                rate = spec.sample_rate.get(),
                "analysis ingest: range measured on another axis; refused"
            );
            return Offer::ForeignRate;
        }

        let channels = usize::from(spec.channels.max(1));
        let frames = pcm.len() / channels;
        let inv = 1.0 / channels.to_f32().unwrap_or(1.0);
        let mono = pcm
            .chunks_exact(channels)
            .map(move |frame| frame.iter().sum::<f32>() * inv);

        if self.ring.push(at, frames, mono) {
            Offer::Taken
        } else {
            Offer::Full
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use kithara_bufpool::PcmPool;
    use kithara_decode::PcmSpec;
    use kithara_test_utils::kithara;

    use super::{AnalysisProducer, Offer};
    use crate::analysis::producer::ring;

    fn spec(rate: u32, channels: u16) -> PcmSpec {
        PcmSpec {
            channels,
            sample_rate: NonZeroU32::new(rate).expect("test rate is non-zero"),
        }
    }

    fn producer(frames: usize, ranges: usize) -> (AnalysisProducer, ring::Reader) {
        let (tx, rx) = ring::open(frames, ranges);
        let rate = NonZeroU32::new(44_100).expect("test rate is non-zero");
        (AnalysisProducer::new(tx, rate, "track-a".into()), rx)
    }

    #[kithara::test]
    fn the_mono_written_is_the_channel_mean() {
        let (mut producer, mut reader) = producer(64, 4);
        let pool = PcmPool::default();
        let mut out = pool.get_with(Vec::clear);

        // Interleaved stereo: L, R, L, R.
        let pcm = [1.0_f32, 3.0, -2.0, 0.0];
        assert_eq!(
            producer.offer(&pcm, spec(44_100, 2), 512),
            Offer::Taken,
            "the pass axis matches"
        );

        assert_eq!(reader.pop(&mut out), Some(512));
        assert_eq!(
            &out[..],
            &[2.0, -1.0],
            "each frame is the mean of its channels"
        );
    }

    #[kithara::test]
    fn a_foreign_axis_is_refused_without_writing() {
        let (mut producer, mut reader) = producer(64, 4);
        let pool = PcmPool::default();
        let mut out = pool.get_with(Vec::clear);

        assert_eq!(
            producer.offer(&[1.0, 1.0], spec(48_000, 2), 0),
            Offer::ForeignRate
        );
        assert_eq!(reader.pop(&mut out), None, "nothing reached the transport");
    }

    #[kithara::test]
    fn a_full_transport_reports_the_range_untaken() {
        let (mut producer, _reader) = producer(2, 4);

        assert_eq!(
            producer.offer(&[1.0, 1.0], spec(44_100, 2), 0),
            Offer::Taken
        );
        assert_eq!(
            producer.offer(&[1.0, 1.0, 1.0, 1.0], spec(44_100, 2), 1),
            Offer::Full,
            "a range that does not fit is refused whole"
        );
    }

    #[kithara::test]
    fn an_offer_takes_nothing_from_the_pool_and_keeps_nothing() {
        let (mut producer, mut reader) = producer(64, 4);
        // A pool of this test's own: the process-wide default is shared, so
        // its counters move under the test and would measure nothing.
        let pool = PcmPool::new(8, 1024);

        // A pooled buffer standing in for the caller's decoded chunk.
        let mut chunk = pool.get_with(Vec::clear);
        chunk.ensure_len(16).expect("the test pool grows to 16");
        let before = pool.stats();

        assert_eq!(producer.offer(&chunk, spec(44_100, 2), 0), Offer::Taken);

        let after = pool.stats();
        assert_eq!(
            after.alloc_misses, before.alloc_misses,
            "the offer allocates nothing of its own"
        );
        assert_eq!(
            after.allocated_bytes, before.allocated_bytes,
            "and takes no buffer from the caller's pool"
        );

        // The caller is free to recycle the moment the call returns.
        drop(chunk);
        let mut out = pool.get_with(Vec::clear);
        assert_eq!(
            reader.pop(&mut out),
            Some(0),
            "the range survives its source buffer going back to the pool"
        );
        assert_eq!(
            out.len(),
            8,
            "two channels of sixteen samples is eight frames"
        );
    }

    #[kithara::test]
    fn a_pass_that_ended_refuses_as_closed() {
        let (mut producer, reader) = producer(64, 4);
        assert_eq!(
            producer.offer(&[1.0, 1.0], spec(44_100, 2), 0),
            Offer::Taken
        );

        drop(reader);
        assert_eq!(
            producer.offer(&[1.0, 1.0], spec(44_100, 2), 1),
            Offer::Closed,
            "a pass that dropped its half cannot be written to"
        );
    }

    #[kithara::test]
    fn a_mono_source_passes_through_unchanged() {
        let (mut producer, mut reader) = producer(64, 4);
        let pool = PcmPool::default();
        let mut out = pool.get_with(Vec::clear);

        let pcm = [0.25_f32, -0.5, 0.75];
        assert_eq!(producer.offer(&pcm, spec(44_100, 1), 0), Offer::Taken);

        assert_eq!(reader.pop(&mut out), Some(0));
        assert_eq!(
            &out[..],
            &pcm[..],
            "the mean of one channel is the channel itself"
        );
    }
}
