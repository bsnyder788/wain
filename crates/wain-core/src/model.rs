//! The project a user edits: walls, what is on them, and the chosen design.

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::style::{Design, StyleKind};
use crate::units::{Length, UnitPrefs, UnitSystem};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Project {
    pub name: String,
    pub units: UnitPrefs,
    pub walls: Vec<Wall>,
    pub design: Design,
    pub stock: StockOptions,
}

impl Project {
    /// A one-wall starter project with sensible defaults for the unit system.
    pub fn starter(system: UnitSystem) -> Project {
        let (width, height) = match system {
            UnitSystem::Imperial => (Length::feet(12), Length::feet(8)),
            UnitSystem::Metric => (Length::mm(3600), Length::mm(2400)),
        };
        Project {
            name: "Untitled room".to_owned(),
            units: UnitPrefs::for_system(system),
            walls: vec![Wall {
                id: "wall-1".to_owned(),
                name: "Wall 1".to_owned(),
                width,
                height,
                obstructions: Vec::new(),
            }],
            design: Design::default_for(StyleKind::BoardAndBatten, system),
            stock: StockOptions::default_for(system),
        }
    }
}

/// One flat wall, measured by the user with a tape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Wall {
    pub id: String,
    pub name: String,
    /// Corner to corner.
    pub width: Length,
    /// Floor to ceiling.
    pub height: Length,
    pub obstructions: Vec<Obstruction>,
}

/// Something on the wall that trim has to stop at or work around.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Obstruction {
    pub kind: ObstructionKind,
    /// The outer edge of the obstruction, including any casing or cover plate.
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ObstructionKind {
    /// Trim stops at the casing on both sides.
    Door,
    /// A cased or uncased opening with no door; treated like a door.
    Opening,
    /// Trim runs up to the casing and continues underneath if there is room.
    Window,
    Outlet,
    Switch,
    Vent,
    /// Anything else. Wide ones behave like a window, small ones like an outlet.
    Other,
}

/// What lumber can be bought and how it is cut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StockOptions {
    /// Board lengths the store sells.
    pub lengths: Vec<Length>,
    /// Width of material the saw blade removes per cut.
    pub kerf: Length,
    /// Cut-list lengths are rounded to this, e.g. 1/16" or 1 mm.
    pub cut_precision: Length,
}

impl StockOptions {
    pub fn default_for(system: UnitSystem) -> StockOptions {
        match system {
            UnitSystem::Imperial => StockOptions {
                lengths: vec![Length::feet(8), Length::feet(12), Length::feet(16)],
                kerf: Length::inch_fraction(1, 8),
                cut_precision: Length::inch_fraction(1, 16),
            },
            UnitSystem::Metric => StockOptions {
                lengths: vec![Length::mm(2400), Length::mm(3600), Length::mm(4800)],
                kerf: Length::mm(3),
                cut_precision: Length::mm(1),
            },
        }
    }
}
