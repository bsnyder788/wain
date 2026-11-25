//! Maps a measured wall onto a photo of it.
//!
//! The user marks the wall's four corners in a photo. A flat wall seen by a
//! camera is a projective transform (a homography) of the wall's elevation,
//! and four point pairs pin that transform down exactly. With it, anything
//! laid out on the wall can be drawn over the photo in true perspective, and
//! a point tapped on the photo can be turned back into a position on the
//! wall.
//!
//! Lens distortion is ignored: straight lines are assumed to stay straight.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::units::Length;

/// A position in a photo. The origin is the image's top-left corner, `x`
/// runs right and `y` runs down. The unit is whatever the caller uses
/// consistently: pixels, or fractions of the image size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }
}

/// Why four points cannot be the corners of a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadError {
    /// A coordinate or wall dimension is not a usable number.
    NotFinite,
    /// The corners do not go top-left, top-right, bottom-right, bottom-left
    /// around a four-sided outline: some are crossed over, in line with each
    /// other, or on top of one another.
    NotAnOutline,
}

impl fmt::Display for QuadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuadError::NotFinite => write!(f, "the wall size and corner positions must be numbers"),
            QuadError::NotAnOutline => write!(
                f,
                "the corners must go around the wall in order without crossing over"
            ),
        }
    }
}

impl std::error::Error for QuadError {}

/// A 3×3 matrix in row order, applied to `(x, y, 1)`.
type Matrix = [f64; 9];

/// The projective transform between a wall's elevation and a photo of it.
///
/// Wall positions are in nanometres from the wall's bottom-left corner, as
/// everywhere else in the engine. The matrices are exposed so a client can
/// apply them without calling back into the engine for every point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Homography {
    /// Wall (nanometres) to photo.
    pub forward: Matrix,
    /// Photo to wall (nanometres).
    pub inverse: Matrix,
}

impl Homography {
    /// Builds the transform for a wall of the given size whose corners appear
    /// in the photo at `corners`, ordered top-left, top-right, bottom-right,
    /// bottom-left.
    pub fn for_wall(
        width: Length,
        height: Length,
        corners: &[Point; 4],
    ) -> Result<Homography, QuadError> {
        if !width.is_positive() || !height.is_positive() {
            return Err(QuadError::NotFinite);
        }
        if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(QuadError::NotFinite);
        }
        check_outline(corners)?;

        let (w, h) = (width.nm() as f64, height.nm() as f64);
        let square_to_photo = unit_square_to_quad(corners).ok_or(QuadError::NotAnOutline)?;
        let photo_to_square = invert(&square_to_photo).ok_or(QuadError::NotAnOutline)?;

        // The unit square has its origin at the wall's top-left with y down,
        // matching the photo. The wall's origin is bottom-left with y up.
        let wall_to_square = [1.0 / w, 0.0, 0.0, 0.0, -1.0 / h, 1.0, 0.0, 0.0, 1.0];
        let square_to_wall = [w, 0.0, 0.0, 0.0, -h, h, 0.0, 0.0, 1.0];

        let homography = Homography {
            forward: multiply(&square_to_photo, &wall_to_square),
            inverse: multiply(&square_to_wall, &photo_to_square),
        };
        let finite = |m: &Matrix| m.iter().all(|value| value.is_finite());
        if finite(&homography.forward) && finite(&homography.inverse) {
            Ok(homography)
        } else {
            Err(QuadError::NotAnOutline)
        }
    }

    /// Where a position on the wall appears in the photo.
    pub fn project(&self, x: Length, y: Length) -> Point {
        let (u, v) = apply(&self.forward, x.nm() as f64, y.nm() as f64);
        Point::new(u, v)
    }

    /// The position on the wall that a point in the photo shows, in
    /// nanometres as `(x, y)`. It can lie outside the wall. Returns `None`
    /// for a point on the wall plane's horizon, which shows no position.
    pub fn unproject(&self, point: Point) -> Option<(f64, f64)> {
        let (x, y) = apply(&self.inverse, point.x, point.y);
        (x.is_finite() && y.is_finite()).then_some((x, y))
    }
}

fn apply(m: &Matrix, x: f64, y: f64) -> (f64, f64) {
    let w = m[6] * x + m[7] * y + m[8];
    (
        (m[0] * x + m[1] * y + m[2]) / w,
        (m[3] * x + m[4] * y + m[5]) / w,
    )
}

/// Requires the corners to turn the same way at every vertex (clockwise as
/// seen in the photo), which rules out crossed, collinear and repeated
/// points, and outlines traced in the wrong direction.
fn check_outline(corners: &[Point; 4]) -> Result<(), QuadError> {
    for i in 0..4 {
        let (a, b, c) = (corners[i], corners[(i + 1) % 4], corners[(i + 2) % 4]);
        let (e1x, e1y) = (b.x - a.x, b.y - a.y);
        let (e2x, e2y) = (c.x - b.x, c.y - b.y);
        let cross = e1x * e2y - e1y * e2x;
        // The sine of the turn must be clearly positive, whatever the scale.
        let scale = (e1x * e1x + e1y * e1y) * (e2x * e2x + e2y * e2y);
        if !(cross > 0.0 && cross * cross > 1e-10 * scale) {
            return Err(QuadError::NotAnOutline);
        }
    }
    Ok(())
}

/// The transform taking the unit square's corners (0,0), (1,0), (1,1), (0,1)
/// to the four given points, in that order. This is the closed form from
/// Heckbert's "Fundamentals of Texture Mapping and Image Warping" (1989).
fn unit_square_to_quad(p: &[Point; 4]) -> Option<Matrix> {
    let (dx1, dy1) = (p[1].x - p[2].x, p[1].y - p[2].y);
    let (dx2, dy2) = (p[3].x - p[2].x, p[3].y - p[2].y);
    let (sx, sy) = (
        p[0].x - p[1].x + p[2].x - p[3].x,
        p[0].y - p[1].y + p[2].y - p[3].y,
    );

    let det = dx1 * dy2 - dx2 * dy1;
    if det == 0.0 {
        return None;
    }
    let g = (sx * dy2 - dx2 * sy) / det;
    let h = (dx1 * sy - sx * dy1) / det;
    Some([
        p[1].x - p[0].x + g * p[1].x,
        p[3].x - p[0].x + h * p[3].x,
        p[0].x,
        p[1].y - p[0].y + g * p[1].y,
        p[3].y - p[0].y + h * p[3].y,
        p[0].y,
        g,
        h,
        1.0,
    ])
}

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [0.0; 9];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = (0..3).map(|k| a[row * 3 + k] * b[k * 3 + col]).sum();
        }
    }
    out
}

fn invert(m: &Matrix) -> Option<Matrix> {
    let cofactor = |a: usize, b: usize, c: usize, d: usize| m[a] * m[d] - m[b] * m[c];
    let adjugate = [
        cofactor(4, 5, 7, 8),
        -cofactor(1, 2, 7, 8),
        cofactor(1, 2, 4, 5),
        -cofactor(3, 5, 6, 8),
        cofactor(0, 2, 6, 8),
        -cofactor(0, 2, 3, 5),
        cofactor(3, 4, 6, 7),
        -cofactor(0, 1, 6, 7),
        cofactor(0, 1, 3, 4),
    ];
    let det = m[0] * adjugate[0] + m[1] * adjugate[3] + m[2] * adjugate[6];
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    Some(adjugate.map(|value| value / det))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: Length = Length::inches(144);
    const HEIGHT: Length = Length::inches(96);

    /// A wall seen from the left: the right-hand edge is further away, so it
    /// is shorter in the photo.
    const ANGLED: [Point; 4] = [
        Point::new(100.0, 80.0),
        Point::new(900.0, 220.0),
        Point::new(880.0, 560.0),
        Point::new(120.0, 700.0),
    ];

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
    }

    fn angled() -> Homography {
        Homography::for_wall(WIDTH, HEIGHT, &ANGLED).unwrap()
    }

    #[test]
    fn wall_corners_land_on_the_marked_corners() {
        let h = angled();
        let zero = Length::ZERO;
        assert!(close(h.project(zero, HEIGHT), ANGLED[0]), "top-left");
        assert!(close(h.project(WIDTH, HEIGHT), ANGLED[1]), "top-right");
        assert!(close(h.project(WIDTH, zero), ANGLED[2]), "bottom-right");
        assert!(close(h.project(zero, zero), ANGLED[3]), "bottom-left");
    }

    #[test]
    fn a_square_on_view_is_a_plain_scale() {
        let straight = [
            Point::new(0.0, 0.0),
            Point::new(1440.0, 0.0),
            Point::new(1440.0, 960.0),
            Point::new(0.0, 960.0),
        ];
        let h = Homography::for_wall(WIDTH, HEIGHT, &straight).unwrap();
        // Ten pixels to the inch; 36" up the wall is 60" down from the top.
        let p = h.project(Length::inches(48), Length::inches(36));
        assert!(close(p, Point::new(480.0, 600.0)), "{p:?}");
    }

    #[test]
    fn the_wall_centre_lands_where_the_diagonals_cross() {
        // Perspective does not keep midpoints, but it keeps straight lines,
        // so the centre of the wall must sit on both diagonals of the outline.
        let centre = angled().project(WIDTH / 2, HEIGHT / 2);
        let on_line = |a: Point, b: Point| {
            let cross = (b.x - a.x) * (centre.y - a.y) - (b.y - a.y) * (centre.x - a.x);
            cross.abs() < 1e-6
        };
        assert!(on_line(ANGLED[0], ANGLED[2]));
        assert!(on_line(ANGLED[1], ANGLED[3]));
        // And it is nearer the far (right) edge than the plain midpoint is.
        let midpoint = (ANGLED[0].x + ANGLED[1].x + ANGLED[2].x + ANGLED[3].x) / 4.0;
        assert!(centre.x > midpoint);
    }

    #[test]
    fn straight_lines_on_the_wall_stay_straight() {
        let h = angled();
        let rail_height = Length::inches(54);
        let points: Vec<Point> = (0..=4)
            .map(|i| h.project(WIDTH * i / 4, rail_height))
            .collect();
        let (a, b) = (points[0], points[4]);
        for p in &points {
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
            assert!(cross.abs() < 1e-6, "{p:?} is off the line");
        }
    }

    #[test]
    fn unproject_undoes_project() {
        let h = angled();
        for (x, y) in [(0, 0), (17, 12), (72, 48), (144, 96), (130, 5)] {
            let (x, y) = (Length::inches(x), Length::inches(y));
            let (back_x, back_y) = h.unproject(h.project(x, y)).unwrap();
            // Well under a micrometre on a twelve-foot wall.
            assert!((back_x - x.nm() as f64).abs() < 100.0, "x was {back_x}");
            assert!((back_y - y.nm() as f64).abs() < 100.0, "y was {back_y}");
        }
    }

    #[test]
    fn rejects_corners_that_do_not_outline_a_wall() {
        let quad = |points: [(f64, f64); 4]| points.map(|(x, y)| Point::new(x, y));
        let build = |points| Homography::for_wall(WIDTH, HEIGHT, &quad(points));
        let outline = Err(QuadError::NotAnOutline);

        // Top corners swapped: the outline crosses itself.
        assert_eq!(
            build([
                (900.0, 220.0),
                (100.0, 80.0),
                (880.0, 560.0),
                (120.0, 700.0)
            ]),
            outline
        );
        // Traced anticlockwise (a mirror image).
        assert_eq!(
            build([
                (900.0, 220.0),
                (100.0, 80.0),
                (120.0, 700.0),
                (880.0, 560.0)
            ]),
            outline
        );
        // One corner pushed inside the others.
        assert_eq!(
            build([
                (100.0, 80.0),
                (900.0, 220.0),
                (400.0, 300.0),
                (120.0, 700.0)
            ]),
            outline
        );
        // Three corners in a line.
        assert_eq!(
            build([(0.0, 0.0), (500.0, 0.0), (1000.0, 0.0), (0.0, 700.0)]),
            outline
        );
        // Two corners on top of each other.
        assert_eq!(
            build([(100.0, 80.0), (100.0, 80.0), (880.0, 560.0), (120.0, 700.0)]),
            outline
        );

        let not_finite = Err(QuadError::NotFinite);
        assert_eq!(
            build([
                (f64::NAN, 80.0),
                (900.0, 220.0),
                (880.0, 560.0),
                (120.0, 700.0)
            ]),
            not_finite
        );
        assert_eq!(
            Homography::for_wall(Length::ZERO, HEIGHT, &ANGLED),
            not_finite
        );
    }

    #[test]
    fn works_in_fractions_of_the_image_too() {
        let fractions = ANGLED.map(|p| Point::new(p.x / 1000.0, p.y / 800.0));
        let h = Homography::for_wall(WIDTH, HEIGHT, &fractions).unwrap();
        let p = h.project(WIDTH, Length::ZERO);
        assert!(close(p, Point::new(0.88, 0.7)), "{p:?}");
    }
}
