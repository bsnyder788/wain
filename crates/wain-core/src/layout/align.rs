//! Keeps panel widths consistent around the room.
//!
//! Every zone is divided into equal bays (the spaces between battens, the
//! panels between stiles, or the picture-frame boxes). Left alone, each zone
//! would pick the bay count nearest the target spacing, and two walls could
//! round in opposite directions and end up visibly different. Instead the
//! room agrees on one spacing: starting from the target, each zone picks its
//! nearest count, the spacing moves to halfway between the target and what
//! the zones actually achieved, and this repeats until nothing changes.
//!
//! All arithmetic is exact integer maths, so the browser and the server
//! always reach the same answer.

use crate::units::Length;

/// Rounds of adjustment before settling. In practice two or three suffice.
const MAX_ROUNDS: usize = 12;

/// A stretch to be split into equal bays with a fixed separator between
/// neighbours (a batten, a stile, or the gap between boxes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Division {
    /// Combined width of the bays and the separators between them.
    pub extent: Length,
    pub separator: Length,
    /// Counts that would make bays narrower than this are not allowed.
    pub min_bay: Length,
}

impl Division {
    /// The centre-to-centre distance of bays times the bay count: a pitch of
    /// `pitch_span() / count` fills the extent exactly.
    fn pitch_span(&self) -> Length {
        self.extent + self.separator
    }

    /// Width of each bay when split into `count` bays, or `None` when that
    /// many do not fit.
    pub fn bay(&self, count: i64) -> Option<Length> {
        if count < 1 {
            return None;
        }
        let bay = (self.extent - self.separator * (count - 1)) / count;
        (bay >= self.min_bay && bay.is_positive()).then_some(bay)
    }

    /// The bay count whose centre-to-centre spacing is nearest `pitch`, or
    /// `None` when not even one bay fits. A tie goes to the larger count.
    pub fn count_for(&self, pitch: Length) -> Option<i64> {
        let span = i128::from(self.pitch_span().nm());
        let pitch = i128::from(pitch.nm());
        let smallest_pitch = i128::from((self.min_bay + self.separator).nm());
        if span <= 0 || pitch <= 0 || smallest_pitch <= 0 {
            return None;
        }
        let most = span / smallest_pitch;
        if most < 1 {
            return None;
        }

        let lower = (span / pitch).max(1);
        let upper = lower + 1;
        // span/lower - pitch < pitch - span/upper, cross-multiplied.
        let lower_is_nearer = span * (lower + upper) < 2 * pitch * lower * upper;
        let count = if lower_is_nearer { lower } else { upper };
        let count = count.min(most) as i64;
        // `most` is derived from the same rounding as `bay`, but confirm.
        self.bay(count).map(|_| count)
    }
}

/// Chooses a bay count for every division so that spacing is as even as
/// possible across all of them while staying near `target_pitch`. Longer
/// stretches carry more weight. Entries that are `None`, or where nothing
/// fits, come back as `None`.
pub(crate) fn align(divisions: &[Option<Division>], target_pitch: Length) -> Vec<Option<i64>> {
    let assign = |pitch: Length| -> Vec<Option<i64>> {
        divisions
            .iter()
            .map(|division| division.and_then(|d| d.count_for(pitch)))
            .collect()
    };

    let mut counts = assign(target_pitch);
    for _ in 0..MAX_ROUNDS {
        // Mean of the achieved pitches, weighted by how long each stretch is.
        let mut weighted = 0i128;
        let mut weight = 0i128;
        for (division, count) in divisions.iter().zip(&counts) {
            if let (Some(division), Some(count)) = (division, count) {
                let span = i128::from(division.pitch_span().nm());
                weighted += span * (span / i128::from(*count));
                weight += span;
            }
        }
        if weight == 0 {
            break;
        }
        let achieved = weighted / weight;
        let consensus = (achieved + i128::from(target_pitch.nm())) / 2;
        let next = assign(Length::from_nm(consensus as i64));
        if next == counts {
            break;
        }
        counts = next;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board-and-batten stretch with a batten at each end, its length
    /// given in tenths of an inch.
    fn battens(tenths_of_inch: i64) -> Division {
        let batten = Length::inch_fraction(5, 2);
        Division {
            extent: Length::inch_fraction(tenths_of_inch, 10) - batten * 2,
            separator: batten,
            min_bay: batten,
        }
    }

    const SIXTEEN: Length = Length::inches(16);

    #[test]
    fn bays_fill_the_extent() {
        let division = battens(1440);
        // Nine bays: (144 - 10 battens) / 9.
        let bay = division.bay(9).unwrap();
        assert_eq!(
            bay,
            (Length::inches(144) - Length::inch_fraction(5, 2) * 10) / 9
        );
        assert_eq!(division.bay(0), None);
        // 60 bays would each be narrower than a batten.
        assert_eq!(division.bay(60), None);
    }

    #[test]
    fn picks_the_count_nearest_the_pitch() {
        // 141.5 / 9 = 15.7 is nearer 16 than 141.5 / 8 = 17.7.
        assert_eq!(battens(1440).count_for(SIXTEEN), Some(9));
        // 37.5 / 2 = 18.75 is nearer 16 than 37.5 / 3 = 12.5.
        assert_eq!(battens(400).count_for(SIXTEEN), Some(2));
        // Shorter than one pitch still gets a single bay.
        assert_eq!(battens(120).count_for(SIXTEEN), Some(1));
    }

    #[test]
    fn respects_the_minimum_bay() {
        // 7.5" holds two battens and one 2.5" bay, exactly the minimum.
        assert_eq!(battens(75).count_for(SIXTEEN), Some(1));
        // Any narrower and nothing fits.
        assert_eq!(battens(74).count_for(SIXTEEN), None);
        // A tiny pitch is capped at the most bays that fit: five 3" bays.
        assert_eq!(battens(300).count_for(Length::inches(1)), Some(5));
    }

    #[test]
    fn one_division_just_takes_its_nearest_count() {
        assert_eq!(align(&[Some(battens(1440))], SIXTEEN), [Some(9)]);
        assert_eq!(align(&[None], SIXTEEN), [None]);
        assert_eq!(align(&[], SIXTEEN), []);
    }

    #[test]
    fn a_borderline_stretch_follows_the_rest_of_the_room() {
        // On its own a 57.5" stretch splits into four 13.75" pitches, which is
        // a touch nearer 16 than three of 18.3.
        let short = battens(575);
        assert_eq!(align(&[Some(short)], SIXTEEN), [Some(4)]);

        // Two 89.5" stretches each land on five 17.4" pitches. Next to those,
        // three bays in the short stretch match far better than four.
        let long = battens(895);
        assert_eq!(
            align(&[Some(long), Some(long), Some(short)], SIXTEEN),
            [Some(5), Some(5), Some(3)]
        );
    }

    #[test]
    fn stretches_that_fit_nothing_do_not_disturb_the_rest() {
        let counts = align(&[Some(battens(1440)), Some(battens(50)), None], SIXTEEN);
        assert_eq!(counts, [Some(9), None, None]);
    }
}
