//! Finds long straight edges in a photo with a Hough transform.
//!
//! Every edge pixel votes for the lines that could pass through it. A real
//! line collects a vote from each pixel along it, so even a faint one, like
//! a wall meeting a ceiling of nearly the same colour, stands out if it is
//! long. Each pixel only votes for lines running roughly along its own edge
//! direction, which keeps texture and clutter from adding up to false lines.

use crate::raster::Gradient;

/// Angle step of the vote table, in degrees.
const ANGLE_BINS: usize = 180;
/// The table's first angle. Starting at -45 degrees puts the wrap-around on
/// a diagonal, clear of the upright and level lines that matter here.
const FIRST_ANGLE: f32 = -45.0;
/// A pixel votes for normals within this many degrees of its gradient.
const VOTE_SPREAD: isize = 6;
/// An edge pixel belongs to a line when it is this close to it, in pixels.
const NEAR: f32 = 1.6;
/// Gaps in a line's support shorter than this are bridged, in pixels.
const BRIDGE: usize = 3;

/// A straight line, with a record of where along it the photo has an edge.
///
/// Points on the line satisfy `nx * x + ny * y = rho`. Positions along it
/// are measured by `t`, the distance from the point nearest the origin.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub nx: f32,
    pub ny: f32,
    pub rho: f32,
    /// Supported length in pixels: how much of the line lies on an edge.
    pub strength: f32,
    /// How sharp that edge is on average, in brightness levels per pixel.
    /// The join between two surfaces is a strong step; a seam between
    /// floorboards is a hairline that barely registers.
    pub contrast: f32,
    /// `t` of the first support bin.
    t0: f32,
    /// Running count of supported one-pixel bins, starting at `t0`.
    cumulative: Vec<u32>,
}

impl Line {
    /// A line with no edge behind it, such as the border of the photo.
    pub fn bare(nx: f32, ny: f32, rho: f32) -> Line {
        Line {
            nx,
            ny,
            rho,
            strength: 0.0,
            contrast: 0.0,
            t0: 0.0,
            cumulative: vec![0],
        }
    }

    pub fn is_bare(&self) -> bool {
        self.cumulative.len() <= 1
    }

    /// Position of a point along the line.
    pub fn t_of(&self, x: f32, y: f32) -> f32 {
        -self.ny * x + self.nx * y
    }

    /// How many pixels of edge lie on the line between two positions.
    pub fn supported(&self, ta: f32, tb: f32) -> f32 {
        let (lo, hi) = if ta <= tb { (ta, tb) } else { (tb, ta) };
        let last = self.cumulative.len() as isize - 1;
        let bin = |t: f32| ((t - self.t0).round() as isize).clamp(0, last) as usize;
        (self.cumulative[bin(hi)] - self.cumulative[bin(lo)]) as f32
    }

    /// The first and last supported positions, if any.
    pub fn extent(&self) -> Option<(f32, f32)> {
        let total = *self.cumulative.last()?;
        if total == 0 {
            return None;
        }
        let first = self.cumulative.iter().position(|&count| count > 0)?;
        let last = self.cumulative.iter().position(|&count| count == total)?;
        Some((self.t0 + first as f32 - 1.0, self.t0 + last as f32 - 1.0))
    }

    /// Whether the line's edge stops at about `t`: solid on one side of it
    /// and absent on the other.
    pub fn ends_near(&self, t: f32) -> bool {
        const REACH: f32 = 24.0;
        [-4.0, 0.0, 4.0].iter().any(|shift| {
            let at = t + shift;
            let behind = self.supported(at - REACH, at) / REACH;
            let ahead = self.supported(at, at + REACH) / REACH;
            (ahead >= 0.6 && behind <= 0.25) || (behind >= 0.6 && ahead <= 0.25)
        })
    }

    pub fn y_at(&self, x: f32) -> f32 {
        (self.rho - self.nx * x) / self.ny
    }

    pub fn x_at(&self, y: f32) -> f32 {
        (self.rho - self.ny * y) / self.nx
    }

    /// Where two lines cross, unless they are parallel.
    pub fn intersect(&self, other: &Line) -> Option<(f32, f32)> {
        let det = self.nx * other.ny - self.ny * other.nx;
        if det.abs() < 1e-6 {
            return None;
        }
        Some((
            (self.rho * other.ny - self.ny * other.rho) / det,
            (self.nx * other.rho - self.rho * other.nx) / det,
        ))
    }

    /// Degrees the line leans away from upright (0 for a plumb line).
    pub fn lean_from_vertical(&self) -> f32 {
        self.ny.abs().atan2(self.nx.abs()).to_degrees()
    }

    /// Degrees the line slopes away from level (0 for a level line).
    pub fn slope_from_horizontal(&self) -> f32 {
        self.nx.abs().atan2(self.ny.abs()).to_degrees()
    }
}

/// An edge pixel: where it is and which way the image changes across it.
struct EdgePixel {
    x: f32,
    y: f32,
    /// Unit gradient.
    gx: f32,
    gy: f32,
    /// How fast the image changes here.
    steepness: f32,
}

/// The strongest lines in the image, best first. `min_votes` is the shortest
/// supported length worth reporting and `limit` caps how many come back.
pub(crate) fn find_lines(
    gradient: &Gradient,
    edges: &[bool],
    min_votes: f32,
    limit: usize,
) -> Vec<Line> {
    let (w, h) = (gradient.w, gradient.h);
    let pixels: Vec<EdgePixel> = edges
        .iter()
        .enumerate()
        .filter(|&(_, &on)| on)
        .map(|(index, _)| {
            let m = gradient.mag[index].max(1e-6);
            EdgePixel {
                x: (index % w) as f32,
                y: (index / w) as f32,
                gx: gradient.gx[index] / m,
                gy: gradient.gy[index] / m,
                steepness: m,
            }
        })
        .collect();

    let diagonal = ((w * w + h * h) as f32).sqrt().ceil() as usize;
    let rho_bins = 2 * diagonal + 1;
    let trig: Vec<(f32, f32)> = (0..ANGLE_BINS)
        .map(|bin| {
            let angle = (FIRST_ANGLE + bin as f32).to_radians();
            (angle.cos(), angle.sin())
        })
        .collect();

    let mut votes = vec![0u16; ANGLE_BINS * rho_bins];
    for pixel in &pixels {
        // The gradient is the line's normal. Normals half a turn apart are
        // the same line, so fold the angle into the table's range.
        let mut angle = pixel.gy.atan2(pixel.gx).to_degrees();
        if angle < FIRST_ANGLE {
            angle += 180.0;
        } else if angle >= FIRST_ANGLE + 180.0 {
            angle -= 180.0;
        }
        let centre = (angle - FIRST_ANGLE).round() as isize;
        for offset in -VOTE_SPREAD..=VOTE_SPREAD {
            let bin = centre + offset;
            // Votes that would wrap land on diagonals, which are not used.
            if bin < 0 || bin >= ANGLE_BINS as isize {
                continue;
            }
            let (cos, sin) = trig[bin as usize];
            let rho = (pixel.x * cos + pixel.y * sin).round() as isize + diagonal as isize;
            let cell = &mut votes[bin as usize * rho_bins + rho as usize];
            *cell = cell.saturating_add(1);
        }
    }

    // Peaks: cells that beat everything within a few degrees and pixels.
    const ANGLE_REACH: isize = 4;
    const RHO_REACH: isize = 5;
    let mut peaks: Vec<(u16, usize, usize)> = Vec::new();
    for bin in 0..ANGLE_BINS {
        for rho in 0..rho_bins {
            let count = votes[bin * rho_bins + rho];
            if (count as f32) < min_votes * 0.6 {
                continue;
            }
            let mut is_peak = true;
            'around: for da in -ANGLE_REACH..=ANGLE_REACH {
                let a = bin as isize + da;
                if a < 0 || a >= ANGLE_BINS as isize {
                    continue;
                }
                for dr in -RHO_REACH..=RHO_REACH {
                    let r = rho as isize + dr;
                    if r < 0 || r >= rho_bins as isize || (da == 0 && dr == 0) {
                        continue;
                    }
                    let other = votes[a as usize * rho_bins + r as usize];
                    // Ties go to the earlier cell so exactly one survives.
                    if other > count || (other == count && (da, dr) < (0, 0)) {
                        is_peak = false;
                        break 'around;
                    }
                }
            }
            if is_peak {
                peaks.push((count, bin, rho));
            }
        }
    }
    peaks.sort_by_key(|peak| std::cmp::Reverse(peak.0));
    peaks.truncate(limit * 3);

    let mut lines: Vec<Line> = Vec::new();
    for (_, bin, rho) in peaks {
        let (cos, sin) = trig[bin];
        let rough = Line::bare(cos, sin, rho as f32 - diagonal as f32);
        let Some(line) = refine(&rough, &pixels, w, h) else {
            continue;
        };
        if line.strength < min_votes {
            continue;
        }
        // Two peaks can settle on the same line once refined.
        let duplicate = lines.iter().any(|kept| same_line(kept, &line, w, h));
        if !duplicate {
            lines.push(line);
        }
    }
    lines.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    lines.truncate(limit);
    lines
}

/// True when two lines stay within a couple of pixels of each other across
/// the whole image.
fn same_line(a: &Line, b: &Line, w: usize, h: usize) -> bool {
    let (w, h) = (w as f32, h as f32);
    let apart = |x: f32, y: f32| (b.nx * x + b.ny * y - b.rho).abs();
    // Compare where `a` enters and leaves the image.
    let ends = if a.nx.abs() > a.ny.abs() {
        [(a.x_at(0.0), 0.0), (a.x_at(h), h)]
    } else {
        [(0.0, a.y_at(0.0)), (w, a.y_at(w))]
    };
    ends.iter().all(|&(x, y)| apart(x, y) < 2.5)
}

/// Fits the line to the edge pixels that lie on it, then records where
/// along its length those pixels are.
fn refine(rough: &Line, pixels: &[EdgePixel], w: usize, h: usize) -> Option<Line> {
    let mut line = rough.clone();
    // The first pass reaches a little wider, since the peak is only good to
    // a degree and a pixel.
    for reach in [NEAR * 1.6, NEAR, NEAR] {
        let (mut n, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64);
        let (mut sxx, mut sxy, mut syy) = (0.0f64, 0.0f64, 0.0f64);
        for pixel in pixels.iter().filter(|p| on_line(&line, p, reach)) {
            let (x, y) = (pixel.x as f64, pixel.y as f64);
            n += 1.0;
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
            syy += y * y;
        }
        if n < 8.0 {
            return None;
        }
        let (mx, my) = (sx / n, sy / n);
        let (cxx, cxy, cyy) = (sxx / n - mx * mx, sxy / n - mx * my, syy / n - my * my);
        // The line runs along the direction of greatest spread.
        let along = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
        let (mut nx, mut ny) = (-along.sin(), along.cos());
        // Keep the normal pointing the way it did, so `t` keeps its sense.
        if nx * f64::from(line.nx) + ny * f64::from(line.ny) < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        line.nx = nx as f32;
        line.ny = ny as f32;
        line.rho = (nx * mx + ny * my) as f32;
    }

    // Support: one bin per pixel of length across the image.
    let corners = [
        (0.0, 0.0),
        (w as f32, 0.0),
        (0.0, h as f32),
        (w as f32, h as f32),
    ];
    let ts = corners.map(|(x, y)| line.t_of(x, y));
    let t0 = ts.iter().copied().fold(f32::INFINITY, f32::min).floor();
    let t1 = ts.iter().copied().fold(f32::NEG_INFINITY, f32::max).ceil();
    let bins = (t1 - t0) as usize + 1;
    let mut on = vec![false; bins];
    let (mut steepness, mut count) = (0.0, 0.0);
    for pixel in pixels.iter().filter(|p| on_line(&line, p, NEAR)) {
        let bin = (line.t_of(pixel.x, pixel.y) - t0).round() as usize;
        on[bin.min(bins - 1)] = true;
        steepness += pixel.steepness;
        count += 1.0;
    }
    line.contrast = if count > 0.0 { steepness / count } else { 0.0 };
    bridge_gaps(&mut on);

    let mut cumulative = Vec::with_capacity(bins + 1);
    let mut total = 0u32;
    cumulative.push(0);
    for &bin in &on {
        total += bin as u32;
        cumulative.push(total);
    }
    line.t0 = t0;
    line.cumulative = cumulative;
    line.strength = total as f32;
    Some(line)
}

/// Whether an edge pixel lies on the line and runs the same way.
fn on_line(line: &Line, pixel: &EdgePixel, reach: f32) -> bool {
    let distance = line.nx * pixel.x + line.ny * pixel.y - line.rho;
    // Within about 25 degrees: faint edges have noisy directions.
    distance.abs() <= reach && (line.nx * pixel.gx + line.ny * pixel.gy).abs() > 0.9
}

/// Fills short gaps between supported bins.
fn bridge_gaps(on: &mut [bool]) {
    let mut last_on: Option<usize> = None;
    for index in 0..on.len() {
        if !on[index] {
            continue;
        }
        if let Some(last) = last_on
            && index - last > 1
            && index - last <= BRIDGE + 1
        {
            on[last + 1..index].iter_mut().for_each(|bin| *bin = true);
        }
        last_on = Some(index);
    }
}
