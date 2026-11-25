//! How trim on adjoining walls meets at a corner.
//!
//! Where two trimmed walls meet, one wall's trim runs *through* to the corner
//! and the other *butts* against it. The convention is fixed: at the corner
//! after a wall, that wall runs through and the next wall butts. Installing
//! walls in project order therefore always works.
//!
//! Everything a joint changes is expressed relative to the wall's own end,
//! with positive distances pointing outward, past the end of the wall.

use serde::{Deserialize, Serialize};

use crate::model::{Corner, Wall};
use crate::units::Length;

use super::EndCut;

/// What one end of a wall's trim meets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Joint {
    /// Nothing: the trim ends square at the wall end or a casing.
    #[default]
    Free,
    /// Inside corner, and this wall's trim runs into the corner. The last
    /// board thickness of it is hidden behind the other wall's trim.
    InsideThrough,
    /// Inside corner, and this wall's trim stops one board thickness short,
    /// against the face of the other wall's trim.
    InsideButt,
    /// Outside corner, and this wall's trim runs past the corner by one board
    /// thickness to cover the edge of the other wall's trim.
    OutsideThrough,
    /// Outside corner, and this wall's trim stops flush with the corner,
    /// behind the edge of the other wall's trim.
    OutsideButt,
}

impl Joint {
    /// How far the visible face of flat board trim reaches past the wall end.
    /// The visible corner sits one thickness inside an inside corner and one
    /// thickness outside an outside corner, on both walls.
    pub(crate) fn board_reveal(self, thickness: Length) -> Length {
        match self {
            Joint::Free => Length::ZERO,
            Joint::InsideThrough | Joint::InsideButt => -thickness,
            Joint::OutsideThrough | Joint::OutsideButt => thickness,
        }
    }

    /// How far a board rail physically reaches past the wall end. Rails are
    /// butted at inside corners and mitered at outside corners, where the
    /// long point is on the face.
    pub(crate) fn rail_extension(self, thickness: Length) -> Length {
        match self {
            Joint::Free | Joint::InsideThrough => Length::ZERO,
            Joint::InsideButt => -thickness,
            Joint::OutsideThrough | Joint::OutsideButt => thickness,
        }
    }

    pub(crate) fn rail_cut(self) -> EndCut {
        match self {
            Joint::OutsideThrough | Joint::OutsideButt => EndCut::OutsideMiter,
            _ => EndCut::Square,
        }
    }

    /// Where the outer edge of the corner batten or stile sits, past the wall
    /// end. Its inner edge is set by the visible layout, so this also decides
    /// how wide the piece has to be cut.
    pub(crate) fn member_outer(self, thickness: Length) -> Length {
        match self {
            Joint::Free | Joint::InsideThrough | Joint::OutsideButt => Length::ZERO,
            Joint::InsideButt => -thickness,
            Joint::OutsideThrough => thickness,
        }
    }

    /// How far a profiled moulding (chair rail, baseboard) reaches past the
    /// wall end. At an inside corner both pieces measure to the wall; at an
    /// outside corner the miter's long point is on the face.
    pub(crate) fn moulding_extension(self, projection: Length) -> Length {
        match self {
            Joint::OutsideThrough | Joint::OutsideButt => projection,
            _ => Length::ZERO,
        }
    }

    pub(crate) fn moulding_cut(self) -> EndCut {
        match self {
            Joint::Free | Joint::InsideThrough => EndCut::Square,
            Joint::InsideButt => EndCut::Cope,
            Joint::OutsideThrough | Joint::OutsideButt => EndCut::OutsideMiter,
        }
    }
}

/// Works out the joint at each end of every wall, as `(left, right)`.
///
/// `reaches[i]` says whether wall `i` has trim running all the way to its
/// left and right ends. A corner only becomes a joint when both walls do: if
/// one has a door in the corner, the other simply runs to the wall.
pub(crate) fn resolve(walls: &[Wall], reaches: &[(bool, bool)]) -> Vec<(Joint, Joint)> {
    let mut joints = vec![(Joint::Free, Joint::Free); walls.len()];
    if walls.len() < 2 {
        // A lone wall has no next wall to turn into.
        return joints;
    }
    for (index, wall) in walls.iter().enumerate() {
        let next = (index + 1) % walls.len();
        let (through, butt) = match wall.corner {
            Corner::None => continue,
            Corner::Inside => (Joint::InsideThrough, Joint::InsideButt),
            Corner::Outside => (Joint::OutsideThrough, Joint::OutsideButt),
        };
        if reaches[index].1 && reaches[next].0 {
            joints[index].1 = through;
            joints[next].0 = butt;
        }
    }
    joints
}
