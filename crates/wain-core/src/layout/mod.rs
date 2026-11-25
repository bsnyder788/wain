//! Turns a wall and a design into placed parts.
//!
//! Layout works in three steps. [`zones`] splits the wall into runs between
//! doors and, within each run, zones of constant available height. A style
//! module then fills each zone with equally spaced members. Finally every
//! part is checked against outlets and switches so conflicts are reported.

mod picture_frame;
mod stile_rail;
mod zones;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::model::{ObstructionKind, Wall};
use crate::style::{Design, Style};
use crate::units::{Length, UnitPrefs, format_length};

use stile_rail::StileRail;

/// Space kept clear around outlets, switches and vents when choosing where
/// members go.
const CLEARANCE: Length = Length::mm(6);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Baseboard,
    ChairRail,
    TopRail,
    BottomRail,
    Stile,
    Batten,
    FrameMoulding,
}

impl PartKind {
    pub fn name(self) -> &'static str {
        match self {
            PartKind::Baseboard => "baseboard",
            PartKind::ChairRail => "chair rail",
            PartKind::TopRail => "top rail",
            PartKind::BottomRail => "bottom rail",
            PartKind::Stile => "stile",
            PartKind::Batten => "batten",
            PartKind::FrameMoulding => "frame moulding",
        }
    }
}

/// What a part is cut from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Baseboard,
    /// Flat square-edged stock (boards or ripped MDF).
    Board,
    ChairRail,
    PanelMoulding,
}

impl Material {
    pub fn name(self) -> &'static str {
        match self {
            Material::Baseboard => "baseboard",
            Material::Board => "board",
            Material::ChairRail => "chair rail",
            Material::PanelMoulding => "panel moulding",
        }
    }
}

/// A material and face width. Parts that share a profile can be cut from the
/// same lengths of stock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Profile {
    pub material: Material,
    pub width: Length,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum EndCut {
    /// Both ends cut square.
    Square,
    /// Both ends mitered at 45 degrees. The part's length is measured between
    /// the long points.
    Miter,
}

/// One piece of trim placed on a wall.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Part {
    pub kind: PartKind,
    pub profile: Profile,
    /// Where the part sits on the wall elevation.
    pub rect: Rect,
    /// Exact length along the part's long axis.
    pub length: Length,
    pub end_cut: EndCut,
    /// Cut-list label shared by identical parts. Empty until the cut list is
    /// built.
    pub label: String,
}

impl Part {
    fn horizontal(kind: PartKind, material: Material, rect: Rect) -> Part {
        Part {
            kind,
            profile: Profile {
                material,
                width: rect.height,
            },
            rect,
            length: rect.width,
            end_cut: EndCut::Square,
            label: String::new(),
        }
    }

    fn vertical(kind: PartKind, material: Material, rect: Rect) -> Part {
        Part {
            kind,
            profile: Profile {
                material,
                width: rect.width,
            },
            rect,
            length: rect.height,
            end_cut: EndCut::Square,
            label: String::new(),
        }
    }

    fn mitered(mut self) -> Part {
        self.end_cut = EndCut::Miter;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WallLayout {
    pub wall_id: String,
    pub parts: Vec<Part>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    /// Trim covers an outlet, switch or vent and no nearby spacing avoids it.
    ObstructionOverlap,
    /// A part is longer than any stock length and must be joined.
    NeedsJoint,
    /// The job has too many pieces to plan stock for.
    StockPlanSkipped,
}

/// Something the user should know about before cutting. Plans with warnings
/// are still usable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Warning {
    pub code: WarningCode,
    pub wall_id: Option<String>,
    pub message: String,
}

/// Lays out one wall. The project must already have passed validation.
pub(crate) fn layout_wall(
    wall: &Wall,
    design: &Design,
    units: &UnitPrefs,
) -> (WallLayout, Vec<Warning>) {
    let base_top = design.baseboard.height;
    let surface = zones::analyze(wall, base_top, design.style.top());
    let keep_clear: Vec<Rect> = surface
        .small
        .iter()
        .map(|&(_, rect)| rect.inflate(CLEARANCE))
        .collect();

    let mut parts = Vec::new();
    for run in &surface.runs {
        if !design.baseboard.existing && base_top.is_positive() {
            parts.push(Part::horizontal(
                PartKind::Baseboard,
                Material::Baseboard,
                Rect::new(run.x0, Length::ZERO, run.x1 - run.x0, base_top),
            ));
        }
        match &design.style {
            Style::PictureFrame(style) => {
                for zone in &run.zones {
                    picture_frame::layout_zone(zone, style, base_top, &keep_clear, &mut parts);
                }
            }
            Style::BoardAndBatten(style) => {
                let config = StileRail {
                    top: style.height,
                    top_rail: style.top_rail_width,
                    bottom_rail: None,
                    member: style.batten_width,
                    member_kind: PartKind::Batten,
                    target_on_centre: style.target_spacing,
                };
                stile_rail::layout_run(run, &config, base_top, &keep_clear, &mut parts);
            }
            Style::ShakerPanel(style) => {
                let config = StileRail {
                    top: style.height,
                    top_rail: style.top_rail_width,
                    bottom_rail: Some(style.bottom_rail_width),
                    member: style.stile_width,
                    member_kind: PartKind::Stile,
                    target_on_centre: style.target_panel_width + style.stile_width,
                };
                stile_rail::layout_run(run, &config, base_top, &keep_clear, &mut parts);
            }
        }
    }

    let mut warnings = Vec::new();
    for &(index, rect) in &surface.small {
        let Some(hit) = parts.iter().find(|part| part.rect.intersects(&rect)) else {
            continue;
        };
        let kind = wall.obstructions[index].kind;
        warnings.push(Warning {
            code: WarningCode::ObstructionOverlap,
            wall_id: Some(wall.id.clone()),
            message: format!(
                "{}: the {} {} from the left is partly covered by a {}. \
                 Adjust the spacing or height, or plan to notch the trim around it.",
                wall.name,
                obstruction_name(kind),
                format_length(rect.x, units),
                hit.kind.name(),
            ),
        });
    }

    let layout = WallLayout {
        wall_id: wall.id.clone(),
        parts,
    };
    (layout, warnings)
}

fn obstruction_name(kind: ObstructionKind) -> &'static str {
    match kind {
        ObstructionKind::Door => "door",
        ObstructionKind::Opening => "opening",
        ObstructionKind::Window => "window",
        ObstructionKind::Outlet => "outlet",
        ObstructionKind::Switch => "switch",
        ObstructionKind::Vent => "vent",
        ObstructionKind::Other => "obstruction",
    }
}

/// Tries division counts near `ideal`, nearest first, and returns the layout
/// with the fewest collisions. `build` returns `None` for a count that does
/// not fit. Ties go to the count nearest `ideal`.
fn best_division<T>(
    ideal: i64,
    build: impl Fn(i64) -> Option<T>,
    collisions: impl Fn(&T) -> usize,
) -> Option<T> {
    let mut best: Option<(usize, T)> = None;
    for offset in [0, 1, -1, 2, -2] {
        let count = ideal + offset;
        if count < 1 {
            continue;
        }
        let Some(candidate) = build(count) else {
            continue;
        };
        let score = collisions(&candidate);
        if score == 0 {
            return Some(candidate);
        }
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score < *best_score)
        {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// `a / b` rounded to the nearest whole number; zero when `a` is negative.
fn round_div(a: Length, b: Length) -> i64 {
    if a.nm() <= 0 || b.nm() <= 0 {
        return 0;
    }
    (a.nm() * 2 + b.nm()) / (b.nm() * 2)
}

fn count_hits(rects: &[Rect], keep_clear: &[Rect]) -> usize {
    rects
        .iter()
        .filter(|rect| keep_clear.iter().any(|clear| rect.intersects(clear)))
        .count()
}
