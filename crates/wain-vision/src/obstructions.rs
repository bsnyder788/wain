//! Finds doors, windows, outlets and switches on a wall whose corners are
//! known.
//!
//! The photo is first redrawn as if the wall were seen square-on, at a known
//! number of millimetres per pixel. In that view everything of interest is
//! a rectangle of a predictable size, which is what makes simple image
//! processing good enough:
//!
//! - Doors and windows are rectangles whose sides lie on long straight
//!   edges that meet at the corners, with plain wall just outside them and
//!   something that is not plain wall inside. The edges are found as fitted
//!   lines, so they may be faint, broken or a few degrees off true.
//! - Outlets and switches are small isolated blobs the size of a cover
//!   plate, told apart by how high they sit.
//!
//! Every test compares a patch of the wall with what is next to it, never
//! with one colour for the whole wall, because light falls off across a
//! room and a wall is rarely one brightness from end to end.
//!
//! Anything rectangular fits the description, so a framed picture can come
//! back as a window. The results are suggestions for the user to confirm.
//! `examples/evaluate.rs` scores them against marked-up photos; run it
//! after any change here.

use wain_core::{Homography, Length, Obstruction, ObstructionKind, Point, Rect};

use crate::Photo;
use crate::lines::{Line, Search, find_lines};
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
/// Share of a plate's bounding box that lies between the first and last
/// edge on its row or in its column.
const SOLID: f32 = 0.7;
/// The sides of a door or window can be this many degrees off upright or
/// level in the squared-up view.
const SLANT: f32 = 4.0;
/// A long straight edge counts as a side when it is this fraction as strong
/// as the weakest edge that counts on its own.
const FAINT: f32 = 0.6;

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
    /// An edge that runs on this far past both corners of a rectangle is
    /// not that rectangle's own side.
    pub const THROUGH: f64 = 150.0;
    /// The band just outside a rectangle that must be plain wall.
    pub const CLEAR_BAND: f64 = 50.0;
    pub const CLEAR_OFFSET: f64 = 15.0;
    /// A door or window this close to the end of the wall is taken to be
    /// in the room's corner.
    pub const WALL_END: f64 = 250.0;

    /// Cover plates, from a single one to three in a row: US, European
    /// and British, with room for the wall's size being a little out.
    pub const PLATE_WIDTH: std::ops::RangeInclusive<f64> = 45.0..=210.0;
    pub const PLATE_HEIGHT: std::ops::RangeInclusive<f64> = 55.0..=180.0;
    /// Outlets sit low on the wall. Switches are at hand height, and
    /// thermostats and the like a little above that.
    pub const OUTLET_CENTRE: std::ops::Range<f64> = 100.0..800.0;
    pub const SWITCH_CENTRE: std::ops::RangeInclusive<f64> = 800.0..=1650.0;
    pub const PLATE_RING: f64 = 35.0;
    /// An edge shorter than this is a speck of texture, not part of a plate.
    pub const SPECK: f64 = 12.0;
    /// Pieces of a plate's outline this far apart are joined up.
    pub const PLATE_JOIN: f64 = 8.0;
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
    /// Counts of the pixels on the crest of an edge, of any direction.
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

/// A level line and where it crosses the two uprights being tried, left
/// and right.
type Cap<'a> = (&'a Line, (f32, f32), (f32, f32));

/// Share of a line between two points on it that lies on an edge.
fn share(line: &Line, from: (f32, f32), to: (f32, f32)) -> f32 {
    let (a, b) = (line.t_of(from.0, from.1), line.t_of(to.0, to.1));
    let length = (b - a).abs();
    if length < 1.0 {
        return 0.0;
    }
    (line.supported(a, b) / length).min(1.0)
}

/// As [`share`], for the first `length` pixels going from one point toward
/// another. A negative length looks the other way: at what lies on the line
/// before `from`.
fn share_from(line: &Line, from: (f32, f32), toward: (f32, f32), length: f32) -> f32 {
    let (a, b) = (line.t_of(from.0, from.1), line.t_of(toward.0, toward.1));
    let end = a + (b - a).signum() * length;
    (line.supported(a, end) / length.abs()).min(1.0)
}

/// Finds the rectangles that look like doors and windows.
///
/// Their sides are looked for among the long straight edges in the view,
/// found the same way the wall's own outline is. Those need not be exactly
/// upright or level: the corners the view was squared up from are never
/// quite right, and neither are old houses.
fn find_openings(scene: &Scene) -> Vec<(Box, ObstructionKind)> {
    let (view, gradient) = (scene.view, scene.gradient);
    let (w, h) = (view.image.w, view.image.h);
    let px = |millimetres: f64| (millimetres / view.scale).max(1.0) as f32;

    // A long edge can be fainter than a short one and still be believed,
    // which is what finds a white casing on a white wall.
    let mut faint = gradient.thin(scene.threshold * FAINT);
    for (edge, &seen) in faint.iter_mut().zip(&view.seen) {
        *edge &= seen;
    }
    let search = Search {
        min_votes: px(mm::MIN_SIDE) * 0.8,
        limit: 120,
        bridge: px(mm::GAP) as usize,
        slant: Some(SLANT),
    };
    let lines = find_lines(gradient, &faint, &search);
    let middle = (w as f32 / 2.0, h as f32 / 2.0);
    let mut uprights: Vec<&Line> = lines
        .iter()
        .filter(|line| line.lean_from_vertical() <= SLANT + 1.0)
        .collect();
    uprights.sort_by(|a, b| a.x_at(middle.1).total_cmp(&b.x_at(middle.1)));
    let mut levels: Vec<&Line> = lines
        .iter()
        .filter(|line| line.slope_from_horizontal() <= SLANT + 1.0)
        .collect();
    levels.sort_by(|a, b| a.y_at(middle.0).total_cmp(&b.y_at(middle.0)));
    // The floor is always a possible bottom.
    let floor = Line::bare(0.0, 1.0, h as f32);
    levels.push(&floor);

    let band = px(mm::CLEAR_BAND) as usize;
    let wall_end = px(mm::WALL_END) as usize;
    let offset = px(mm::CLEAR_OFFSET) as usize;
    let floor_reach = px(mm::FLOOR_REACH);
    let min_opening = px(mm::MIN_OPENING);
    let max_width = px(mm::MAX_OPENING_WIDTH);
    let through = px(mm::THROUGH);
    let door_width = px(mm::MIN_DOOR_WIDTH)..=px(mm::MAX_DOOR_WIDTH);
    let door_height = px(mm::MIN_DOOR_HEIGHT);
    let met = |share: f32| share >= CORNER;

    let mut candidates: Vec<(f32, Box, ObstructionKind)> = Vec::new();
    for (index, left) in uprights.iter().enumerate() {
        for right in &uprights[index + 1..] {
            let width = right.x_at(middle.1) - left.x_at(middle.1);
            if width < min_opening || width > max_width {
                continue;
            }
            let (Some(floor_left), Some(floor_right)) =
                (left.intersect(&floor), right.intersect(&floor))
            else {
                continue;
            };
            // Level edges across a good part of the gap are possible tops
            // and bottoms. (A pair of windows side by side has a top in two
            // pieces.)
            let caps: Vec<Cap> = levels
                .iter()
                .filter_map(|&level| {
                    let (a, b) = (left.intersect(level)?, right.intersect(level)?);
                    (level.is_bare() || share(level, a, b) >= 0.3).then_some((level, a, b))
                })
                .collect();
            for (cap_index, &(top, tl, tr)) in caps.iter().enumerate() {
                if top.is_bare() {
                    continue;
                }
                let top_y = (tl.1 + tr.1) / 2.0;
                let top_share = share(top, tl, tr);
                // A line that carries on well past both corners is not the
                // top of this: it is a ceiling line, a picture rail or the
                // top of a tiled dado running through. At the end of the
                // wall there is no telling, so the worst is assumed.
                let at_wall_end = |x: f32| x <= wall_end as f32 || x >= (w - wall_end) as f32;
                let runs_on = |line: &Line, a: (f32, f32), b: (f32, f32)| {
                    let past = |from: (f32, f32), toward: (f32, f32)| {
                        at_wall_end(from.0) || share_from(line, from, toward, -through) >= 0.7
                    };
                    past(a, b) && past(b, a)
                };
                if top_share < 0.6 || runs_on(top, tl, tr) {
                    continue;
                }
                for &(bottom, bl, br) in &caps[cap_index + 1..] {
                    let bottom_y = (bl.1 + br.1) / 2.0;
                    let height = bottom_y - top_y;
                    if height < min_opening {
                        continue;
                    }
                    // Something standing on the floor is a door if it is
                    // the size of one. If not, it is furniture or a part of
                    // the room.
                    let on_floor = h as f32 - bottom_y <= floor_reach;
                    let kind = if !on_floor {
                        ObstructionKind::Window
                    } else if !bottom.is_bare() {
                        // The floor itself stands in for anything this low.
                        continue;
                    } else if door_width.contains(&width) && h as f32 - top_y >= door_height {
                        ObstructionKind::Door
                    } else {
                        continue;
                    };
                    let (bl, br) = if on_floor {
                        (floor_left, floor_right)
                    } else {
                        (bl, br)
                    };
                    let sides = [share(left, tl, bl), share(right, tr, br), top_share];
                    let base = if on_floor { 1.0 } else { share(bottom, bl, br) };
                    if sides.iter().any(|&share| share < 0.6) || base < 0.6 {
                        continue;
                    }
                    if !on_floor && runs_on(bottom, bl, br) {
                        continue;
                    }
                    // The sides must reach the corners. Without this, the
                    // side of a window stretched up to the ceiling line
                    // still looks mostly covered. One whole corner at the
                    // top is enough, an upright and a level edge meeting:
                    // the other is often painted over, in shadow or behind
                    // a curtain.
                    let (tall, wide) = ((height / 5.0).max(4.0), (width / 5.0).max(4.0));
                    let top_left =
                        met(share_from(left, tl, bl, tall)) && met(share_from(top, tl, tr, wide));
                    let top_right =
                        met(share_from(right, tr, br, tall)) && met(share_from(top, tr, tl, wide));
                    // A side at the very end of the wall is on the room's
                    // own corner, which is an edge whether or not anything
                    // stands there. Its top corner has to be seen.
                    let meets_top = (top_left || top_right)
                        && (top_left || !at_wall_end(tl.0))
                        && (top_right || !at_wall_end(tr.0));
                    // A door's casing runs down to the floor, give or take
                    // the baseboard, where a window's sides stop at its
                    // sill. One side is enough: the other is often lost
                    // against the baseboard or behind something.
                    let meets_bottom = if on_floor {
                        let above_base = |line: &Line, floor: (f32, f32), up: (f32, f32)| {
                            let start = line.t_of(floor.0, floor.1);
                            let sense = (line.t_of(up.0, up.1) - start).signum();
                            let from = start + sense * floor_reach;
                            line.supported(from, from + sense * tall) / tall
                        };
                        met(above_base(left, bl, tl)) || met(above_base(right, br, tr))
                    } else {
                        (met(share_from(left, bl, tl, tall))
                            && met(share_from(bottom, bl, br, wide)))
                            || (met(share_from(right, br, tr, tall))
                                && met(share_from(bottom, br, bl, wide)))
                    };
                    if !(meets_top && meets_bottom) {
                        continue;
                    }
                    let outline = (sides.iter().sum::<f32>() + base) / 4.0;

                    // The upright box nearest to the four corners.
                    let edge = |a: f32, b: f32, size: usize| {
                        ((a + b) / 2.0).round().clamp(0.0, size as f32) as usize
                    };
                    let rect = Box {
                        x0: edge(tl.0, bl.0, w),
                        x1: edge(tr.0, br.0, w),
                        y0: edge(tl.1, tr.1, h),
                        y1: if on_floor { h } else { edge(bl.1, br.1, h) },
                    };
                    if rect.x1 <= rect.x0 + 2 || rect.y1 <= rect.y0 + 2 {
                        continue;
                    }

                    // Plain wall just outside, on every side but the floor:
                    // a band with next to no edges in it. A door or window
                    // hard against the end of the wall has the room's
                    // corner beside it instead, so that side is not held to
                    // this.
                    // Beside a door only the upper part is looked at:
                    // furniture stands against the wall lower down.
                    let beside = Box {
                        y1: if on_floor {
                            rect.y0 + (rect.y1 - rect.y0) * 9 / 20
                        } else {
                            rect.y1
                        },
                        ..rect
                    };
                    let mut bands = Vec::with_capacity(4);
                    if rect.x0 > wall_end {
                        bands.push(Box {
                            x0: rect.x0.saturating_sub(offset + band),
                            x1: rect.x0.saturating_sub(offset),
                            ..beside
                        });
                    }
                    if rect.x1 + wall_end < w {
                        bands.push(Box {
                            x0: rect.x1 + offset,
                            x1: (rect.x1 + offset + band).min(w),
                            ..beside
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

/// Finds small isolated blobs the size of a cover plate.
fn find_plates(scene: &Scene, openings: &[Box]) -> Vec<Box> {
    let (view, gradient) = (scene.view, scene.gradient);
    let (w, h) = (view.image.w, view.image.h);
    let px = |millimetres: f64| (millimetres / view.scale).round().max(1.0) as usize;

    // A plate shows up as a knot of edges: its outline, and the slots or
    // rockers inside it. The full width of each edge is used here, not just
    // its crest, so that the pieces of a small plate join up.
    let mut mask: Vec<bool> = (0..w * h)
        .map(|index| view.seen[index] && gradient.mag[index] >= scene.threshold)
        .collect();

    // Anything that runs on further than a plate is wide is not part of
    // one: the top of a baseboard, a door casing, the edge of a tiled dado.
    // Taking those away first frees a plate that sits against one.
    let too_long = px(*mm::PLATE_WIDTH.end() + 40.0);
    let mut long = vec![false; w * h];
    let mut clear_runs = |cells: &mut dyn Iterator<Item = usize>| {
        let mut run: Vec<usize> = Vec::new();
        let mut flush = |run: &mut Vec<usize>| {
            if run.len() > too_long {
                run.iter().for_each(|&index| long[index] = true);
            }
            run.clear();
        };
        for index in cells {
            if mask[index] {
                run.push(index);
            } else {
                flush(&mut run);
            }
        }
        flush(&mut run);
    };
    for y in 0..h {
        clear_runs(&mut (0..w).map(|x| y * w + x));
    }
    for x in 0..w {
        clear_runs(&mut (0..h).map(|y| y * w + x));
    }
    for (cell, &long) in mask.iter_mut().zip(&long) {
        *cell &= !long;
    }
    // Nor is a speck: rough plaster is covered in them, and joined to a
    // plate they would make it look bigger than it is.
    let speck = px(mm::SPECK);
    let mut seen_piece = vec![false; w * h];
    let mut piece = Vec::new();
    for start in 0..w * h {
        if !mask[start] || seen_piece[start] {
            continue;
        }
        piece.clear();
        piece.push(start);
        seen_piece[start] = true;
        let mut next = 0;
        while next < piece.len() {
            let (x, y) = (piece[next] % w, piece[next] / w);
            next += 1;
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let index = ny * w + nx;
                    if mask[index] && !seen_piece[index] {
                        seen_piece[index] = true;
                        piece.push(index);
                    }
                }
            }
        }
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for &index in &piece {
            let (x, y) = (index % w, index / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        if (x1 - x0).max(y1 - y0) < speck {
            piece.iter().for_each(|&index| mask[index] = false);
        }
    }
    let busy = Sums::of(w, h, &mask);

    // The outline of a pale plate on a pale wall comes out in pieces, with
    // the sides fainter than the top and bottom. Pieces this close together
    // are taken as one thing.
    let join = px(mm::PLATE_JOIN);
    let mut joined = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] {
                for ny in y.saturating_sub(join)..=(y + join).min(h - 1) {
                    for nx in x.saturating_sub(join)..=(x + join).min(w - 1) {
                        joined[ny * w + nx] = true;
                    }
                }
            }
        }
    }

    let ring = px(mm::PLATE_RING).max(2);
    // An edge is drawn about this much wider than the thing it surrounds.
    const HALO: usize = 1;
    // Blobs of the right size and shape, before asking what is around them.
    let mut shaped: Vec<Box> = Vec::new();
    // Blobs as wide as a plate but not as tall: possibly half of one.
    let mut halves: Vec<Box> = Vec::new();
    let mut visited = vec![false; w * h];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !joined[start] || visited[start] {
            continue;
        }
        // Flood fill one blob, tracking the bounds of the edges in it and
        // the first and last of them on each row.
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        visited[start] = true;
        stack.push(start);
        while let Some(index) = stack.pop() {
            let (x, y) = (index % w, index / w);
            if mask[index] {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                spans.push((y, x, x + 1));
            }
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let next = ny * w + nx;
                    if joined[next] && !visited[next] {
                        visited[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        if x1 <= x0 + 2 * HALO || y1 <= y0 + 2 * HALO {
            continue;
        }
        let blob = Box { x0, y0, x1, y1 };
        let width = (x1 - x0 - 2 * HALO) as f64 * view.scale;
        let height = (y1 - y0 - 2 * HALO) as f64 * view.scale;
        if !mm::PLATE_WIDTH.contains(&width) || !mm::PLATE_HEIGHT.contains(&height) {
            if mm::PLATE_WIDTH.contains(&width) && height < *mm::PLATE_HEIGHT.start() {
                halves.push(blob);
            }
            continue;
        }
        // A plate is a solid rectangle, even when only parts of its outline
        // show. Fill in between the first and last edge on each row and in
        // each column: a real plate comes out covering its whole box.
        let (bw, bh) = (x1 - x0, y1 - y0);
        let mut rows = vec![(usize::MAX, 0usize); bh];
        let mut columns = vec![(usize::MAX, 0usize); bw];
        for (y, x, _) in spans {
            let row = &mut rows[y - y0];
            *row = (row.0.min(x), row.1.max(x + 1));
            let column = &mut columns[x - x0];
            *column = (column.0.min(y), column.1.max(y + 1));
        }
        let spanned = (0..bh)
            .flat_map(|j| (0..bw).map(move |i| (i, j)))
            .filter(|&(i, j)| {
                let (x, y) = (x0 + i, y0 + j);
                (rows[j].0 <= x && x < rows[j].1) || (columns[i].0 <= y && y < columns[i].1)
            })
            .count();
        if (spanned as f32) < SOLID * blob.area() {
            continue;
        }
        shaped.push(blob);
    }

    // A white plate on a white wall can show only its top and bottom edges,
    // where the light catches them. Two plate-wide blobs, one directly above
    // the other and a plate's height apart, are taken to be that.
    halves.sort_by_key(|half| half.y0);
    let mut used = vec![false; halves.len()];
    for upper in 0..halves.len() {
        for lower in upper + 1..halves.len() {
            if used[upper] || used[lower] {
                continue;
            }
            let (a, b) = (halves[upper], halves[lower]);
            let shared = a.x1.min(b.x1).saturating_sub(a.x0.max(b.x0));
            let narrower = (a.x1 - a.x0).min(b.x1 - b.x0);
            let whole = Box {
                x0: a.x0.min(b.x0),
                y0: a.y0,
                x1: a.x1.max(b.x1),
                y1: b.y1,
            };
            let width = (whole.x1 - whole.x0 - 2 * HALO) as f64 * view.scale;
            let height = (whole.y1 - whole.y0 - 2 * HALO) as f64 * view.scale;
            if b.y0 < a.y1
                || shared * 10 < narrower * 7
                || !mm::PLATE_WIDTH.contains(&width)
                || !mm::PLATE_HEIGHT.contains(&height)
            {
                continue;
            }
            used[upper] = true;
            used[lower] = true;
            shaped.push(whole);
        }
    }

    // Nothing else close by: a plate sits alone on bare wall, or beside
    // another plate.
    let mut plates = Vec::new();
    for (index, blob) in shaped.iter().enumerate() {
        let around = Box {
            x0: blob.x0.saturating_sub(ring),
            y0: blob.y0.saturating_sub(ring),
            x1: (blob.x1 + ring).min(w),
            y1: (blob.y1 + ring).min(h),
        };
        let neighbours: u32 = shaped
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != index)
            .map(|(_, other)| {
                busy.count(&Box {
                    x0: around.x0.max(other.x0),
                    y0: around.y0.max(other.y0),
                    x1: around.x1.min(other.x1).max(around.x0.max(other.x0)),
                    y1: around.y1.min(other.y1).max(around.y0.max(other.y0)),
                })
            })
            .sum();
        let nearby = (busy.count(&around) - busy.count(blob)).saturating_sub(neighbours);
        if nearby as f32 > 0.1 * (around.area() - blob.area()) {
            continue;
        }
        if openings.iter().any(|opening| blob.overlap(opening) > 0.0) {
            continue;
        }
        plates.push(Box {
            x0: blob.x0 + HALO,
            y0: blob.y0 + HALO,
            x1: blob.x1 - HALO,
            y1: blob.y1 - HALO,
        });
    }
    plates
}
