//! A chair rail with equal mitered boxes of panel moulding beneath it.

use crate::geometry::Rect;
use crate::style::PictureFrame;
use crate::units::Length;

use super::zones::{Edge, Zone};
use super::{Material, Part, PartKind, best_division, count_hits, round_div};

pub(crate) fn layout_zone(
    zone: &Zone,
    style: &PictureFrame,
    base_top: Length,
    keep_clear: &[Rect],
    parts: &mut Vec<Part>,
) {
    let length = zone.length();
    let rail_bottom = style.chair_rail_height - style.chair_rail_width;

    // The chair rail stops at a window casing; the window's own sill and
    // apron take its place.
    if !zone.under {
        parts.push(Part::horizontal(
            PartKind::ChairRail,
            Material::ChairRail,
            Rect::new(zone.x0, rail_bottom, length, style.chair_rail_width),
        ));
    }

    let moulding = style.moulding_width;
    let gap = style.gap;
    // Smaller than this and a box is mostly moulding.
    let min_box = moulding * 4;

    let field_top = zone.top.min(rail_bottom);
    let box_y = base_top + gap;
    let box_height = field_top - gap - box_y;
    if box_height < min_box {
        return;
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
    let span = length - lead - inset(zone.right);

    let build = |count: i64| -> Option<Vec<Rect>> {
        let box_width = (span - gap * (count - 1)) / count;
        if box_width < min_box {
            return None;
        }
        Some(
            (0..count)
                .map(|i| {
                    let x = zone.x0 + lead + (box_width + gap) * i;
                    Rect::new(x, box_y, box_width, box_height)
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

    // span = count * box_width + (count - 1) * gap
    let ideal = round_div(span + gap, style.target_box_width + gap).max(1);
    let Some(boxes) = best_division(ideal, build, hits) else {
        return;
    };
    for frame in &boxes {
        let [bottom, top, left, right] = sides(frame, moulding);
        for rect in [bottom, top] {
            let part = Part::horizontal(PartKind::FrameMoulding, Material::PanelMoulding, rect);
            parts.push(part.mitered());
        }
        for rect in [left, right] {
            let part = Part::vertical(PartKind::FrameMoulding, Material::PanelMoulding, rect);
            parts.push(part.mitered());
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
