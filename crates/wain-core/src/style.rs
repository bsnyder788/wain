//! The wall-treatment styles the engine can lay out, and their parameters.

use serde::{Deserialize, Serialize};

use crate::units::{Length, UnitSystem};

/// A chosen style plus the trim that every style shares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Design {
    pub style: Style,
    pub baseboard: Baseboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Baseboard {
    pub height: Length,
    /// An existing baseboard stays in place and is left off the cut list.
    pub existing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Style {
    PictureFrame(PictureFrame),
    BoardAndBatten(BoardAndBatten),
    ShakerPanel(ShakerPanel),
}

impl Style {
    pub fn kind(&self) -> StyleKind {
        match self {
            Style::PictureFrame(_) => StyleKind::PictureFrame,
            Style::BoardAndBatten(_) => StyleKind::BoardAndBatten,
            Style::ShakerPanel(_) => StyleKind::ShakerPanel,
        }
    }

    /// Height above the floor of the top of the treatment.
    pub fn top(&self) -> Length {
        match self {
            Style::PictureFrame(s) => s.chair_rail_height,
            Style::BoardAndBatten(s) => s.height,
            Style::ShakerPanel(s) => s.height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum StyleKind {
    PictureFrame,
    BoardAndBatten,
    ShakerPanel,
}

impl StyleKind {
    pub const ALL: [StyleKind; 3] = [
        StyleKind::PictureFrame,
        StyleKind::BoardAndBatten,
        StyleKind::ShakerPanel,
    ];
}

/// A chair rail with mitered boxes of panel moulding applied to the wall
/// beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PictureFrame {
    /// Floor to the top of the chair rail.
    pub chair_rail_height: Length,
    /// Face width of the chair rail.
    pub chair_rail_width: Length,
    /// Face width of the panel moulding that forms each box.
    pub moulding_width: Length,
    /// Space between boxes, and between a box and the baseboard, chair rail,
    /// corner or casing.
    pub gap: Length,
    /// Preferred box width. The solver picks the box count that makes equal
    /// boxes closest to this.
    pub target_box_width: Length,
}

/// Vertical battens between the baseboard and a top rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BoardAndBatten {
    /// Floor to the top of the top rail.
    pub height: Length,
    pub top_rail_width: Length,
    pub batten_width: Length,
    /// Preferred centre-to-centre batten spacing.
    pub target_spacing: Length,
}

/// Flat stiles and rails applied to the wall so the wall itself reads as
/// recessed panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ShakerPanel {
    /// Floor to the top of the top rail.
    pub height: Length,
    pub top_rail_width: Length,
    /// The bottom rail sits directly on the baseboard.
    pub bottom_rail_width: Length,
    pub stile_width: Length,
    /// Preferred clear width of each panel between stiles.
    pub target_panel_width: Length,
}

impl Design {
    /// Starting dimensions for a style. Each unit system gets round numbers
    /// in its own units, sized to lumber sold in that system.
    pub fn default_for(kind: StyleKind, system: UnitSystem) -> Design {
        let imperial = system == UnitSystem::Imperial;
        let pick = |inches: Length, mm: i64| if imperial { inches } else { Length::mm(mm) };
        let style = match kind {
            StyleKind::PictureFrame => Style::PictureFrame(PictureFrame {
                chair_rail_height: pick(Length::inches(34), 860),
                chair_rail_width: pick(Length::inch_fraction(5, 2), 60),
                moulding_width: pick(Length::inch_fraction(9, 8), 30),
                gap: pick(Length::inch_fraction(7, 2), 90),
                target_box_width: pick(Length::inches(28), 700),
            }),
            StyleKind::BoardAndBatten => Style::BoardAndBatten(BoardAndBatten {
                height: pick(Length::inches(54), 1350),
                top_rail_width: pick(Length::inch_fraction(7, 2), 90),
                batten_width: pick(Length::inch_fraction(5, 2), 65),
                target_spacing: pick(Length::inches(16), 400),
            }),
            StyleKind::ShakerPanel => Style::ShakerPanel(ShakerPanel {
                height: pick(Length::inches(40), 1000),
                top_rail_width: pick(Length::inch_fraction(7, 2), 90),
                bottom_rail_width: pick(Length::inch_fraction(7, 2), 90),
                stile_width: pick(Length::inch_fraction(7, 2), 90),
                target_panel_width: pick(Length::inches(18), 450),
            }),
        };
        Design {
            style,
            baseboard: Baseboard {
                height: pick(Length::inch_fraction(21, 4), 120),
                existing: true,
            },
        }
    }
}
