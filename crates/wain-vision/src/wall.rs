//! Picks out the wall in a photo: the four lines where it meets the
//! ceiling, the floor and the walls either side.
//!
//! Long straight edges are found first. Those that run roughly level are
//! candidates for the ceiling and floor lines, and upright ones for the two
//! ends of the wall. Every combination of four is scored, and the best one
//! wins. An outline scores well when:
//!
//! - its sides lie on real edges, and those edges stop at its corners
//!   rather than running on past them;
//! - a third line ends at each corner, as the neighbouring wall's ceiling
//!   and floor lines do in a real room;
//! - what is inside it is the colour of the wall, which is taken to be the
//!   commonest colour around the middle of the photo.
//!
//! A side with no convincing edge falls back to the border of the photo, on
//! the assumption that the wall runs out of shot there.
//!
//! None of this understands the picture. A photo of a room's corner, or one
//! crowded with furniture, can give an outline that is partly or wholly
//! wrong, so the result is a starting point for the user to correct.

use serde::{Deserialize, Serialize};
use wain_core::Point;

use crate::Photo;
use crate::lines::{Line, find_lines};
use crate::raster::{Gradient, Rgb};

/// The photo is shrunk to this before looking for lines. Small enough that
/// wall texture and image noise average away, large enough to place a
/// corner within about a quarter of a percent of the photo's size.
const WORK_SIDE: usize = 384;
const BLUR: f32 = 1.0;
/// Weakest edge worth keeping, in brightness levels per pixel. Low, because
/// a wall and ceiling painted the same colour differ only by shading.
const EDGE_THRESHOLD: f32 = 1.5;
/// A line must be backed by an edge this long, as a share of the photo's
/// shorter side.
const MIN_LINE: f32 = 0.15;
/// How many candidate lines to try for each side of the wall.
const PER_SIDE: usize = 12;

/// The wall's ends lean at most this far from upright, in degrees.
const MAX_LEAN: f32 = 22.0;
/// The ceiling and floor lines slope at most this far from level.
const MAX_SLOPE: f32 = 32.0;
/// The wall takes up at least this share of the photo each way.
const MIN_SPAN: f32 = 0.25;

/// Cost of each pixel of a side that has no edge under it, against a gain
/// of one for each pixel that does.
const MISSING: f32 = 0.5;
/// Cost of each pixel of edge that carries on past a corner. A real ceiling
/// line stops at the end of the wall; a line that runs straight on says the
/// corner is somewhere else.
const OVERSHOOT: f32 = 0.5;
/// What each corner is worth when a third line ends there, as the ceiling
/// line of the neighbouring wall does at a real corner of the room.
const JUNCTION: f32 = 0.2;
/// A baseboard is at most this tall, as a share of the wall's height.
const MAX_BASEBOARD: f32 = 0.09;
/// What an outline full of wall-coloured pixels is worth if it covers the
/// whole photo, in the same units as a side that is fully on an edge.
const FILL: f32 = 2.0;
/// Cost of a pixel inside the outline that is not wall-coloured, against a
/// gain of one for a pixel that is. This is what keeps the outline from
/// spreading over the floor.
const NOT_WALL: f32 = 1.0;

/// Where a wall sits in a photo.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WallOutline {
    /// The wall's corners, ordered top-left, top-right, bottom-right,
    /// bottom-left, as fractions of the photo's width and height.
    pub corners: [Point; 4],
    /// Which sides of the outline follow an edge that was seen in the photo.
    /// The others are guesses that sit on the photo's border.
    pub sides: SidesFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SidesFound {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl SidesFound {
    pub fn count(&self) -> usize {
        [self.top, self.right, self.bottom, self.left]
            .iter()
            .filter(|&&found| found)
            .count()
    }
}

/// The lines found in a photo and the outline chosen from them, for
/// inspecting how detection went. Positions are fractions of the photo.
#[doc(hidden)]
pub struct WallTrace {
    /// Each line as the two ends of its supported stretch.
    pub lines: Vec<[Point; 2]>,
    pub outline: Option<WallOutline>,
}

pub(crate) fn find(photo: &Photo) -> Option<WallOutline> {
    trace(photo).outline
}

pub(crate) fn trace(photo: &Photo) -> WallTrace {
    let image = Rgb::from_photo(photo, WORK_SIDE).blurred(BLUR);
    let gradient = Gradient::of(&image);
    let edges = gradient.thin(EDGE_THRESHOLD);
    let (w, h) = (image.w as f32, image.h as f32);
    let lines = find_lines(&gradient, &edges, MIN_LINE * w.min(h), 80);
    let paint = Paint::of(&image);

    let fraction = |(x, y): (f32, f32)| Point::new((x / w) as f64, (y / h) as f64);
    let segments = lines
        .iter()
        .filter_map(|line| {
            let (a, b) = line.extent()?;
            Some([fraction(point_at(line, a)), fraction(point_at(line, b))])
        })
        .collect();

    let outline = choose(&lines, &paint, w, h).map(|(corners, sides)| WallOutline {
        corners: corners.map(fraction),
        sides,
    });
    WallTrace {
        lines: segments,
        outline,
    }
}

fn point_at(line: &Line, t: f32) -> (f32, f32) {
    (
        line.rho * line.nx - t * line.ny,
        line.rho * line.ny + t * line.nx,
    )
}

type Corners = [(f32, f32); 4];

fn choose(lines: &[Line], paint: &Paint, w: f32, h: f32) -> Option<(Corners, SidesFound)> {
    // A line that hugs the border of the photo is the edge of the picture
    // (a scan's margin, a phone's rounded crop), not of anything in the room.
    let hugs = |a: f32, b: f32, size: f32| {
        let margin = 0.02 * size;
        (a < margin && b < margin) || (a > size - margin && b > size - margin)
    };
    let level = |line: &Line| {
        line.slope_from_horizontal() <= MAX_SLOPE && !hugs(line.y_at(0.0), line.y_at(w), h)
    };
    let upright = |line: &Line| {
        line.lean_from_vertical() <= MAX_LEAN && !hugs(line.x_at(0.0), line.x_at(h), w)
    };
    // `lines` is strongest first, so `take` keeps the best of each group.
    // Each group ends with the photo's border on that side.
    let pick = |keep: &dyn Fn(&Line) -> bool, border: Line| -> Vec<Line> {
        let mut group: Vec<Line> = lines
            .iter()
            .filter(|line| keep(line))
            .take(PER_SIDE)
            .cloned()
            .collect();
        group.push(border);
        group
    };
    let tops = pick(
        &|line| level(line) && line.y_at(w / 2.0) < 0.55 * h,
        Line::bare(0.0, 1.0, 0.0),
    );
    let bottoms = pick(
        &|line| level(line) && line.y_at(w / 2.0) > 0.45 * h,
        Line::bare(0.0, 1.0, h),
    );
    let lefts = pick(
        &|line| upright(line) && line.x_at(h / 2.0) < 0.55 * w,
        Line::bare(1.0, 0.0, 0.0),
    );
    let rights = pick(
        &|line| upright(line) && line.x_at(h / 2.0) > 0.45 * w,
        Line::bare(1.0, 0.0, w),
    );

    // Whether a third line ends where each pair of sides crosses. The same
    // pair turns up in many outlines, so this is worked out once per pair.
    let junctions = |uprights: &[Line], levels: &[Line]| -> Vec<Vec<f32>> {
        uprights
            .iter()
            .map(|side| {
                levels
                    .iter()
                    .map(|cap| match side.intersect(cap) {
                        Some(corner) if is_junction(corner, side, cap, lines) => JUNCTION,
                        _ => 0.0,
                    })
                    .collect()
            })
            .collect()
    };
    let (top_left, top_right) = (junctions(&lefts, &tops), junctions(&rights, &tops));
    let (bottom_left, bottom_right) = (junctions(&lefts, &bottoms), junctions(&rights, &bottoms));

    let mut best: Option<(f32, Corners, [usize; 4])> = None;
    for (t, top) in tops.iter().enumerate() {
        for (b, bottom) in bottoms.iter().enumerate() {
            for (l, left) in lefts.iter().enumerate() {
                for (r, right) in rights.iter().enumerate() {
                    let Some(corners) = outline(top, right, bottom, left, w, h) else {
                        continue;
                    };
                    let [tl, tr, br, bl] = corners;
                    let score = side_score(top, tl, tr, w, h) / w
                        + side_score(bottom, bl, br, w, h) / w
                        + side_score(left, tl, bl, w, h) / h
                        + side_score(right, tr, br, w, h) / h
                        + top_left[l][t]
                        + top_right[r][t]
                        + bottom_left[l][b]
                        + bottom_right[r][b]
                        + FILL * paint.inside(&corners) / (w * h);
                    if best
                        .as_ref()
                        .is_none_or(|(top_score, ..)| score > *top_score)
                    {
                        best = Some((score, corners, [t, r, b, l]));
                    }
                }
            }
        }
    }
    let (_, mut corners, [t, r, b, l]) = best?;
    let (top, right, left) = (&tops[t], &rights[r], &lefts[l]);
    let mut bottom = &bottoms[b];

    // The floor line usually has the top of a baseboard just above it, and
    // the two look alike. If a matching line runs a little below the one
    // chosen, that lower one is the floor. It has to be as sharp an edge,
    // though: a seam between floorboards also runs just below, and is not.
    if !bottom.is_bare() {
        let wall_height = ((corners[3].1 - corners[0].1) + (corners[2].1 - corners[1].1)) / 2.0;
        let mid = (corners[3].0 + corners[2].0) / 2.0;
        let lower = bottoms
            .iter()
            .filter(|other| !other.is_bare())
            .filter(|other| {
                let gap = other.y_at(mid) - bottom.y_at(mid);
                let turn = other.nx * bottom.ny - other.ny * bottom.nx;
                (0.012 * wall_height..=MAX_BASEBOARD * wall_height).contains(&gap)
                    && turn.abs() < 0.07
                    && other.strength >= 0.5 * bottom.strength
                    && other.contrast >= 0.8 * bottom.contrast
            })
            .max_by(|a, b| a.y_at(mid).total_cmp(&b.y_at(mid)));
        if let Some(floor) = lower
            && let Some(moved) = outline(top, right, floor, left, w, h)
        {
            bottom = floor;
            corners = moved;
        }
    }

    let sides = SidesFound {
        top: !top.is_bare(),
        right: !right.is_bare(),
        bottom: !bottom.is_bare(),
        left: !left.is_bare(),
    };
    // With one side or none on a real edge, the outline is just the photo's
    // frame and says nothing about the wall.
    (sides.count() >= 2).then_some((corners, sides))
}

/// Whether some other line ends at the point where two sides cross. Where a
/// wall meets its neighbour, the neighbour's ceiling or floor line runs off
/// from the corner at an angle; the corner of a door casing has no such
/// third line.
fn is_junction(corner: (f32, f32), side: &Line, cap: &Line, lines: &[Line]) -> bool {
    // About 12 degrees: the third line must head somewhere else.
    let differs = |a: &Line, b: &Line| (a.nx * b.ny - a.ny * b.nx).abs() > 0.2;
    lines.iter().any(|line| {
        let distance = line.nx * corner.0 + line.ny * corner.1 - line.rho;
        distance.abs() <= 3.0
            && differs(line, side)
            && differs(line, cap)
            && line.ends_near(line.t_of(corner.0, corner.1))
    })
}

/// The four corners where the side lines cross, if they make a sensible
/// outline: convex, the right way round, and neither tiny nor far out of
/// the photo.
fn outline(
    top: &Line,
    right: &Line,
    bottom: &Line,
    left: &Line,
    w: f32,
    h: f32,
) -> Option<Corners> {
    let corners = [
        left.intersect(top)?,
        top.intersect(right)?,
        right.intersect(bottom)?,
        bottom.intersect(left)?,
    ];
    let [tl, tr, br, bl] = corners;
    let wide = tr.0 - tl.0 >= MIN_SPAN * w && br.0 - bl.0 >= MIN_SPAN * w;
    let tall = bl.1 - tl.1 >= MIN_SPAN * h && br.1 - tr.1 >= MIN_SPAN * h;
    let near = corners
        .iter()
        .all(|&(x, y)| (-0.25 * w..=1.25 * w).contains(&x) && (-0.25 * h..=1.25 * h).contains(&y));
    let convex = (0..4).all(|i| {
        let (a, b, c) = (corners[i], corners[(i + 1) % 4], corners[(i + 2) % 4]);
        (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) > 0.0
    });
    (wide && tall && near && convex).then_some(corners)
}

/// How well one side of an outline sits on an edge, in pixels: the length
/// that does, less a charge for the length that does not and for edge that
/// runs on past the corners.
fn side_score(line: &Line, from: (f32, f32), to: (f32, f32), w: f32, h: f32) -> f32 {
    if line.is_bare() {
        return 0.0;
    }
    let Some((v0, v1)) = visible_range(line, w, h) else {
        return 0.0;
    };
    let (ta, tb) = (line.t_of(from.0, from.1), line.t_of(to.0, to.1));
    let (lo, hi) = (ta.min(tb).max(v0), ta.max(tb).min(v1));
    if hi <= lo {
        return 0.0;
    }
    let on = line.supported(lo, hi);
    let beyond = line.supported(v0, lo) + line.supported(hi, v1);
    on - MISSING * (hi - lo - on) - OVERSHOOT * beyond
}

/// The stretch of a line that is inside the photo, as positions along it.
fn visible_range(line: &Line, w: f32, h: f32) -> Option<(f32, f32)> {
    // The line is `origin + t * direction`; clip it to each pair of borders.
    let (ox, oy) = (line.rho * line.nx, line.rho * line.ny);
    let (dx, dy) = (-line.ny, line.nx);
    let (mut lo, mut hi) = (f32::NEG_INFINITY, f32::INFINITY);
    for (origin, direction, limit) in [(ox, dx, w), (oy, dy, h)] {
        if direction.abs() < 1e-6 {
            if origin < 0.0 || origin > limit {
                return None;
            }
            continue;
        }
        let (a, b) = (-origin / direction, (limit - origin) / direction);
        lo = lo.max(a.min(b));
        hi = hi.min(a.max(b));
    }
    (hi > lo).then_some((lo, hi))
}

/// Which pixels are the colour of the wall.
///
/// The wall is taken to be whatever colour is most common around the middle
/// of the photo. Shading varies a lot across a wall, so brightness is
/// matched loosely and tint more closely.
struct Paint {
    w: usize,
    h: usize,
    /// For each row, a running total across it: plus one for each
    /// wall-coloured pixel, minus [`NOT_WALL`] for each other one.
    running: Vec<f32>,
}

/// Brightness and two tint axes (red against green, blue against yellow).
fn tone(p: [f32; 3]) -> [f32; 3] {
    [
        0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2],
        p[0] - p[1],
        p[2] - 0.5 * (p[0] + p[1]),
    ]
}

impl Paint {
    /// Tint may differ from the wall's by this much, in brightness levels.
    const TINT: f32 = 14.0;
    /// Brightness may be this many times darker or lighter.
    const SHADE: f32 = 1.6;

    fn of(image: &Rgb) -> Paint {
        let (w, h) = (image.w, image.h);
        let wall = Self::wall_tone(image);
        let matches = |p: [f32; 3]| {
            let t = tone(p);
            let tint = ((t[1] - wall[1]).powi(2) + (t[2] - wall[2]).powi(2)).sqrt();
            let shade = (t[0] + 8.0) / (wall[0] + 8.0);
            tint <= Self::TINT && (1.0 / Self::SHADE..=Self::SHADE).contains(&shade)
        };
        let mut running = Vec::with_capacity((w + 1) * h);
        for y in 0..h {
            let mut total = 0.0;
            running.push(total);
            for x in 0..w {
                total += if matches(image.px[y * w + x]) {
                    1.0
                } else {
                    -NOT_WALL
                };
                running.push(total);
            }
        }
        Paint { w, h, running }
    }

    /// The most common colour in the middle of the photo.
    fn wall_tone(image: &Rgb) -> [f32; 3] {
        let (w, h) = (image.w, image.h);
        let middle: Vec<[f32; 3]> = (h / 5..h - h / 5)
            .flat_map(|y| (w / 5..w - w / 5).map(move |x| (x, y)))
            .map(|(x, y)| tone(image.px[y * w + x]))
            .collect();
        if middle.is_empty() {
            return [128.0, 0.0, 0.0];
        }

        // Coarse histogram: 16 levels of brightness, 16 of each tint axis.
        const BINS: usize = 16;
        let bin = |t: [f32; 3]| {
            let level = |value: f32, lo: f32, hi: f32| {
                (((value - lo) / (hi - lo) * BINS as f32) as usize).min(BINS - 1)
            };
            (level(t[0], 0.0, 256.0) * BINS + level(t[1], -128.0, 128.0)) * BINS
                + level(t[2], -128.0, 128.0)
        };
        let mut counts = vec![0u32; BINS * BINS * BINS];
        for &t in &middle {
            counts[bin(t)] += 1;
        }
        let fullest = (0..counts.len())
            .max_by_key(|&index| counts[index])
            .unwrap_or(0);
        let mut centre = mean(middle.iter().copied().filter(|&t| bin(t) == fullest));

        // Settle on the centre of the cluster around that bin.
        for _ in 0..3 {
            let near = middle.iter().copied().filter(|t| {
                (t[0] - centre[0]).abs() < 24.0
                    && (t[1] - centre[1]).abs() < 10.0
                    && (t[2] - centre[2]).abs() < 10.0
            });
            centre = mean(near);
        }
        centre
    }

    /// The balance of wall-coloured pixels inside an outline.
    fn inside(&self, corners: &Corners) -> f32 {
        let mut total = 0.0;
        for row in 0..self.h {
            let y = row as f32 + 0.5;
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                if (a.1 <= y) != (b.1 <= y) {
                    let x = a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0);
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            if hi <= lo {
                continue;
            }
            let column = |x: f32| x.round().clamp(0.0, self.w as f32) as usize;
            let base = row * (self.w + 1);
            total += self.running[base + column(hi)] - self.running[base + column(lo)];
        }
        total
    }
}

fn mean(tones: impl Iterator<Item = [f32; 3]>) -> [f32; 3] {
    let (mut sum, mut count) = ([0.0f32; 3], 0.0f32);
    for t in tones {
        sum[0] += t[0];
        sum[1] += t[1];
        sum[2] += t[2];
        count += 1.0;
    }
    if count == 0.0 {
        return [128.0, 0.0, 0.0];
    }
    sum.map(|value| value / count)
}
