//! Groups placed parts into a cut list.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::layout::{EndCut, PartKind, Profile, WallLayout};
use crate::units::Length;

/// A line of the cut list: `quantity` identical pieces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CutItem {
    /// Short label (A, B, ... AA) that also appears on the elevation drawing.
    pub label: String,
    pub kind: PartKind,
    pub profile: Profile,
    /// Finished length, rounded to the project's cut precision. Mitered ends
    /// are measured to the long point and coped ends to the wall.
    pub length: Length,
    /// The cut at each end: left then right as the piece sits on the wall,
    /// or bottom then top for a vertical piece.
    pub cuts: [EndCut; 2],
    pub quantity: u32,
}

type Key = (Profile, PartKind, std::cmp::Reverse<Length>, [EndCut; 2]);

/// Builds the cut list for every wall and writes each item's label back onto
/// the parts it covers.
///
/// Items are ordered by material and width, then part kind, then longest
/// first, which is the order they are worth cutting in.
pub(crate) fn build(layouts: &mut [WallLayout], precision: Length) -> Vec<CutItem> {
    let key_of = |part: &crate::layout::Part| -> Key {
        (
            part.profile,
            part.kind,
            std::cmp::Reverse(part.length.round_to(precision)),
            part.cuts,
        )
    };

    let mut counts: HashMap<Key, u32> = HashMap::new();
    for part in layouts.iter().flat_map(|layout| &layout.parts) {
        *counts.entry(key_of(part)).or_default() += 1;
    }
    let mut keys: Vec<Key> = counts.keys().copied().collect();
    keys.sort();

    let items: Vec<CutItem> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| CutItem {
            label: label(index),
            profile: key.0,
            kind: key.1,
            length: key.2.0,
            cuts: key.3,
            quantity: counts[key],
        })
        .collect();

    let labels: HashMap<Key, &str> = keys
        .iter()
        .zip(&items)
        .map(|(key, item)| (*key, item.label.as_str()))
        .collect();
    for part in layouts.iter_mut().flat_map(|layout| &mut layout.parts) {
        part.label = labels[&key_of(part)].to_owned();
    }
    items
}

/// Spreadsheet-style labels: A..Z, AA..AZ, BA..
fn label(mut index: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).expect("labels are ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_continue_past_z() {
        assert_eq!(label(0), "A");
        assert_eq!(label(25), "Z");
        assert_eq!(label(26), "AA");
        assert_eq!(label(27), "AB");
        assert_eq!(label(51), "AZ");
        assert_eq!(label(52), "BA");
        assert_eq!(label(701), "ZZ");
        assert_eq!(label(702), "AAA");
    }
}
