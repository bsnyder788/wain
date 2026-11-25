//! Finds doors, windows, outlets and switches on a wall whose corners are
//! known.
//!
//! The photo is first redrawn as if the wall were seen square-on, at a known
//! number of millimetres per pixel. In that view everything of interest is
//! an upright rectangle of a predictable size, which is what makes simple
//! image processing good enough:
//!
//! - Doors and windows are rectangles whose sides lie on long straight
//!   edges, with plain wall just outside them and something that is not
//!   plain wall inside.
//! - Outlets and switches are small isolated blobs the size of a cover
//!   plate, told apart by how high they sit.
//!
//! Every test compares a patch of the wall with what is next to it, never
//! with one colour for the whole wall, because light falls off across a
//! room and a wall is rarely one brightness from end to end.
//!
//! Anything rectangular fits the description, so a framed picture can come
//! back as a window. The results are suggestions for the user to confirm.

use wain_core::{Homography, Length, Obstruction, ObstructionKind, Point, Rect};

use crate::Photo;
use crate::raster::{Gradient, Rgb};

/// The longer side of the squared-up view, in pixels.
const VIEW_SIDE: f64 = 720.0;
/// Never finer than this many millimetres per pixel; a photo has no more
/// detail to give.
const FINEST_MM: f64 = 2.0;
/// Weakest edge that counts, in brightness levels per pixel. Raised for a
/// wall with a rough texture; see [`edge_threshold`].
const EDGE: std::ops::RangeInclusive<f32> = 4.0..=14.0;
/// Two patches of the same wall can differ this much in any colour channel
/// through shading alone.
const SAME_WALL: f32 = 45.0;
/// Share of the last fifth of each side, at each corner, that must lie on
/// an edge.
const CORNER: f32 = 0.4;
/// Sides are matched to edges within this many pixels, since the corners
/// the view was squared up from are never quite exact.
const SLACK: usize = 2;

/// Sizes below are in millimetres on the wall.
mod mm {
    /// A straight edge must run this far to be the side of a door or window.
    pub const MIN_SIDE: f64 = 300.0;
    /// Breaks in an edge shorter than this are ignored.
    pub const GAP: f64 = 40.0;
    pub const MIN_OPENING: f64 = 350.0;
    pub const MAX_OPENING_WIDTH: f64 = 2600.0;
    /// A rectangle that starts this close to the floor stands on it.
    pub const FLOOR_REACH: f64 = 200.0;
    pub const MIN_DOOR_HEIGHT: f64 = 1700.0;
    pub const MIN_DOOR_WIDTH: f64 = 550.0;
    pub const MAX_DOOR_WIDTH: f64 = 2000.0;
    /// Something that reaches this close to the ceiling is part of the
    /// room, not a door.
    pub const CEILING_REACH: f64 = 120.0;
    /// The band just outside a rectangle that must be plain wall.
    pub const CLEAR_BAND: f64 = 50.0;
    pub const CLEAR_OFFSET: f64 = 15.0;
    /// A door or window this close to the end of the wall is taken to be
    /// in the room's corner.
    pub const WALL_END: f64 = 250.0;

    /// Cover plates, from a single one to three in a row: US, European
    /// and British, with room for the wall's size being a little out.
    pub const PLATE_WIDTH: std::ops::RangeInclusive<f64> = 50.0..=210.0;
    pub const PLATE_HEIGHT: std::ops::RangeInclusive<f64> = 55.0..=160.0;
    /// Outlets sit low on the wall and switches at hand height.
    pub const OUTLET_CENTRE: std::ops::RangeInclusive<f64> = 150.0..=750.0;
    pub const SWITCH_CENTRE: std::ops::RangeInclusive<f64> = 900.0..=1500.0;
    pub const PLATE_RING: f64 = 35.0;
}

/// A rectangle in the squared-up view, in pixels. `x1` and `y1` are one
/// past the last column and row.
#[derive(Clone, Copy, Debug)]
struct Box {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Box {
    fn area(&self) -> f32 {
        ((self.x1 - self.x0) * (self.y1 - self.y0)) as f32
    }

    fn overlap(&self, other: &Box) -> f32 {
        let w = self.x1.min(other.x1).saturating_sub(self.x0.max(other.x0));
        let h = self.y1.min(other.y1).saturating_sub(self.y0.max(other.y0));
        (w * h) as f32
    }
}

/// The wall seen square-on.
struct View {
    image: Rgb,
    /// False where the wall is out of shot.
    seen: Vec<bool>,
    /// Millimetres of wall per pixel.
    scale: f64,
}

pub(crate) fn find(
    photo: &Photo,
    wall_width: Length,
    wall_height: Length,
    corners: &[Point; 4],
) -> Vec<Obstruction> {
    let Some(view) = square_up(photo, wall_width, wall_height, corners) else {
        return Vec::new();
    };
    let (w, h) = (view.image.w, view.image.h);
    if w < 16 || h < 16 {
        return Vec::new();
    }
    let gradient = Gradient::of(&view.image);
    let threshold = edge_threshold(&view, &gradient);
    // Only the crest of each edge, so that a strong edge is one pixel wide
    // and the wall beside it counts as plain.
    let mut edges = gradient.thin(threshold);
    for (edge, &seen) in edges.iter_mut().zip(&view.seen) {
        *edge &= seen;
    }
    let scene = Scene {
        view: &view,
        gradient: &gradient,
        threshold,
        busy: Sums::of(w, h, &edges),
        edges: &edges,
        colour: Means::of(&view.image),
    };
    let openings = find_openings(&scene);

    let to_wall = |b: &Box, kind: ObstructionKind| {
        let nm = |pixels: f64| Length::from_nm((pixels * view.scale * 1e6).round() as i64);
        let x = nm(b.x0 as f64).min(wall_width);
        let y = nm((h - b.y1) as f64).min(wall_height);
        Obstruction {
            kind,
            rect: Rect::new(
                x,
                y,
                nm((b.x1 - b.x0) as f64).min(wall_width - x),
                nm((b.y1 - b.y0) as f64).min(wall_height - y),
            ),
        }
    };

    let mut found: Vec<Obstruction> = openings
        .iter()
        .map(|(opening, kind)| to_wall(opening, *kind))
        .collect();
    let openings: Vec<Box> = openings.iter().map(|(opening, _)| *opening).collect();
    let plates = find_plates(&scene, &openings);
    for plate in &plates {
        let centre = (h as f64 - (plate.y0 + plate.y1) as f64 / 2.0) * view.scale;
        if mm::OUTLET_CENTRE.contains(&centre) {
            found.push(to_wall(plate, ObstructionKind::Outlet));
        } else if mm::SWITCH_CENTRE.contains(&centre) {
            found.push(to_wall(plate, ObstructionKind::Switch));
        }
    }
    found.retain(|o| o.rect.width.is_positive() && o.rect.height.is_positive());
    found.sort_by_key(|o| (o.rect.x, o.rect.y));
    found
}

/// Redraws the wall as seen square-on.
fn square_up(
    photo: &Photo,
    wall_width: Length,
    wall_height: Length,
    corners: &[Point; 4],
) -> Option<View> {
    let in_pixels =
        corners.map(|p| Point::new(p.x * photo.width as f64, p.y * photo.height as f64));
    let homography = Homography::for_wall(wall_width, wall_height, &in_pixels).ok()?;
    let m = homography.forward;

    let (width_mm, height_mm) = (wall_width.as_mm(), wall_height.as_mm());
    let scale = (width_mm.max(height_mm) / VIEW_SIDE).max(FINEST_MM);
    let w = (width_mm / scale).ceil() as usize;
    let h = (height_mm / scale).ceil() as usize;

    let mut px = Vec::with_capacity(w * h);
    let mut seen = Vec::with_capacity(w * h);
    for j in 0..h {
        // The wall's origin is at the floor; the view's is at the ceiling.
        let y = (height_mm - (j as f64 + 0.5) * scale) * 1e6;
        for i in 0..w {
            let x = (i as f64 + 0.5) * scale * 1e6;
            let d = m[6] * x + m[7] * y + m[8];
            let u = (m[0] * x + m[1] * y + m[2]) / d - 0.5;
            let v = (m[3] * x + m[4] * y + m[5]) / d - 0.5;
            match sample(photo, u, v) {
                Some(colour) => {
                    px.push(colour);
                    seen.push(true);
                }
                None => {
                    px.push([0.0; 3]);
                    seen.push(false);
                }
            }
        }
    }
    // A wall partly out of shot leaves holes; fill them with the nearest
    // colour seen on the same row so that they do not read as edges.
    for j in 0..h {
        let row = j * w;
        let mut last: Option<[f32; 3]> = None;
        for i in (0..w).chain((0..w).rev()) {
            if seen[row + i] {
                last = Some(px[row + i]);
            } else if let Some(colour) = last
                && px[row + i] == [0.0; 3]
            {
                px[row + i] = colour;
            }
        }
    }
    let image = Rgb { w, h, px }.blurred(0.8);
    Some(View { image, seen, scale })
}

/// The photo's colour at a position between pixels, or `None` outside it.
fn sample(photo: &Photo, u: f64, v: f64) -> Option<[f32; 3]> {
    if !(u >= 0.0 && v >= 0.0 && u <= (photo.width - 1) as f64 && v <= (photo.height - 1) as f64) {
        return None;
    }
    let (x0, y0) = (u.floor() as usize, v.floor() as usize);
    let (x1, y1) = (
        (x0 + 1).min(photo.width - 1),
        (y0 + 1).min(photo.height - 1),
    );
    let (fx, fy) = ((u - x0 as f64) as f32, (v - y0 as f64) as f32);
    let at = |x: usize, y: usize| {
        let index = (y * photo.width + x) * 4;
        [
            photo.rgba[index] as f32,
            photo.rgba[index + 1] as f32,
            photo.rgba[index + 2] as f32,
        ]
    };
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let mut out = [0.0; 3];
    for channel in 0..3 {
        let top = a[channel] + (b[channel] - a[channel]) * fx;
        let bottom = c[channel] + (d[channel] - c[channel]) * fx;
        out[channel] = top + (bottom - top) * fy;
    }
    Some(out)
}

/// Everything the two searches share.
struct Scene<'a> {
    view: &'a View,
    gradient: &'a Gradient,
    /// Weakest edge that counts on this wall.
    threshold: f32,
    /// The crest of every edge, of any direction, and counts of them.
    edges: &'a [bool],
    busy: Sums,
    colour: Means,
}

/// The weakest edge worth counting on this wall. A smooth painted wall is
/// nearly flat between its features, but orange-peel texture or woodchip
/// paper is covered in faint edges, so the bar is set above whatever the
/// typical pixel shows.
fn edge_threshold(view: &View, gradient: &Gradient) -> f32 {
    let mut strengths: Vec<f32> = gradient
        .mag
        .iter()
        .zip(&view.seen)
        .filter(|&(_, &seen)| seen)
        .map(|(&strength, _)| strength)
        .collect();
    if strengths.is_empty() {
        return *EDGE.start();
    }
    let middle = strengths.len() / 2;
    strengths.select_nth_unstable_by(middle, f32::total_cmp);
    (strengths[middle] * 4.0).clamp(*EDGE.start(), *EDGE.end())
}

/// Running totals of each colour channel, for the average colour of any
/// rectangle in constant time.
struct Means {
    w: usize,
    total: Vec<[f64; 3]>,
}

impl Means {
    fn of(image: &Rgb) -> Means {
        let (w, h) = (image.w, image.h);
        let mut total = vec![[0.0f64; 3]; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = [0.0f64; 3];
            for x in 0..w {
                let p = image.px[y * w + x];
                let above = total[y * (w + 1) + x + 1];
                for channel in 0..3 {
                    row[channel] += p[channel] as f64;
                    total[(y + 1) * (w + 1) + x + 1][channel] = above[channel] + row[channel];
                }
            }
        }
        Means { w, total }
    }

    /// Average colour of the box, or `None` if it is empty.
    fn mean(&self, b: &Box) -> Option<[f32; 3]> {
        if b.x1 <= b.x0 || b.y1 <= b.y0 {
            return None;
        }
        let at = |x: usize, y: usize| self.total[y * (self.w + 1) + x];
        let (a, d, c, e) = (
            at(b.x1, b.y1),
            at(b.x0, b.y0),
            at(b.x1, b.y0),
            at(b.x0, b.y1),
        );
        let area = b.area() as f64;
        Some(std::array::from_fn(|channel| {
            ((a[channel] + d[channel] - c[channel] - e[channel]) / area) as f32
        }))
    }
}

/// How far apart two colours are: the largest difference in any channel.
fn colour_gap(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3)
        .map(|channel| (a[channel] - b[channel]).abs())
        .fold(0.0, f32::max)
}

/// Running totals over a mask, for counting set pixels in any rectangle in
/// constant time.
struct Sums {
    w: usize,
    total: Vec<u32>,
}

impl Sums {
    fn of(w: usize, h: usize, mask: &[bool]) -> Sums {
        let mut total = vec![0u32; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = 0;
            for x in 0..w {
                row += mask[y * w + x] as u32;
                total[(y + 1) * (w + 1) + x + 1] = total[y * (w + 1) + x + 1] + row;
            }
        }
        Sums { w, total }
    }

    fn count(&self, b: &Box) -> u32 {
        let at = |x: usize, y: usize| self.total[y * (self.w + 1) + x];
        at(b.x1, b.y1) + at(b.x0, b.y0) - at(b.x1, b.y0) - at(b.x0, b.y1)
    }

    /// Share of the box's pixels that are set; zero for an empty box.
    fn share(&self, b: &Box) -> f32 {
        if b.x1 <= b.x0 || b.y1 <= b.y0 {
            return 0.0;
        }
        self.count(b) as f32 / b.area()
    }
}

/// A straight run of edge along one column or row: its position across,
/// and where it starts and stops along.
#[derive(Clone, Copy, Debug)]
struct Run {
    at: usize,
    from: usize,
    to: usize,
}

impl Run {
    fn len(&self) -> usize {
        self.to - self.from
    }
}

/// Finds the rectangles that look like doors and windows.
fn find_openings(scene: &Scene) -> Vec<(Box, ObstructionKind)> {
    let (view, gradient) = (scene.view, scene.gradient);
    let (w, h) = (view.image.w, view.image.h);
    let px = |millimetres: f64| (millimetres / view.scale).round().max(1.0) as usize;

    // Upright edges change brightness going across; level ones going down.
    let mut upright = vec![false; w * h];
    let mut level = vec![false; w * h];
    for index in 0..w * h {
        let (gx, gy) = (gradient.gx[index].abs(), gradient.gy[index].abs());
        upright[index] = scene.edges[index] && gx >= 1.5 * gy;
        level[index] = scene.edges[index] && gy >= 1.5 * gx;
    }
    // Widen each edge a little either side, so a line that is slightly off
    // straight still reads as one run.
    let widen = |mask: &[bool], across_x: bool| -> Vec<bool> {
        let mut out = mask.to_vec();
        for y in 0..h {
            for x in 0..w {
                if !mask[y * w + x] {
                    continue;
                }
                if across_x {
                    for near in x.saturating_sub(SLACK)..=(x + SLACK).min(w - 1) {
                        out[y * w + near] = true;
                    }
                } else {
                    for near in y.saturating_sub(SLACK)..=(y + SLACK).min(h - 1) {
                        out[near * w + x] = true;
                    }
                }
            }
        }
        out
    };
    let upright = widen(&upright, true);
    let level = widen(&level, false);
    let upright_sums = Sums::of(w, h, &upright);
    let level_sums = Sums::of(w, h, &level);

    let min_side = px(mm::MIN_SIDE);
    let gap = px(mm::GAP);
    let uprights = thin_runs(
        (0..w)
            .flat_map(|x| runs((0..h).map(|y| upright[y * w + x]), gap, min_side, x))
            .collect(),
    );
    let levels = thin_runs(
        (0..h)
            .flat_map(|y| runs((0..w).map(|x| level[y * w + x]), gap, min_side, y))
            .collect(),
    );

    // Share of a side that lies on an edge.
    let side_upright = |x: usize, y0: usize, y1: usize| {
        upright_sums.share(&Box {
            x0: x,
            x1: x + 1,
            y0,
            y1,
        })
    };
    let side_level = |y: usize, x0: usize, x1: usize| {
        level_sums.share(&Box {
            x0,
            x1,
            y0: y,
            y1: y + 1,
        })
    };

    let band = px(mm::CLEAR_BAND);
    let wall_end = px(mm::WALL_END);
    let offset = px(mm::CLEAR_OFFSET);
    let floor_reach = px(mm::FLOOR_REACH);
    let min_opening = px(mm::MIN_OPENING);
    let max_width = px(mm::MAX_OPENING_WIDTH);

    let door_width = px(mm::MIN_DOOR_WIDTH)..=px(mm::MAX_DOOR_WIDTH);
    let (door_height, ceiling_reach) = (px(mm::MIN_DOOR_HEIGHT), px(mm::CEILING_REACH));
    let mut candidates: Vec<(f32, Box, ObstructionKind)> = Vec::new();
    for (index, left) in uprights.iter().enumerate() {
        for right in &uprights[index + 1..] {
            let (left, right) = if left.at <= right.at {
                (left, right)
            } else {
                (right, left)
            };
            let width = right.at - left.at;
            if width < min_opening || width > max_width {
                continue;
            }
            // Level edges across a good part of the gap are possible tops
            // and bottoms. (A pair of windows side by side has a top in two
            // pieces.) The floor is always a possible bottom.
            let mut caps: Vec<usize> = levels
                .iter()
                .filter(|run| {
                    let covered = run.to.min(right.at).saturating_sub(run.from.max(left.at));
                    covered * 10 >= width * 3
                })
                .map(|run| run.at)
                .collect();
            caps.sort_unstable();
            caps.dedup();
            caps.push(h);
            for (cap_index, &top) in caps.iter().enumerate() {
                for &bottom in &caps[cap_index + 1..] {
                    let (top, bottom) = if top <= bottom {
                        (top, bottom)
                    } else {
                        (bottom, top)
                    };
                    if bottom - top < min_opening || top >= h {
                        continue;
                    }
                    // Something standing on the floor is a door if it is
                    // the size of one and stops short of the ceiling. If
                    // not, it is furniture or a part of the room.
                    let on_floor = h - bottom <= floor_reach;
                    let kind = if !on_floor {
                        ObstructionKind::Window
                    } else if door_width.contains(&width)
                        && h - top >= door_height
                        && top > ceiling_reach
                    {
                        ObstructionKind::Door
                    } else {
                        continue;
                    };
                    let rect = Box {
                        x0: left.at,
                        x1: right.at + 1,
                        y0: top,
                        y1: if on_floor { h } else { bottom + 1 },
                    };
                    let sides = [
                        side_upright(left.at, top, bottom.min(h)),
                        side_upright(right.at, top, bottom.min(h)),
                        side_level(top, left.at, right.at),
                    ];
                    let base = if on_floor {
                        1.0
                    } else {
                        side_level(bottom, left.at, right.at)
                    };
                    if sides.iter().any(|&share| share < 0.6) || base < 0.6 {
                        continue;
                    }
                    // The sides must reach the corners. Without this, the
                    // side of a window stretched up to the ceiling line
                    // still looks mostly covered.
                    let reach = |length: usize| (length / 5).max(4);
                    let (tall, wide) = (reach(bottom.min(h) - top), reach(width));
                    let met = |share: f32| share >= CORNER;
                    let meets_top = [left.at, right.at]
                        .iter()
                        .all(|&x| met(side_upright(x, top, top + tall)))
                        && met(side_level(top, left.at, left.at + wide))
                        && met(side_level(top, right.at - wide, right.at));
                    // A door's casing runs down to the floor, give or take
                    // the baseboard, where a window's sides stop at its
                    // sill. One side is enough: the other is often lost
                    // against the baseboard or behind something.
                    let meets_bottom = if on_floor {
                        let low = h.saturating_sub(floor_reach);
                        [left.at, right.at]
                            .iter()
                            .any(|&x| met(side_upright(x, low.saturating_sub(tall), low)))
                    } else {
                        [left.at, right.at]
                            .iter()
                            .all(|&x| met(side_upright(x, bottom - tall, bottom)))
                            && met(side_level(bottom, left.at, left.at + wide))
                            && met(side_level(bottom, right.at - wide, right.at))
                    };
                    if !(meets_top && meets_bottom) {
                        continue;
                    }
                    let outline = (sides.iter().sum::<f32>() + base) / 4.0;

                    // Plain wall just outside, on every side but the floor:
                    // a band with next to no edges in it.
                    // A door or window hard against the end of the wall has
                    // the room's corner beside it instead, so that side is
                    // not held to this.
                    let mut bands = Vec::with_capacity(4);
                    if rect.x0 > wall_end {
                        bands.push(Box {
                            x0: rect.x0.saturating_sub(offset + band),
                            x1: rect.x0.saturating_sub(offset),
                            ..rect
                        });
                    }
                    if rect.x1 + wall_end < w {
                        bands.push(Box {
                            x0: rect.x1 + offset,
                            x1: (rect.x1 + offset + band).min(w),
                            ..rect
                        });
                    }
                    bands.push(Box {
                        y0: rect.y0.saturating_sub(offset + band),
                        y1: rect.y0.saturating_sub(offset),
                        ..rect
                    });
                    if !on_floor {
                        bands.push(Box {
                            y0: (rect.y1 + offset).min(h),
                            y1: (rect.y1 + offset + band).min(h),
                            ..rect
                        });
                    }
                    let clutter = bands
                        .iter()
                        .map(|band| scene.busy.share(band))
                        .fold(0.0, f32::max);
                    if clutter > 0.12 {
                        continue;
                    }
                    // And the same wall on every side. One sash of a window
                    // has plain sky beyond its meeting rail, which is
                    // featureless but nothing like the wall.
                    let around: Vec<[f32; 3]> = bands
                        .iter()
                        .filter_map(|band| scene.colour.mean(band))
                        .collect();
                    let uneven = around
                        .iter()
                        .flat_map(|&a| around.iter().map(move |&b| colour_gap(a, b)))
                        .fold(0.0, f32::max);
                    if uneven > SAME_WALL {
                        continue;
                    }

                    // Something other than wall inside: a different colour
                    // from the bands around it, or plenty of detail. A door
                    // painted to match the wall has little of either, so a
                    // crisp outline will do instead.
                    let inset = (rect.x1 - rect.x0).min(rect.y1 - rect.y0) / 8;
                    let inner = Box {
                        x0: rect.x0 + inset,
                        x1: rect.x1 - inset,
                        y0: rect.y0 + inset,
                        y1: rect.y1 - inset,
                    };
                    let differs = match scene.colour.mean(&inner) {
                        Some(inside) if !around.is_empty() => around
                            .iter()
                            .map(|&outside| colour_gap(inside, outside))
                            .fold(f32::INFINITY, f32::min),
                        _ => 0.0,
                    };
                    let filled = (differs / 30.0)
                        .max(scene.busy.share(&inner) / 0.15)
                        .min(1.0);
                    if filled < 0.3 && outline < 0.85 {
                        continue;
                    }
                    candidates.push((outline + 0.3 * filled - clutter, rect, kind));
                }
            }
        }
    }

    // Best first; drop anything that overlaps one already kept.
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut kept: Vec<(f32, Box, ObstructionKind)> = Vec::new();
    for (score, rect, kind) in candidates {
        let clash = kept
            .iter()
            .position(|(_, other, _)| rect.overlap(other) > 0.3 * rect.area().min(other.area()));
        match clash {
            None => kept.push((score, rect, kind)),
            // A casing has an edge either side of it, so a door gives two
            // nested rectangles that score about the same. The outer one is
            // what trim has to stop at.
            Some(index) => {
                let (other_score, other, _) = kept[index];
                let wraps = rect.overlap(&other) >= 0.95 * other.area()
                    && rect.area() <= 1.4 * other.area();
                if wraps && score >= other_score - 0.08 {
                    kept[index] = (other_score, rect, kind);
                }
            }
        }
    }
    kept.into_iter()
        .map(|(_, rect, kind)| (rect, kind))
        .collect()
}

/// Runs of set cells at least `min` long, bridging breaks of up to `gap`.
fn runs(cells: impl Iterator<Item = bool>, gap: usize, min: usize, at: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut current: Option<(usize, usize)> = None;
    for (index, on) in cells.enumerate() {
        if !on {
            continue;
        }
        current = match current {
            Some((from, to)) if index - to <= gap => Some((from, index + 1)),
            Some((from, to)) => {
                if to - from >= min {
                    out.push(Run { at, from, to });
                }
                Some((index, index + 1))
            }
            None => Some((index, index + 1)),
        };
    }
    if let Some((from, to)) = current
        && to - from >= min
    {
        out.push(Run { at, from, to });
    }
    out
}

/// An edge a few pixels thick gives a run in each of several neighbouring
/// columns. Keeps the longest of each such cluster.
fn thin_runs(mut all: Vec<Run>) -> Vec<Run> {
    all.sort_by_key(|run| std::cmp::Reverse(run.len()));
    let mut kept: Vec<Run> = Vec::new();
    for run in all {
        let shadowed = kept.iter().any(|other| {
            run.at.abs_diff(other.at) <= 2 * SLACK + 1 && run.from < other.to && other.from < run.to
        });
        if !shadowed {
            kept.push(run);
        }
    }
    // Enough for any real wall, and it bounds the search.
    kept.truncate(60);
    kept
}

/// Finds small isolated blobs the size of a cover plate.
fn find_plates(scene: &Scene, openings: &[Box]) -> Vec<Box> {
    let (view, gradient) = (scene.view, scene.gradient);
    let (w, h) = (view.image.w, view.image.h);
    let px = |millimetres: f64| millimetres / view.scale;

    // A plate shows up as a knot of edges: its outline, and the slots or
    // rockers inside it. The full width of each edge is used here, not just
    // its crest, so that a small plate reads as one solid blob.
    let mask: Vec<bool> = (0..w * h)
        .map(|index| view.seen[index] && gradient.mag[index] >= scene.threshold)
        .collect();
    let busy = Sums::of(w, h, &mask);

    let ring = px(mm::PLATE_RING).round().max(2.0) as usize;
    let mut plates = Vec::new();
    let mut visited = vec![false; w * h];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !mask[start] || visited[start] {
            continue;
        }
        // Flood fill one blob, tracking its bounds and size.
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        let mut size = 0usize;
        visited[start] = true;
        stack.push(start);
        while let Some(index) = stack.pop() {
            let (x, y) = (index % w, index / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            size += 1;
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let next = ny * w + nx;
                    if mask[next] && !visited[next] {
                        visited[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        let blob = Box { x0, y0, x1, y1 };
        let (width, height) = ((x1 - x0) as f64 * view.scale, (y1 - y0) as f64 * view.scale);
        if !mm::PLATE_WIDTH.contains(&width) || !mm::PLATE_HEIGHT.contains(&height) {
            continue;
        }
        if (size as f32) < 0.45 * blob.area() {
            continue;
        }
        // Nothing else close by: a plate sits alone on bare wall.
        let around = Box {
            x0: x0.saturating_sub(ring),
            y0: y0.saturating_sub(ring),
            x1: (x1 + ring).min(w),
            y1: (y1 + ring).min(h),
        };
        let nearby = busy.count(&around) - busy.count(&blob);
        if nearby as f32 > 0.1 * (around.area() - blob.area()) {
            continue;
        }
        if openings.iter().any(|opening| blob.overlap(opening) > 0.0) {
            continue;
        }
        plates.push(blob);
    }
    plates
}
