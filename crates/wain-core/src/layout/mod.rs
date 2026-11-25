//! Turns walls and a design into placed parts.
//!
//! Layout works on the whole project at once, in four steps:
//!
//! 1. [`zones`] splits each wall into runs between doors and, within a run,
//!    zones of constant available height.
//! 2. [`corners`] decides how the trim on adjoining walls meets.
//! 3. [`align`] picks a bay count for every zone so that panel widths stay
//!    consistent around the room.
//! 4. A style module fills each zone, and every part is checked against
//!    outlets and switches so conflicts are reported.

mod align;
mod corners;
mod picture_frame;
mod stile_rail;
mod zones;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::model::{ObstructionKind, Project, Wall};
use crate::style::{Design, PictureFrame, Style};
use crate::units::{Length, UnitPrefs, format_length};

pub use corners::Joint;

use align::Division;
use stile_rail::StileRail;
use zones::{Run, Surface, Zone};

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
    Square,
    /// A 45 degree miter across the face, as at the corner of a picture
    /// frame. Length is measured to the long point.
    FrameMiter,
    /// A 45 degree bevel through the thickness for an outside corner, with
    /// the long point on the face. Length is measured to the long point.
    OutsideMiter,
    /// Shaped to fit over the profile of the piece on the adjoining wall at
    /// an inside corner (or inside-mitered, if preferred). Length is measured
    /// to the wall.
    Cope,
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
    /// The cut at each end: left then right for a horizontal part, bottom
    /// then top for a vertical one.
    pub cuts: [EndCut; 2],
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
            cuts: [EndCut::Square; 2],
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
            cuts: [EndCut::Square; 2],
            label: String::new(),
        }
    }

    fn with_cuts(mut self, cuts: [EndCut; 2]) -> Part {
        self.cuts = cuts;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WallLayout {
    pub wall_id: String,
    pub parts: Vec<Part>,
    /// What the trim meets at the wall's left and right ends.
    pub left_joint: Joint,
    pub right_joint: Joint,
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

/// The style-specific half of layout: how zones divide and what goes in them.
enum Solver<'a> {
    PictureFrame(&'a PictureFrame),
    StileRail(StileRail),
}

impl<'a> Solver<'a> {
    fn new(design: &'a Design) -> Solver<'a> {
        match &design.style {
            Style::PictureFrame(style) => Solver::PictureFrame(style),
            Style::BoardAndBatten(style) => Solver::StileRail(StileRail {
                top: style.height,
                top_rail: style.top_rail_width,
                bottom_rail: None,
                member: style.batten_width,
                member_kind: PartKind::Batten,
                target_on_centre: style.target_spacing,
                thickness: style.thickness,
            }),
            Style::ShakerPanel(style) => Solver::StileRail(StileRail {
                top: style.height,
                top_rail: style.top_rail_width,
                bottom_rail: Some(style.bottom_rail_width),
                member: style.stile_width,
                member_kind: PartKind::Stile,
                target_on_centre: style.target_panel_width + style.stile_width,
                thickness: style.thickness,
            }),
        }
    }

    /// The centre-to-centre spacing the user asked for.
    fn target_pitch(&self) -> Length {
        match self {
            Solver::PictureFrame(style) => style.target_box_width + style.gap,
            Solver::StileRail(config) => config.target_on_centre,
        }
    }

    fn division(&self, run: &Run, zone: &Zone, base_top: Length) -> Option<Division> {
        match self {
            Solver::PictureFrame(style) => picture_frame::division(zone, style, base_top),
            Solver::StileRail(config) => config.division(run, zone, base_top),
        }
    }

    fn layout_run(
        &self,
        run: &Run,
        counts: &[Option<i64>],
        base_top: Length,
        keep_clear: &[Rect],
        parts: &mut Vec<Part>,
    ) {
        match self {
            Solver::PictureFrame(style) => {
                picture_frame::layout_run(run, style, counts, base_top, keep_clear, parts);
            }
            Solver::StileRail(config) => {
                config.layout_run(run, counts, base_top, keep_clear, parts);
            }
        }
    }
}

/// Lays out every wall. The project must already have passed validation.
pub(crate) fn layout_project(project: &Project) -> (Vec<WallLayout>, Vec<Warning>) {
    let design = &project.design;
    let base_top = design.baseboard.height;
    let solver = Solver::new(design);

    // 1. Runs and zones for each wall.
    let mut surfaces: Vec<Surface> = project
        .walls
        .iter()
        .map(|wall| zones::analyze(wall, base_top, design.style.top()))
        .collect();

    // 2. How adjoining walls meet.
    let reaches: Vec<(bool, bool)> = project
        .walls
        .iter()
        .zip(&surfaces)
        .map(|(wall, surface)| surface.reaches(wall.width))
        .collect();
    let joints = corners::resolve(&project.walls, &reaches);
    for ((wall, surface), &(left, right)) in project.walls.iter().zip(&mut surfaces).zip(&joints) {
        surface.set_joints(wall.width, left, right);
    }

    // 3. One bay count per zone, agreed across the whole room. The counts are
    //    in wall, run, zone order, which step 4 walks in the same way.
    let divisions: Vec<Option<Division>> = surfaces
        .iter()
        .flat_map(|surface| &surface.runs)
        .flat_map(|run| {
            let solver = &solver;
            run.zones
                .iter()
                .map(move |zone| solver.division(run, zone, base_top))
        })
        .collect();
    let counts = align::align(&divisions, solver.target_pitch());
    let mut counts = counts.as_slice();

    // 4. Parts.
    let mut layouts = Vec::with_capacity(project.walls.len());
    let mut warnings = Vec::new();
    for ((wall, surface), &(left_joint, right_joint)) in
        project.walls.iter().zip(&surfaces).zip(&joints)
    {
        let keep_clear: Vec<Rect> = surface
            .small
            .iter()
            .map(|&(_, rect)| rect.inflate(CLEARANCE))
            .collect();

        let mut parts = Vec::new();
        for run in &surface.runs {
            if !design.baseboard.existing && base_top.is_positive() {
                parts.push(baseboard(run, design));
            }
            let (run_counts, rest) = counts.split_at(run.zones.len());
            counts = rest;
            solver.layout_run(run, run_counts, base_top, &keep_clear, &mut parts);
        }

        warnings.extend(overlap_warnings(wall, surface, &parts, &project.units));
        layouts.push(WallLayout {
            wall_id: wall.id.clone(),
            parts,
            left_joint,
            right_joint,
        });
    }
    (layouts, warnings)
}

/// A new baseboard for one run. It is a profiled moulding, so corners are
/// coped or mitered rather than butted.
fn baseboard(run: &Run, design: &Design) -> Part {
    let projection = design.baseboard.thickness;
    let x0 = run.x0 - run.left.moulding_extension(projection);
    let x1 = run.x1 + run.right.moulding_extension(projection);
    let rect = Rect::new(x0, Length::ZERO, x1 - x0, design.baseboard.height);
    Part::horizontal(PartKind::Baseboard, Material::Baseboard, rect)
        .with_cuts([run.left.moulding_cut(), run.right.moulding_cut()])
}

/// One warning for each outlet, switch or vent that ended up under trim.
fn overlap_warnings(
    wall: &Wall,
    surface: &Surface,
    parts: &[Part],
    units: &UnitPrefs,
) -> Vec<Warning> {
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
    warnings
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

fn count_hits(rects: &[Rect], keep_clear: &[Rect]) -> usize {
    rects
        .iter()
        .filter(|rect| keep_clear.iter().any(|clear| rect.intersects(clear)))
        .count()
}
