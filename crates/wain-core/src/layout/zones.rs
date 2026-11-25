//! Splits a wall into the stretches that trim can occupy.

use crate::geometry::Rect;
use crate::model::{ObstructionKind, Wall};
use crate::units::Length;

use super::corners::Joint;

/// An `Other` obstruction at least this wide is handled like a window.
const WIDE_OBSTRUCTION: Length = Length::mm(300);

/// A window needs at least this much wall between the baseboard and its
/// underside for trim to continue beneath it. Lower than that, trim stops at
/// the casing as it would for a door.
const MIN_HEIGHT_UNDER: Length = Length::mm(150);

/// What a zone has at one of its ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    /// A wall corner or a door casing.
    RunEnd,
    /// The side of a window; the neighbouring zone continues past it.
    Boundary,
}

/// A stretch of wall with one available height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Zone {
    pub x0: Length,
    pub x1: Length,
    /// Trim may not rise above this.
    pub top: Length,
    /// True when the zone sits beneath a window.
    pub under: bool,
    pub left: Edge,
    pub right: Edge,
}

impl Zone {
    pub fn length(&self) -> Length {
        self.x1 - self.x0
    }
}

/// An unbroken stretch of wall between corners and door casings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub x0: Length,
    pub x1: Length,
    pub zones: Vec<Zone>,
    /// What the run meets at each end. `Free` at a door casing, and at a wall
    /// end until [`Surface::set_joints`] says otherwise.
    pub left: Joint,
    pub right: Joint,
}

impl Run {
    /// The joints at the two ends of `zone`. Only a zone at the end of the
    /// run can meet a corner.
    pub fn joints(&self, zone: &Zone) -> (Joint, Joint) {
        let at = |edge: Edge, joint: Joint| match edge {
            Edge::RunEnd => joint,
            Edge::Boundary => Joint::Free,
        };
        (at(zone.left, self.left), at(zone.right, self.right))
    }
}

pub(crate) struct Surface {
    pub runs: Vec<Run>,
    /// Outlets, switches and similar within the treated area, with their
    /// index in `wall.obstructions`.
    pub small: Vec<(usize, Rect)>,
}

impl Surface {
    /// Whether trim runs all the way to the wall's left and right ends,
    /// rather than stopping at a door in the corner.
    pub fn reaches(&self, wall_width: Length) -> (bool, bool) {
        let left = self.runs.first().is_some_and(|run| run.x0 == Length::ZERO);
        let right = self.runs.last().is_some_and(|run| run.x1 == wall_width);
        (left, right)
    }

    /// Records what the wall's two ends meet. Ignored for an end the trim
    /// does not reach.
    pub fn set_joints(&mut self, wall_width: Length, left: Joint, right: Joint) {
        let (reaches_left, reaches_right) = self.reaches(wall_width);
        if reaches_left && let Some(run) = self.runs.first_mut() {
            run.left = left;
        }
        if reaches_right && let Some(run) = self.runs.last_mut() {
            run.right = right;
        }
    }
}

enum Class {
    Ignore,
    Blocker,
    Under,
    Small,
}

fn classify(kind: ObstructionKind, rect: &Rect, base_top: Length, top: Length) -> Class {
    // Entirely above the treatment or down in the baseboard: no effect.
    if rect.y >= top || rect.top() <= base_top {
        return Class::Ignore;
    }
    let window_like = match kind {
        ObstructionKind::Door | ObstructionKind::Opening => return Class::Blocker,
        ObstructionKind::Outlet | ObstructionKind::Switch | ObstructionKind::Vent => false,
        ObstructionKind::Window => true,
        ObstructionKind::Other => rect.width >= WIDE_OBSTRUCTION,
    };
    if !window_like {
        Class::Small
    } else if rect.y - base_top < MIN_HEIGHT_UNDER {
        Class::Blocker
    } else {
        Class::Under
    }
}

/// Works out the runs and zones of `wall` for a treatment that spans from
/// `base_top` (top of the baseboard) up to `top`.
pub(crate) fn analyze(wall: &Wall, base_top: Length, top: Length) -> Surface {
    let mut blockers: Vec<(Length, Length)> = Vec::new();
    let mut unders: Vec<(Length, Length, Length)> = Vec::new();
    let mut small = Vec::new();

    for (index, obstruction) in wall.obstructions.iter().enumerate() {
        let rect = &obstruction.rect;
        match classify(obstruction.kind, rect, base_top, top) {
            Class::Ignore => {}
            Class::Blocker => blockers.push((rect.x, rect.right())),
            Class::Under => unders.push((rect.x, rect.right(), rect.y)),
            Class::Small => small.push((index, *rect)),
        }
    }

    blockers.sort();
    unders.sort();

    // Runs are what is left of the wall after removing the blockers.
    let mut spans = Vec::new();
    let mut cursor = Length::ZERO;
    for (start, end) in blockers {
        if start > cursor {
            spans.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < wall.width {
        spans.push((cursor, wall.width));
    }

    let runs = spans
        .into_iter()
        .map(|(x0, x1)| Run {
            x0,
            x1,
            zones: zones_in_run(x0, x1, top, &unders),
            left: Joint::Free,
            right: Joint::Free,
        })
        .collect();

    Surface { runs, small }
}

fn zones_in_run(
    run_x0: Length,
    run_x1: Length,
    top: Length,
    unders: &[(Length, Length, Length)],
) -> Vec<Zone> {
    // Clip the windows to this run and merge any that overlap, keeping the
    // lowest underside.
    let mut merged: Vec<(Length, Length, Length)> = Vec::new();
    for &(start, end, under_top) in unders {
        let (start, end) = (start.max(run_x0), end.min(run_x1));
        if start >= end {
            continue;
        }
        match merged.last_mut() {
            Some(last) if start <= last.1 => {
                last.1 = last.1.max(end);
                last.2 = last.2.min(under_top);
            }
            _ => merged.push((start, end, under_top)),
        }
    }

    let edge = |x: Length| {
        if x == run_x0 || x == run_x1 {
            Edge::RunEnd
        } else {
            Edge::Boundary
        }
    };
    let zone = |x0: Length, x1: Length, zone_top: Length, under: bool| Zone {
        x0,
        x1,
        top: zone_top,
        under,
        left: edge(x0),
        right: edge(x1),
    };

    let mut zones = Vec::new();
    let mut cursor = run_x0;
    for (start, end, under_top) in merged {
        if start > cursor {
            zones.push(zone(cursor, start, top, false));
        }
        zones.push(zone(start, end, under_top, true));
        cursor = end;
    }
    if cursor < run_x1 {
        zones.push(zone(cursor, run_x1, top, false));
    }
    zones
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Obstruction;

    fn wall(width: i64, obstructions: Vec<Obstruction>) -> Wall {
        Wall {
            id: "w".into(),
            name: "Wall".into(),
            width: Length::inches(width),
            height: Length::inches(96),
            obstructions,
            corner: crate::model::Corner::None,
        }
    }

    fn obstruction(kind: ObstructionKind, x: i64, y: i64, w: i64, h: i64) -> Obstruction {
        Obstruction {
            kind,
            rect: Rect::new(
                Length::inches(x),
                Length::inches(y),
                Length::inches(w),
                Length::inches(h),
            ),
        }
    }

    fn analyze_default(wall: &Wall) -> Surface {
        analyze(wall, Length::inches(5), Length::inches(54))
    }

    fn spans(surface: &Surface) -> Vec<(i64, i64)> {
        let inch = Length::inches(1).nm();
        surface
            .runs
            .iter()
            .map(|run| (run.x0.nm() / inch, run.x1.nm() / inch))
            .collect()
    }

    #[test]
    fn a_bare_wall_is_one_run_with_one_zone() {
        let surface = analyze_default(&wall(144, vec![]));
        assert_eq!(spans(&surface), [(0, 144)]);
        let zones = &surface.runs[0].zones;
        assert_eq!(zones.len(), 1);
        assert!(!zones[0].under);
        assert_eq!(
            (zones[0].left, zones[0].right),
            (Edge::RunEnd, Edge::RunEnd)
        );
        assert_eq!(zones[0].top, Length::inches(54));
    }

    #[test]
    fn doors_split_the_wall_into_runs() {
        let door = obstruction(ObstructionKind::Door, 40, 0, 36, 82);
        let surface = analyze_default(&wall(144, vec![door]));
        assert_eq!(spans(&surface), [(0, 40), (76, 144)]);
    }

    #[test]
    fn a_door_at_the_corner_leaves_one_run() {
        let door = obstruction(ObstructionKind::Door, 0, 0, 36, 82);
        let surface = analyze_default(&wall(144, vec![door]));
        assert_eq!(spans(&surface), [(36, 144)]);
    }

    #[test]
    fn overlapping_doors_merge() {
        let a = obstruction(ObstructionKind::Door, 40, 0, 36, 82);
        let b = obstruction(ObstructionKind::Opening, 70, 0, 30, 82);
        let surface = analyze_default(&wall(144, vec![b, a]));
        assert_eq!(spans(&surface), [(0, 40), (100, 144)]);
    }

    #[test]
    fn a_low_window_makes_a_zone_beneath_it() {
        let window = obstruction(ObstructionKind::Window, 50, 30, 40, 50);
        let surface = analyze_default(&wall(144, vec![window]));
        assert_eq!(spans(&surface), [(0, 144)]);
        let zones = &surface.runs[0].zones;
        assert_eq!(zones.len(), 3);
        assert_eq!(
            zones.iter().map(|z| z.under).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(zones[1].top, Length::inches(30));
        assert_eq!(
            (zones[1].x0, zones[1].x1),
            (Length::inches(50), Length::inches(90))
        );
        assert_eq!(
            (zones[0].left, zones[0].right),
            (Edge::RunEnd, Edge::Boundary)
        );
        assert_eq!(
            (zones[1].left, zones[1].right),
            (Edge::Boundary, Edge::Boundary)
        );
    }

    #[test]
    fn a_window_above_the_treatment_is_ignored() {
        let window = obstruction(ObstructionKind::Window, 50, 54, 40, 30);
        let surface = analyze_default(&wall(144, vec![window]));
        assert_eq!(surface.runs[0].zones.len(), 1);
    }

    #[test]
    fn a_window_too_close_to_the_baseboard_blocks_like_a_door() {
        let window = obstruction(ObstructionKind::Window, 50, 8, 40, 60);
        let surface = analyze_default(&wall(144, vec![window]));
        assert_eq!(spans(&surface), [(0, 50), (90, 144)]);
    }

    #[test]
    fn small_obstructions_are_collected_not_zoned() {
        let outlet = obstruction(ObstructionKind::Outlet, 30, 12, 3, 5);
        let thermostat = obstruction(ObstructionKind::Other, 60, 48, 4, 4);
        let high_switch = obstruction(ObstructionKind::Switch, 100, 60, 3, 5);
        let surface = analyze_default(&wall(144, vec![outlet, thermostat, high_switch]));
        assert_eq!(surface.runs[0].zones.len(), 1);
        let indexes: Vec<usize> = surface.small.iter().map(|(i, _)| *i).collect();
        assert_eq!(indexes, [0, 1]);
    }
}
