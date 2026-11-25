//! The wain layout engine.
//!
//! Given a [`Project`] (measured walls, what is on them, and a chosen
//! [`Design`]) the engine works out where every piece of trim goes and what
//! to cut and buy. It is pure and deterministic, with no I/O, so the same
//! code runs natively on the server and as WebAssembly in the browser.
//!
//! ```
//! use wain_core::{Project, UnitSystem, plan};
//!
//! let project = Project::starter(UnitSystem::Imperial);
//! let plan = plan(&project).unwrap();
//! assert!(!plan.cut_list.is_empty());
//! ```

pub mod cutlist;
pub mod geometry;
pub mod layout;
pub mod model;
pub mod stock;
pub mod style;
pub mod units;
pub mod validate;

use serde::{Deserialize, Serialize};

pub use cutlist::CutItem;
pub use geometry::Rect;
pub use layout::{
    EndCut, Joint, Material, Part, PartKind, Profile, WallLayout, Warning, WarningCode,
};
pub use model::{Corner, Obstruction, ObstructionKind, Project, StockOptions, Wall};
pub use stock::{StockBoard, StockCut, StockGroup, StockPurchase};
pub use style::{Baseboard, BoardAndBatten, Design, PictureFrame, ShakerPanel, Style, StyleKind};
pub use units::{Length, ParseLengthError, UnitPrefs, UnitSystem, format_length, parse_length};
pub use validate::{Issue, PlanError};

/// Everything derived from a project: the drawing, the cut list and the
/// shopping list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Plan {
    /// Placed parts for each wall, in the project's wall order.
    pub walls: Vec<WallLayout>,
    pub cut_list: Vec<CutItem>,
    /// Stock to buy and how to cut it, one group per profile.
    pub stock: Vec<StockGroup>,
    pub warnings: Vec<Warning>,
}

/// Lays out every wall of the project and derives its cut list and stock
/// plan. Fails only when the project itself is invalid.
pub fn plan(project: &Project) -> Result<Plan, PlanError> {
    validate::validate(project)?;

    let (mut walls, mut warnings) = layout::layout_project(project);

    let cut_list = cutlist::build(&mut walls, project.stock.cut_precision);
    let (stock, stock_warnings) = stock::plan(&cut_list, &project.stock, &project.units);
    warnings.extend(stock_warnings);

    Ok(Plan {
        walls,
        cut_list,
        stock,
        warnings,
    })
}
