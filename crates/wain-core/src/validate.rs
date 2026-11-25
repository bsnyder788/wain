//! Checks a project before layout.
//!
//! Projects arrive from the browser and, on the server, from the network, so
//! nothing here is trusted. Besides catching mistakes, the limits bound how
//! much work a single request can ask for.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Project, StockOptions, Wall, WallPhoto};
use crate::style::{Design, Style};
use crate::units::Length;

pub const MAX_WALLS: usize = 40;
pub const MAX_OBSTRUCTIONS_PER_WALL: usize = 100;
pub const MAX_STOCK_LENGTHS: usize = 12;
const MAX_WALL_WIDTH: Length = Length::mm(30_000);
const MAX_WALL_HEIGHT: Length = Length::mm(10_000);
const MAX_STOCK_LENGTH: Length = Length::mm(10_000);
const MIN_STOCK_LENGTH: Length = Length::mm(300);
/// Narrowest allowed trim face.
const MIN_FACE: Length = Length::mm(5);
/// Trim stock is never thinner or thicker than this.
const MIN_THICKNESS: Length = Length::mm(1);
const MAX_THICKNESS: Length = Length::mm(100);
/// Smallest allowed target spacing; keeps member counts bounded.
const MIN_SPACING: Length = Length::mm(50);
const MAX_KERF: Length = Length::mm(15);
const MAX_IMAGE_ID_LENGTH: usize = 64;
const MAX_IMAGE_SIDE: u32 = 20_000;
/// How far outside the image a wall corner may be placed, in image sizes.
const MAX_CORNER_OVERHANG: f64 = 4.0;
const MAX_CUT_PRECISION: Length = Length::mm(30);

/// One problem with a project, located by a JSON-style path such as
/// `walls[0].width`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Issue {
    pub path: String,
    pub message: String,
}

/// The project cannot be laid out until these issues are fixed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PlanError {
    pub issues: Vec<Issue>,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid project")?;
        for issue in &self.issues {
            write!(f, "; {}: {}", issue.path, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for PlanError {}

struct Checker {
    issues: Vec<Issue>,
}

impl Checker {
    fn fail(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            path: path.into(),
            message: message.into(),
        });
    }

    /// Requires `min <= value <= max`; returns whether it held.
    fn range(&mut self, path: &str, value: Length, min: Length, max: Length, what: &str) -> bool {
        if value < min {
            self.fail(path, format!("{what} is too small"));
            false
        } else if value > max {
            self.fail(path, format!("{what} is too large"));
            false
        } else {
            true
        }
    }
}

pub(crate) fn validate(project: &Project) -> Result<(), PlanError> {
    let mut check = Checker { issues: Vec::new() };

    if project.walls.is_empty() {
        check.fail("walls", "add at least one wall");
    } else if project.walls.len() > MAX_WALLS {
        check.fail(
            "walls",
            format!("a project can have at most {MAX_WALLS} walls"),
        );
    }
    for (index, wall) in project.walls.iter().enumerate().take(MAX_WALLS) {
        check_wall(&mut check, &format!("walls[{index}]"), wall);
    }

    let lowest_ceiling = project.walls.iter().map(|wall| wall.height).min();
    check_design(&mut check, &project.design, lowest_ceiling);
    check_stock(&mut check, &project.stock);

    if check.issues.is_empty() {
        Ok(())
    } else {
        Err(PlanError {
            issues: check.issues,
        })
    }
}

fn check_wall(check: &mut Checker, path: &str, wall: &Wall) {
    let one = Length::mm(1);
    let width_ok = check.range(
        &format!("{path}.width"),
        wall.width,
        one,
        MAX_WALL_WIDTH,
        "wall width",
    );
    let height_ok = check.range(
        &format!("{path}.height"),
        wall.height,
        one,
        MAX_WALL_HEIGHT,
        "wall height",
    );

    if let Some(photo) = &wall.photo {
        check_photo(check, &format!("{path}.photo"), photo);
    }

    if wall.obstructions.len() > MAX_OBSTRUCTIONS_PER_WALL {
        check.fail(
            format!("{path}.obstructions"),
            format!("a wall can have at most {MAX_OBSTRUCTIONS_PER_WALL} obstructions"),
        );
        return;
    }
    if !(width_ok && height_ok) {
        return;
    }
    for (index, obstruction) in wall.obstructions.iter().enumerate() {
        let path = format!("{path}.obstructions[{index}]");
        let rect = &obstruction.rect;
        if !rect.width.is_positive() || !rect.height.is_positive() {
            check.fail(path, "width and height must be greater than zero");
        } else if rect.x < Length::ZERO
            || rect.y < Length::ZERO
            || rect.width > wall.width
            || rect.height > wall.height
            || rect.x > wall.width - rect.width
            || rect.y > wall.height - rect.height
        {
            check.fail(path, "must fit within the wall");
        }
    }
}

/// Checks only that the photo record is sane. Whether the corners outline a
/// usable shape is the preview's concern, not the plan's: a half-placed
/// corner must not stop the cut list from updating.
fn check_photo(check: &mut Checker, path: &str, photo: &WallPhoto) {
    let id = &photo.image_id;
    let id_ok = !id.is_empty()
        && id.len() <= MAX_IMAGE_ID_LENGTH
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !id_ok {
        check.fail(format!("{path}.image_id"), "is not a valid image id");
    }
    let side_ok = |side: u32| (1..=MAX_IMAGE_SIDE).contains(&side);
    if !side_ok(photo.width) || !side_ok(photo.height) {
        check.fail(path, "image size is out of range");
    }
    let range = -MAX_CORNER_OVERHANG..=1.0 + MAX_CORNER_OVERHANG;
    if photo
        .corners
        .iter()
        .any(|p| !range.contains(&p.x) || !range.contains(&p.y))
    {
        check.fail(format!("{path}.corners"), "are too far outside the photo");
    }
}

fn check_design(check: &mut Checker, design: &Design, lowest_ceiling: Option<Length>) {
    let limit = MAX_WALL_HEIGHT;
    let base = design.baseboard.height;
    let base_ok = check.range(
        "design.baseboard.height",
        base,
        Length::ZERO,
        limit,
        "baseboard height",
    );
    check.range(
        "design.baseboard.thickness",
        design.baseboard.thickness,
        MIN_THICKNESS,
        MAX_THICKNESS,
        "baseboard thickness",
    );

    let thickness = |check: &mut Checker, field: &str, value: Length| {
        check.range(
            &format!("design.style.{field}"),
            value,
            MIN_THICKNESS,
            MAX_THICKNESS,
            &field.replace('_', " "),
        );
    };
    let face = |check: &mut Checker, field: &str, value: Length| {
        check.range(
            &format!("design.style.{field}"),
            value,
            MIN_FACE,
            limit,
            &field.replace('_', " "),
        )
    };

    // `rails` are the horizontal parts that must fit between the baseboard
    // and the top of the treatment. They are only added up once every value
    // is known to be in range, so hostile input cannot overflow the sum.
    let (top_field, top, parts_ok, rails) = match &design.style {
        Style::PictureFrame(s) => {
            thickness(check, "chair_rail_thickness", s.chair_rail_thickness);
            let mut ok = face(check, "chair_rail_width", s.chair_rail_width);
            ok &= face(check, "moulding_width", s.moulding_width);
            ok &= check.range("design.style.gap", s.gap, Length::ZERO, limit, "gap");
            ok &= check.range(
                "design.style.target_box_width",
                s.target_box_width,
                MIN_SPACING,
                MAX_WALL_WIDTH,
                "target box width",
            );
            (
                "chair_rail_height",
                s.chair_rail_height,
                ok,
                [s.chair_rail_width, Length::ZERO],
            )
        }
        Style::BoardAndBatten(s) => {
            thickness(check, "thickness", s.thickness);
            let mut ok = face(check, "top_rail_width", s.top_rail_width);
            ok &= face(check, "batten_width", s.batten_width);
            ok &= check.range(
                "design.style.target_spacing",
                s.target_spacing,
                MIN_SPACING,
                MAX_WALL_WIDTH,
                "target spacing",
            );
            ("height", s.height, ok, [s.top_rail_width, Length::ZERO])
        }
        Style::ShakerPanel(s) => {
            thickness(check, "thickness", s.thickness);
            let mut ok = face(check, "top_rail_width", s.top_rail_width);
            ok &= face(check, "bottom_rail_width", s.bottom_rail_width);
            ok &= face(check, "stile_width", s.stile_width);
            ok &= check.range(
                "design.style.target_panel_width",
                s.target_panel_width,
                MIN_SPACING,
                MAX_WALL_WIDTH,
                "target panel width",
            );
            (
                "height",
                s.height,
                ok,
                [s.top_rail_width, s.bottom_rail_width],
            )
        }
    };

    let top_path = format!("design.style.{top_field}");
    let top_ok = check.range(&top_path, top, MIN_FACE, limit, "height");
    if !(base_ok && parts_ok && top_ok) {
        return;
    }
    if lowest_ceiling.is_some_and(|ceiling| top > ceiling) {
        check.fail(top_path, "is taller than the wall");
    } else if top - rails[0] - rails[1] <= base {
        check.fail(top_path, "is too low to fit the rails above the baseboard");
    }
}

fn check_stock(check: &mut Checker, stock: &StockOptions) {
    if stock.lengths.is_empty() {
        check.fail("stock.lengths", "add at least one stock length");
    } else if stock.lengths.len() > MAX_STOCK_LENGTHS {
        check.fail(
            "stock.lengths",
            format!("at most {MAX_STOCK_LENGTHS} stock lengths are supported"),
        );
    }
    for (index, &length) in stock.lengths.iter().enumerate().take(MAX_STOCK_LENGTHS) {
        check.range(
            &format!("stock.lengths[{index}]"),
            length,
            MIN_STOCK_LENGTH,
            MAX_STOCK_LENGTH,
            "stock length",
        );
    }
    check.range("stock.kerf", stock.kerf, Length::ZERO, MAX_KERF, "kerf");
    check.range(
        "stock.cut_precision",
        stock.cut_precision,
        Length::ZERO,
        MAX_CUT_PRECISION,
        "cut precision",
    );
}
