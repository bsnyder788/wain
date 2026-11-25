//! A chair rail with equal mitered boxes of panel moulding beneath it.
//!
//! The boxes sit on the wall itself, one gap in from each corner, so corners
//! do not move them. Only the chair rail meets the next wall: see
//! [`super::corners`].

use crate::geometry::Rect;
use crate::style::PictureFrame;
use crate::units::Length;

use super::align::Division;
use super::zones::{Edge, Run, Zone};
use super::{EndCut, Material, Part, PartKind, best_division, count_hits};

/// The area of a zone that boxes are spread across.
struct Field {
    /// Left edge of the first box.
    x: Length,
    /// Bottom edge and height of every box.
    y: Length,
    height: Length,
    division: Division,
}

/// Works out where boxes can go in a zone, or `None` when it is too low.
fn field(zone: &Zone, style: &PictureFrame, base_top: Length) -> Option<Field> {
    let gap = style.gap;
    // Smaller than this and a box is mostly moulding.
    let min_box = style.moulding_width * 4;

    let rail_bottom = style.chair_rail_height - style.chair_rail_width;
    let y = base_top + gap;
    let height = zone.top.min(rail_bottom) - gap - y;
    if height < min_box {
        return None;
    }

    // Boxes sit one gap in from a corner or casing. Under a window the outer
    // boxes line up with the casing edges, which already puts them one gap
    // from the boxes on either side.
    let inset = |edge: Edge| {
        if zone.under && edge == Edge::Boundary {
            Length::ZERO
        } else {
            gap
        }
    };
    let lead = inset(zone.left);
    Some(Field {
        x: zone.x0 + lead,
        y,
        height,
        division: Division {
            extent: zone.length() - lead - inset(zone.right),
            separator: gap,
            min_bay: min_box,
        },
    })
}

/// How this zone divides into boxes, for aligning widths across the room.
pub(crate) fn division(zone: &Zone, style: &PictureFrame, base_top: Length) -> Option<Division> {
    field(zone, style, base_top).map(|field| field.division)
}

/// Lays out one run with the box count chosen for each of its zones.
pub(crate) fn layout_run(
    run: &Run,
    style: &PictureFrame,
    counts: &[Option<i64>],
    base_top: Length,
    keep_clear: &[Rect],
    parts: &mut Vec<Part>,
) {
    for (zone, &count) in run.zones.iter().zip(counts) {
        layout_zone(run, zone, style, count, base_top, keep_clear, parts);
    }
}

fn layout_zone(
    run: &Run,
    zone: &Zone,
    style: &PictureFrame,
    count: Option<i64>,
    base_top: Length,
    keep_clear: &[Rect],
    parts: &mut Vec<Part>,
) {
    // The chair rail stops at a window casing; the window's own sill and
    // apron take its place.
    if !zone.under {
        let (left, right) = run.joints(zone);
        let projection = style.chair_rail_thickness;
        let x0 = zone.x0 - left.moulding_extension(projection);
        let x1 = zone.x1 + right.moulding_extension(projection);
        let rect = Rect::new(
            x0,
            style.chair_rail_height - style.chair_rail_width,
            x1 - x0,
            style.chair_rail_width,
        );
        let rail = Part::horizontal(PartKind::ChairRail, Material::ChairRail, rect);
        parts.push(rail.with_cuts([left.moulding_cut(), right.moulding_cut()]));
    }

    let (Some(field), Some(ideal)) = (field(zone, style, base_top), count) else {
        return;
    };
    let moulding = style.moulding_width;

    let build = |count: i64| -> Option<Vec<Rect>> {
        let width = field.division.bay(count)?;
        Some(
            (0..count)
                .map(|i| {
                    let x = field.x + (width + style.gap) * i;
                    Rect::new(x, field.y, width, field.height)
                })
                .collect(),
        )
    };
    let hits = |boxes: &Vec<Rect>| {
        boxes
            .iter()
            .map(|frame| count_hits(&sides(frame, moulding), keep_clear))
            .sum()
    };

    let Some(boxes) = best_division(ideal, build, hits) else {
        return;
    };
    let mitered = [EndCut::FrameMiter, EndCut::FrameMiter];
    for frame in &boxes {
        let [bottom, top, left, right] = sides(frame, moulding);
        for rect in [bottom, top] {
            let part = Part::horizontal(PartKind::FrameMoulding, Material::PanelMoulding, rect);
            parts.push(part.with_cuts(mitered));
        }
        for rect in [left, right] {
            let part = Part::vertical(PartKind::FrameMoulding, Material::PanelMoulding, rect);
            parts.push(part.with_cuts(mitered));
        }
    }
}

/// The four strips of moulding around a box: bottom, top, left, right. Each
/// runs the full outside dimension, since mitered pieces are measured between
/// their long points.
fn sides(frame: &Rect, moulding: Length) -> [Rect; 4] {
    [
        Rect::new(frame.x, frame.y, frame.width, moulding),
        Rect::new(frame.x, frame.top() - moulding, frame.width, moulding),
        Rect::new(frame.x, frame.y, moulding, frame.height),
        Rect::new(frame.right() - moulding, frame.y, moulding, frame.height),
    ]
}
