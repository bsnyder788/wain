//! Equally spaced vertical members under a top rail. Board and batten and
//! shaker panelling are both this shape; shaker adds a bottom rail.

use crate::geometry::Rect;
use crate::units::Length;

use super::zones::{Edge, Run, Zone};
use super::{Material, Part, PartKind, best_division, count_hits, round_div};

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
}

/// Lays out one run: a bottom rail if the style has one, then each zone.
pub(crate) fn layout_run(
    run: &Run,
    config: &StileRail,
    base_top: Length,
    keep_clear: &[Rect],
    parts: &mut Vec<Part>,
) {
    if let Some(bottom_rail) = config.bottom_rail {
        // The bottom rail sits on the baseboard and passes under windows in
        // one piece. It only breaks where a window is too low to clear it.
        let fits =
            |zone: &Zone| zone.top.min(config.top) - config.top_rail >= base_top + bottom_rail;
        let mut span: Option<(Length, Length)> = None;
        let flush = |span: &mut Option<(Length, Length)>, parts: &mut Vec<Part>| {
            if let Some((x0, x1)) = span.take() {
                parts.push(Part::horizontal(
                    PartKind::BottomRail,
                    Material::Board,
                    Rect::new(x0, base_top, x1 - x0, bottom_rail),
                ));
            }
        };
        for zone in &run.zones {
            if fits(zone) {
                span = Some((span.map_or(zone.x0, |(x0, _)| x0), zone.x1));
            } else {
                flush(&mut span, parts);
            }
        }
        flush(&mut span, parts);
    }

    for zone in &run.zones {
        layout_zone(zone, config, base_top, keep_clear, parts);
    }
}

fn layout_zone(
    zone: &Zone,
    config: &StileRail,
    base_top: Length,
    keep_clear: &[Rect],
    parts: &mut Vec<Part>,
) {
    let length = zone.length();
    let top = zone.top.min(config.top);
    let rail_y = top - config.top_rail;
    let members_y = base_top + config.bottom_rail.unwrap_or(Length::ZERO);
    if rail_y < members_y {
        // Not enough height for even the rails, e.g. under a very low window.
        return;
    }

    parts.push(Part::horizontal(
        PartKind::TopRail,
        Material::Board,
        Rect::new(zone.x0, rail_y, length, config.top_rail),
    ));

    let member_length = rail_y - members_y;
    if member_length < MIN_MEMBER_LENGTH {
        return;
    }

    // A full-height zone gets a member at each end, against the corner or
    // casing. Under a window the neighbouring zones already supply those, so
    // only the members in between are placed.
    let end_members = if zone.under {
        (zone.left == Edge::RunEnd, zone.right == Edge::RunEnd)
    } else {
        (true, true)
    };
    let end_count = i64::from(end_members.0) + i64::from(end_members.1);
    let width = config.member;
    let member_rect = |x: Length| Rect::new(x, members_y, width, member_length);

    let build = |bays: i64| -> Option<Vec<Rect>> {
        let members = bays - 1 + end_count;
        let bay = (length - width * members) / bays;
        // A gap narrower than the member itself reads as a mistake.
        if bay < width {
            return None;
        }
        let lead = if end_members.0 { width } else { Length::ZERO };
        let mut rects = Vec::with_capacity(members as usize);
        if end_members.0 {
            rects.push(member_rect(zone.x0));
        }
        for i in 0..bays - 1 {
            rects.push(member_rect(zone.x0 + lead + (bay + width) * i + bay));
        }
        if end_members.1 {
            rects.push(member_rect(zone.x1 - width));
        }
        Some(rects)
    };

    // length = bays * on_centre + (end_count - 1) * width
    let ideal = round_div(length - width * (end_count - 1), config.target_on_centre).max(1);
    let Some(rects) = best_division(ideal, build, |rects| count_hits(rects, keep_clear)) else {
        return;
    };
    parts.extend(
        rects
            .into_iter()
            .map(|rect| Part::vertical(config.member_kind, Material::Board, rect)),
    );
}
