//! Equally spaced vertical members under a top rail. Board and batten and
//! shaker panelling are both this shape; shaker adds a bottom rail.
//!
//! The members are laid out on the *visible* face of the trim, so every wall
//! shows a full-width member at each end and equal bays between, whatever
//! happens at the corners. Corner joints then only change how the hidden
//! parts are cut: see [`super::corners`].

use crate::geometry::Rect;
use crate::units::Length;

use super::align::Division;
use super::zones::{Edge, Run, Zone};
use super::{EndCut, Material, Part, PartKind, best_division, count_hits};

/// Members shorter than this are left out; only the rails are placed.
const MIN_MEMBER_LENGTH: Length = Length::mm(50);

pub(crate) struct StileRail {
    /// Floor to the top of the top rail.
    pub top: Length,
    pub top_rail: Length,
    pub bottom_rail: Option<Length>,
    /// Face width of each vertical member.
    pub member: Length,
    pub member_kind: PartKind,
    /// Preferred centre-to-centre spacing of the members.
    pub target_on_centre: Length,
    /// Thickness of the boards, which sets the overlap at corners.
    pub thickness: Length,
}

/// The fixed geometry of one zone, before the bay count is chosen.
struct Frame {
    /// The top rail as cut, including any corner extension.
    rail: Rect,
    rail_cuts: [EndCut; 2],
    /// The visible face that the members are spaced across.
    visible: (Length, Length),
    /// Bottom and length of the members; `None` when they would be too short.
    members: Option<(Length, Length)>,
    /// Whether there is a member at the left and right end of the zone.
    end_members: (bool, bool),
    /// Where the outer edges of those end members are cut to.
    outer: (Length, Length),
}

impl StileRail {
    /// Works out a zone's rails and visible span, or `None` when there is not
    /// enough height for the rails (under a very low window, say).
    fn frame(&self, run: &Run, zone: &Zone, base_top: Length) -> Option<Frame> {
        let top = zone.top.min(self.top);
        let rail_y = top - self.top_rail;
        let members_y = base_top + self.bottom_rail.unwrap_or(Length::ZERO);
        if rail_y < members_y {
            return None;
        }

        let (left, right) = run.joints(zone);
        let t = self.thickness;
        let rail_x0 = zone.x0 - left.rail_extension(t);
        let rail_x1 = zone.x1 + right.rail_extension(t);
        let visible = (
            zone.x0 - left.board_reveal(t),
            zone.x1 + right.board_reveal(t),
        );
        if rail_x1 <= rail_x0 || visible.1 <= visible.0 {
            return None;
        }

        // A full-height zone gets a member at each end, against the corner or
        // casing. Under a window the neighbouring zones already supply those,
        // so only the members in between are placed.
        let end_members = if zone.under {
            (zone.left == Edge::RunEnd, zone.right == Edge::RunEnd)
        } else {
            (true, true)
        };
        let member_length = rail_y - members_y;

        Some(Frame {
            rail: Rect::new(rail_x0, rail_y, rail_x1 - rail_x0, self.top_rail),
            rail_cuts: [left.rail_cut(), right.rail_cut()],
            visible,
            members: (member_length >= MIN_MEMBER_LENGTH).then_some((members_y, member_length)),
            end_members,
            outer: (
                zone.x0 - left.member_outer(t),
                zone.x1 + right.member_outer(t),
            ),
        })
    }

    fn divide(&self, frame: &Frame) -> Option<Division> {
        frame.members?;
        let end_count = i64::from(frame.end_members.0) + i64::from(frame.end_members.1);
        Some(Division {
            extent: frame.visible.1 - frame.visible.0 - self.member * end_count,
            separator: self.member,
            // A gap narrower than the member itself reads as a mistake.
            min_bay: self.member,
        })
    }

    /// How this zone divides into bays, for aligning spacing across the room.
    pub(crate) fn division(&self, run: &Run, zone: &Zone, base_top: Length) -> Option<Division> {
        self.divide(&self.frame(run, zone, base_top)?)
    }

    /// Lays out one run: a bottom rail if the style has one, then each zone
    /// with the bay count chosen for it.
    pub(crate) fn layout_run(
        &self,
        run: &Run,
        counts: &[Option<i64>],
        base_top: Length,
        keep_clear: &[Rect],
        parts: &mut Vec<Part>,
    ) {
        if let Some(width) = self.bottom_rail {
            self.bottom_rails(run, width, base_top, parts);
        }
        for (zone, &count) in run.zones.iter().zip(counts) {
            self.layout_zone(run, zone, count, base_top, keep_clear, parts);
        }
    }

    /// The bottom rail sits on the baseboard and passes under windows in one
    /// piece. It only breaks where a window is too low to clear the rails.
    fn bottom_rails(&self, run: &Run, width: Length, base_top: Length, parts: &mut Vec<Part>) {
        let t = self.thickness;
        let fits: Vec<bool> = run
            .zones
            .iter()
            .map(|zone| self.frame(run, zone, base_top).is_some())
            .collect();

        let mut start = 0;
        while start < fits.len() {
            if !fits[start] {
                start += 1;
                continue;
            }
            let mut end = start;
            while end + 1 < fits.len() && fits[end + 1] {
                end += 1;
            }
            // Only a piece that reaches the end of the run meets the corner.
            let (left, _) = run.joints(&run.zones[start]);
            let (_, right) = run.joints(&run.zones[end]);
            let x0 = run.zones[start].x0 - left.rail_extension(t);
            let x1 = run.zones[end].x1 + right.rail_extension(t);
            if x1 > x0 {
                let rect = Rect::new(x0, base_top, x1 - x0, width);
                let rail = Part::horizontal(PartKind::BottomRail, Material::Board, rect);
                parts.push(rail.with_cuts([left.rail_cut(), right.rail_cut()]));
            }
            start = end + 1;
        }
    }

    fn layout_zone(
        &self,
        run: &Run,
        zone: &Zone,
        count: Option<i64>,
        base_top: Length,
        keep_clear: &[Rect],
        parts: &mut Vec<Part>,
    ) {
        let Some(frame) = self.frame(run, zone, base_top) else {
            return;
        };
        let rail = Part::horizontal(PartKind::TopRail, Material::Board, frame.rail);
        parts.push(rail.with_cuts(frame.rail_cuts));

        let (Some(division), Some(ideal), Some((y, length))) =
            (self.divide(&frame), count, frame.members)
        else {
            return;
        };
        let width = self.member;
        let (visible_x0, visible_x1) = frame.visible;
        let member = |x0: Length, x1: Length| Rect::new(x0, y, x1 - x0, length);

        let build = |bays: i64| -> Option<Vec<Rect>> {
            let bay = division.bay(bays)?;
            let lead = if frame.end_members.0 {
                width
            } else {
                Length::ZERO
            };
            let mut rects = Vec::new();
            // The end members show `width` of face. Their outer edges are cut
            // to suit the corner, which can make them wider or narrower.
            if frame.end_members.0 {
                rects.push(member(frame.outer.0, visible_x0 + width));
            }
            for i in 0..bays - 1 {
                let x = visible_x0 + lead + (bay + width) * i + bay;
                rects.push(member(x, x + width));
            }
            if frame.end_members.1 {
                rects.push(member(visible_x1 - width, frame.outer.1));
            }
            // An outside corner can leave nothing of a very narrow member.
            rects.retain(|rect| rect.width.is_positive());
            Some(rects)
        };

        let hits = |rects: &Vec<Rect>| count_hits(rects, keep_clear);
        let Some(rects) = best_division(ideal, build, hits) else {
            return;
        };
        parts.extend(
            rects
                .into_iter()
                .map(|rect| Part::vertical(self.member_kind, Material::Board, rect)),
        );
    }
}
