use std::{collections::BTreeSet, num::NonZeroU32};

use kithara_decode::frames_for_duration;
use kithara_platform::time::Duration;

use crate::coverage::{Coverage, FrameRange};

/// Where an analysis pass decodes next, chosen from what it has not covered.
///
/// One rule: the middle of the largest uncovered range. On a track nothing has
/// covered that is binary subdivision; on a mostly covered one it is hole
/// refill. Nothing has to detect which of the two it is in.
#[derive(Default)]
pub(crate) struct Schedule {
    /// Positions that produced no new coverage. A seek that snapped backwards
    /// into covered audio, or landed past the end, leaves the gap it was meant
    /// to fill exactly as it was, so without this the same position would be
    /// chosen forever.
    barren: BTreeSet<u64>,
}

impl Schedule {
    /// The next source position to decode from, or `None` when nothing is left
    /// to schedule: no extent to plan against, an extent already covered, or
    /// every remaining gap already proved unreachable.
    pub(crate) fn next(&self, coverage: &Coverage, extent: Option<u64>) -> Option<u64> {
        let mut widest: Option<FrameRange> = None;
        for gap in coverage.gaps(extent?) {
            if self.barren.contains(&middle(gap)) {
                continue;
            }
            if widest.is_none_or(|held| gap.frames() > held.frames()) {
                widest = Some(gap);
            }
        }
        widest.map(middle)
    }

    /// Record a position that added no coverage, so it is never chosen again.
    pub(crate) fn barren(&mut self, at: u64) {
        self.barren.insert(at);
    }
}

/// The source length an analysis pass plans against.
///
/// Two facts from the same reader: the length it reports, which the decode
/// path refines upward as it learns more, and the length it proved it can
/// reach, which end of stream and a seek past the end refine downward. The
/// schedule plans against the lower of the two, so a duration that is an
/// estimate cannot leave a tail no seek can reach.
#[derive(Default)]
pub(crate) struct Extent {
    reported: Option<u64>,
    reachable: Option<u64>,
}

impl Extent {
    /// What the schedule plans against, once the source names a length.
    pub(crate) fn frames(&self) -> Option<u64> {
        let reported = self.reported?;
        Some(self.reachable.map_or(reported, |limit| reported.min(limit)))
    }

    /// Take the length the source reports, keeping the largest seen.
    pub(crate) fn report(&mut self, duration: Option<Duration>, rate: NonZeroU32) {
        self.reported = self.reported.max(extent_frames(duration, rate));
    }

    /// Record that the source has nothing at or past `frame`.
    pub(crate) fn unreachable(&mut self, frame: u64) {
        self.reachable = Some(self.reachable.map_or(frame, |limit| limit.min(frame)));
    }
}

/// The middle of a range, which is where a subdivision splits it.
fn middle(range: FrameRange) -> u64 {
    range.start().saturating_add(range.frames() / 2)
}

/// What the source says its length is, in frames on the pass's axis. A source
/// that reports no duration, or a zero one, gives nothing to subdivide.
fn extent_frames(duration: Option<Duration>, rate: NonZeroU32) -> Option<u64> {
    let frames = frames_for_duration(rate.get(), duration?);
    u64::try_from(frames).ok().filter(|frames| *frames > 0)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use kithara_platform::time::Duration;
    use kithara_test_utils::kithara;

    use super::{Extent, Schedule};
    use crate::coverage::{Coverage, FrameRange};

    const EXTENT: u64 = 1000;

    fn coverage(runs: &[(u64, u64)]) -> Coverage {
        let mut out = Coverage::default();
        for (start, frames) in runs {
            out.insert(FrameRange::new(*start, *frames));
        }
        out
    }

    #[kithara::test]
    fn an_uncovered_track_is_split_in_the_middle() {
        let schedule = Schedule::default();
        assert_eq!(
            schedule.next(&Coverage::default(), Some(EXTENT)),
            Some(500),
            "the largest uncovered range of an untouched track is the track"
        );
    }

    #[kithara::test]
    fn a_hole_is_taken_from_its_middle() {
        let schedule = Schedule::default();
        assert_eq!(
            schedule.next(&coverage(&[(0, 200), (400, 600)]), Some(EXTENT)),
            Some(300)
        );
    }

    #[kithara::test]
    fn the_wider_of_two_holes_goes_first() {
        let schedule = Schedule::default();
        // Holes of 100 and 300 frames; the second one is the wider.
        let covered = coverage(&[(0, 100), (200, 200), (700, 300)]);
        assert_eq!(covered.gaps(EXTENT).len(), 2, "two holes to choose between");
        assert_eq!(schedule.next(&covered, Some(EXTENT)), Some(550));
    }

    #[kithara::test]
    fn a_covered_extent_schedules_nothing() {
        let schedule = Schedule::default();
        assert_eq!(schedule.next(&coverage(&[(0, EXTENT)]), Some(EXTENT)), None);
    }

    #[kithara::test]
    fn nothing_is_scheduled_without_an_extent() {
        let schedule = Schedule::default();
        assert_eq!(
            schedule.next(&coverage(&[(0, 200)]), None),
            None,
            "a source that reports no duration has no middle to seek to"
        );
    }

    #[kithara::test]
    fn a_position_that_added_nothing_is_not_chosen_again() {
        let mut schedule = Schedule::default();
        let covered = coverage(&[(0, 100), (200, 200), (700, 300)]);
        assert_eq!(schedule.next(&covered, Some(EXTENT)), Some(550));

        // The seek to 550 snapped back into covered audio and added nothing.
        schedule.barren(550);
        assert_eq!(
            schedule.next(&covered, Some(EXTENT)),
            Some(150),
            "the next choice comes from what is still uncovered"
        );

        schedule.barren(150);
        assert_eq!(
            schedule.next(&covered, Some(EXTENT)),
            None,
            "a pass with nowhere left to reach is finished, not spinning"
        );
    }

    fn rate() -> NonZeroU32 {
        NonZeroU32::new(44_100).expect("test rate is non-zero")
    }

    #[kithara::test]
    fn an_extent_is_measured_on_the_pass_axis() {
        let mut extent = Extent::default();
        assert_eq!(extent.frames(), None, "nothing is reported yet");

        extent.report(Some(Duration::from_secs(2)), rate());
        assert_eq!(extent.frames(), Some(88_200));

        extent.report(None, rate());
        assert_eq!(
            extent.frames(),
            Some(88_200),
            "a live source reports no duration, which retracts nothing"
        );
        extent.report(Some(Duration::ZERO), rate());
        assert_eq!(extent.frames(), Some(88_200));
    }

    #[kithara::test]
    fn a_reported_length_grows_and_a_proved_one_bounds_it() {
        let mut extent = Extent::default();
        extent.report(Some(Duration::from_secs(2)), rate());
        extent.report(Some(Duration::from_secs(4)), rate());
        assert_eq!(
            extent.frames(),
            Some(176_400),
            "the decode path refines a duration upward as it learns more"
        );

        extent.unreachable(100_000);
        assert_eq!(
            extent.frames(),
            Some(100_000),
            "what the source proved it can reach bounds what it claims"
        );
        extent.report(Some(Duration::from_secs(8)), rate());
        assert_eq!(
            extent.frames(),
            Some(100_000),
            "a larger claim does not undo what the source already proved"
        );
    }
}
