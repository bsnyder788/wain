//! Packs cut-list pieces into purchasable lengths of stock.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cutlist::CutItem;
use crate::layout::{Profile, Warning, WarningCode};
use crate::model::StockOptions;
use crate::units::{Length, UnitPrefs, format_length};

/// Above this many pieces the stock plan is skipped. Packing is quadratic in
/// the piece count, and no real room comes close.
pub const MAX_PIECES: usize = 4_000;

/// Everything to buy and cut for one profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StockGroup {
    pub profile: Profile,
    /// One entry per board to buy, in cutting order.
    pub boards: Vec<StockBoard>,
    /// The same boards summarised as a shopping list, longest first.
    pub purchase: Vec<StockPurchase>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StockBoard {
    pub stock_length: Length,
    pub cuts: Vec<StockCut>,
    /// What is left after every cut and its kerf.
    pub offcut: Length,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StockCut {
    /// The cut-list label this piece belongs to.
    pub label: String,
    pub length: Length,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StockPurchase {
    pub stock_length: Length,
    pub quantity: u32,
}

/// Plans stock for the whole cut list, one group per profile.
pub(crate) fn plan(
    items: &[CutItem],
    options: &StockOptions,
    units: &UnitPrefs,
) -> (Vec<StockGroup>, Vec<Warning>) {
    let mut warnings = Vec::new();

    let total: usize = items.iter().map(|item| item.quantity as usize).sum();
    if total > MAX_PIECES {
        warnings.push(Warning {
            code: WarningCode::StockPlanSkipped,
            wall_id: None,
            message: format!(
                "This project has {total} pieces, more than the {MAX_PIECES} the stock planner \
                 handles. The cut list is complete, but no shopping list was produced."
            ),
        });
        return (Vec::new(), warnings);
    }

    let mut stock_lengths = options.lengths.clone();
    stock_lengths.sort();
    stock_lengths.dedup();
    let Some(&longest_stock) = stock_lengths.last() else {
        return (Vec::new(), warnings);
    };

    let mut by_profile: BTreeMap<Profile, Vec<StockCut>> = BTreeMap::new();
    for item in items {
        let pieces = by_profile.entry(item.profile).or_default();
        // A piece longer than any stock is made from several lengths joined
        // end to end: as many full lengths as needed, then the remainder.
        let mut segments = Vec::new();
        let mut remaining = item.length;
        while remaining > longest_stock {
            segments.push(longest_stock);
            remaining -= longest_stock;
        }
        if remaining.is_positive() {
            segments.push(remaining);
        }
        if segments.len() > 1 {
            warnings.push(Warning {
                code: WarningCode::NeedsJoint,
                wall_id: None,
                message: format!(
                    "Part {} ({}, {}) is longer than the longest stock ({}), so each one is \
                     planned as {} pieces joined end to end.",
                    item.label,
                    item.kind.name(),
                    format_length(item.length, units),
                    format_length(longest_stock, units),
                    segments.len(),
                ),
            });
        }
        for _ in 0..item.quantity {
            for &length in &segments {
                pieces.push(StockCut {
                    label: item.label.clone(),
                    length,
                });
            }
        }
    }

    let groups = by_profile
        .into_iter()
        .map(|(profile, pieces)| {
            let boards = pack(pieces, &stock_lengths, options.kerf);
            let purchase = summarize(&boards);
            StockGroup {
                profile,
                boards,
                purchase,
            }
        })
        .collect();
    (groups, warnings)
}

/// Packs pieces into boards, choosing a stock length for each board.
///
/// Each round fills one trial board per stock length, taking the longest
/// remaining pieces that fit, and keeps whichever trial wastes the smallest
/// share of its board (the shorter board on a tie). Letting every board pick
/// its own length means one long rail does not force every short piece onto
/// long, expensive stock.
///
/// `stock_lengths` must be sorted ascending and every piece must fit the
/// longest one.
fn pack(mut pieces: Vec<StockCut>, stock_lengths: &[Length], kerf: Length) -> Vec<StockBoard> {
    pieces.sort_by(|a, b| b.length.cmp(&a.length).then_with(|| a.label.cmp(&b.label)));
    let mut placed = vec![false; pieces.len()];
    let mut remaining = pieces.len();
    let mut boards = Vec::new();

    while remaining > 0 {
        // (stock length, indexes of the pieces taken, their total length)
        let mut best: Option<(Length, Vec<usize>, Length)> = None;
        for &stock in stock_lengths {
            let mut taken = Vec::new();
            let mut used = Length::ZERO;
            let mut filled = Length::ZERO;
            for (index, piece) in pieces.iter().enumerate() {
                if placed[index] || used + piece.length > stock {
                    continue;
                }
                taken.push(index);
                used += piece.length + kerf;
                filled += piece.length;
            }
            let better = match &best {
                None => !taken.is_empty(),
                // Compare fill ratios filled/stock without dividing.
                Some((best_stock, _, best_filled)) => {
                    i128::from(filled.nm()) * i128::from(best_stock.nm())
                        > i128::from(best_filled.nm()) * i128::from(stock.nm())
                }
            };
            if better {
                best = Some((stock, taken, filled));
            }
        }

        let Some((stock, taken, filled)) = best else {
            // Unreachable while every piece fits the longest stock.
            break;
        };
        let cut_count = taken.len() as i64;
        let cuts = taken
            .iter()
            .map(|&index| {
                placed[index] = true;
                pieces[index].clone()
            })
            .collect();
        remaining -= taken.len();
        boards.push(StockBoard {
            stock_length: stock,
            cuts,
            offcut: (stock - filled - kerf * cut_count).max(Length::ZERO),
        });
    }
    boards
}

fn summarize(boards: &[StockBoard]) -> Vec<StockPurchase> {
    let mut counts: BTreeMap<Length, u32> = BTreeMap::new();
    for board in boards {
        *counts.entry(board.stock_length).or_default() += 1;
    }
    counts
        .into_iter()
        .rev()
        .map(|(stock_length, quantity)| StockPurchase {
            stock_length,
            quantity,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(lengths: &[i64]) -> Vec<StockCut> {
        lengths
            .iter()
            .map(|&inches| StockCut {
                label: "A".into(),
                length: Length::inches(inches),
            })
            .collect()
    }

    fn feet(lengths: &[i64]) -> Vec<Length> {
        lengths.iter().map(|&f| Length::feet(f)).collect()
    }

    fn bought(boards: &[StockBoard]) -> Vec<(i64, usize)> {
        boards
            .iter()
            .map(|b| (b.stock_length.nm() / Length::feet(1).nm(), b.cuts.len()))
            .collect()
    }

    #[test]
    fn kerf_decides_how_many_pieces_fit() {
        let stock = feet(&[8]);
        // Two 48" pieces fill a 96" board only if the saw removes nothing.
        let no_kerf = pack(pieces(&[48, 48]), &stock, Length::ZERO);
        assert_eq!(bought(&no_kerf), [(8, 2)]);
        let with_kerf = pack(pieces(&[48, 48]), &stock, Length::inch_fraction(1, 8));
        assert_eq!(bought(&with_kerf), [(8, 1), (8, 1)]);
    }

    #[test]
    fn a_piece_that_exactly_fills_a_board_needs_no_kerf() {
        let boards = pack(pieces(&[96]), &feet(&[8]), Length::inch_fraction(1, 8));
        assert_eq!(bought(&boards), [(8, 1)]);
        assert_eq!(boards[0].offcut, Length::ZERO);
    }

    #[test]
    fn each_board_picks_its_own_stock_length() {
        // One 12' rail plus short battens: the battens should not be forced
        // onto 12' boards just because the rail needs one.
        let mut all = pieces(&[144]);
        all.extend(pieces(&[45; 4]));
        let boards = pack(all, &feet(&[8, 12, 16]), Length::inch_fraction(1, 8));
        assert_eq!(bought(&boards), [(12, 1), (8, 2), (8, 2)]);
    }

    #[test]
    fn ties_go_to_the_shorter_board() {
        // 45" pieces fill 8', 12' and 16' boards equally well.
        let boards = pack(pieces(&[45; 2]), &feet(&[8, 12, 16]), Length::ZERO);
        assert_eq!(bought(&boards), [(8, 2)]);
    }

    #[test]
    fn every_piece_is_placed_exactly_once() {
        let lengths = [90, 70, 61, 45, 45, 33, 30, 30, 22, 12, 12, 7];
        let boards = pack(
            pieces(&lengths),
            &feet(&[8, 12]),
            Length::inch_fraction(1, 8),
        );
        let mut placed: Vec<i64> = boards
            .iter()
            .flat_map(|b| &b.cuts)
            .map(|c| c.length.nm() / Length::inches(1).nm())
            .collect();
        placed.sort_by(|a, b| b.cmp(a));
        assert_eq!(placed, lengths);
        for board in &boards {
            let cut: Length = board.cuts.iter().map(|c| c.length).sum();
            assert!(cut <= board.stock_length);
        }
    }

    #[test]
    fn summary_counts_boards_longest_first() {
        let mut all = pieces(&[144]);
        all.extend(pieces(&[45; 4]));
        let boards = pack(all, &feet(&[8, 12, 16]), Length::inch_fraction(1, 8));
        assert_eq!(
            summarize(&boards),
            [
                StockPurchase {
                    stock_length: Length::feet(12),
                    quantity: 1
                },
                StockPurchase {
                    stock_length: Length::feet(8),
                    quantity: 2
                },
            ]
        );
    }
}
