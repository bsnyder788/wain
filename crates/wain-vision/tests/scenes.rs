//! Runs the detectors on rooms drawn by this file, where the right answer
//! is known exactly.
//!
//! Each scene is a wall of known size seen through a known perspective, with
//! a ceiling, a floor and neighbouring walls around it, shaded and a little
//! noisy the way a photo is. Real photos are harder; `examples/detect.rs` is
//! for trying those.

use wain_core::{Homography, Length, Obstruction, ObstructionKind, Point, Rect};
use wain_vision::{Photo, find_obstructions, find_wall};

const WIDTH: usize = 800;
const HEIGHT: usize = 600;
const WALL_WIDTH: Length = Length::mm(3600);
const WALL_HEIGHT: Length = Length::mm(2400);

/// The wall seen straight on, with a margin of ceiling, floor and side wall.
const SQUARE_ON: [(f64, f64); 4] = [(100.0, 90.0), (700.0, 90.0), (700.0, 490.0), (100.0, 490.0)];
/// Seen from the left, so the right-hand end is further away and smaller.
const FROM_THE_LEFT: [(f64, f64); 4] =
    [(70.0, 50.0), (690.0, 130.0), (680.0, 450.0), (80.0, 540.0)];
/// Too close to fit: the right-hand end of the wall is out of shot.
const RIGHT_END_CUT_OFF: [(f64, f64); 4] =
    [(110.0, 80.0), (960.0, 80.0), (960.0, 500.0), (110.0, 500.0)];

fn mm(x: i64, y: i64, width: i64, height: i64) -> Rect {
    Rect::new(
        Length::mm(x),
        Length::mm(y),
        Length::mm(width),
        Length::mm(height),
    )
}

fn obstruction(kind: ObstructionKind, rect: Rect) -> Obstruction {
    Obstruction { kind, rect }
}

/// A door, a window, two outlets and a switch, measured to the outside of
/// their casings and cover plates.
fn furnishings() -> Vec<Obstruction> {
    vec![
        obstruction(ObstructionKind::Door, mm(350, 0, 950, 2090)),
        obstruction(ObstructionKind::Switch, mm(1450, 1110, 80, 120)),
        obstruction(ObstructionKind::Outlet, mm(1700, 290, 75, 115)),
        obstruction(ObstructionKind::Window, mm(2050, 900, 1100, 1250)),
        obstruction(ObstructionKind::Outlet, mm(3300, 290, 75, 115)),
    ]
}

struct Scene {
    rgba: Vec<u8>,
    corners: [Point; 4],
}

impl Scene {
    fn photo(&self) -> Photo<'_> {
        Photo::new(&self.rgba, WIDTH, HEIGHT).unwrap()
    }

    /// The true corners as fractions of the photo.
    fn fractions(&self) -> [Point; 4] {
        self.corners
            .map(|p| Point::new(p.x / WIDTH as f64, p.y / HEIGHT as f64))
    }
}

/// Small repeatable noise, so a failing test fails the same way every time.
struct Noise(u32);

impl Noise {
    /// Roughly -1.5 to 1.5 brightness levels.
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 16) & 0xff) as f32 / 255.0 * 3.0 - 1.5
    }
}

fn scale(colour: [f32; 3], by: f32) -> [f32; 3] {
    colour.map(|channel| channel * by)
}

/// Draws the room. Each pixel is traced back to a position on the wall's
/// plane; positions past the wall's edges are ceiling, floor or the walls
/// either side, split along the diagonals a real room corner would show.
fn render(corners: [(f64, f64); 4], on_wall: &[Obstruction]) -> Scene {
    let corners = corners.map(|(x, y)| Point::new(x, y));
    let homography = Homography::for_wall(WALL_WIDTH, WALL_HEIGHT, &corners).unwrap();
    let (wall_width, wall_height) = (WALL_WIDTH.as_mm(), WALL_HEIGHT.as_mm());
    let inside = |rect: &Rect, x: f64, y: f64, inset: f64| {
        x >= rect.x.as_mm() + inset
            && x < rect.right().as_mm() - inset
            && y >= rect.y.as_mm() + inset
            && y < rect.top().as_mm() - inset
    };

    let paint = [206.0, 200.0, 186.0];
    let trim = [244.0, 243.0, 238.0];
    let mut noise = Noise(7);
    let mut rgba = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for v in 0..HEIGHT {
        for u in 0..WIDTH {
            let (x, y) = homography
                .unproject(Point::new(u as f64 + 0.5, v as f64 + 0.5))
                .map(|(x, y)| (x / 1e6, y / 1e6))
                .unwrap_or((-1e9, -1e9));
            let (left, right) = (-x, x - wall_width);
            let (below, above) = (-y, y - wall_height);
            let beside = left.max(right);

            let colour = if above > 0.0 && above >= beside {
                [238.0, 238.0, 234.0]
            } else if below > 0.0 && below >= beside {
                // Floorboards running along the wall, with dark seams.
                let seam = (below / 140.0).fract() < 0.06;
                if seam {
                    [70.0, 50.0, 36.0]
                } else {
                    [128.0, 92.0, 64.0]
                }
            } else if left > 0.0 {
                scale(paint, 0.84)
            } else if right > 0.0 {
                scale(paint, 1.07)
            } else {
                // The wall itself: lit from the left, a little darker
                // toward the floor.
                let shade =
                    1.0 - 0.1 * (x / wall_width) as f32 - 0.05 * (1.0 - y / wall_height) as f32;
                let mut colour = scale(paint, shade);
                if y < 110.0 {
                    colour = scale(trim, shade);
                }
                for item in on_wall {
                    if !inside(&item.rect, x, y, 0.0) {
                        continue;
                    }
                    colour = match item.kind {
                        // White casing around a panelled door.
                        ObstructionKind::Door if inside(&item.rect, x, y, 70.0) => {
                            [226.0, 222.0, 212.0]
                        }
                        // White casing around bright glass.
                        ObstructionKind::Window if inside(&item.rect, x, y, 70.0) => {
                            [168.0, 204.0, 232.0]
                        }
                        // Cover plates, with a dark slot in the middle.
                        ObstructionKind::Outlet | ObstructionKind::Switch
                            if inside(&item.rect, x, y, 28.0) =>
                        {
                            [60.0, 60.0, 60.0]
                        }
                        _ => scale(trim, shade.max(0.95)),
                    };
                }
                colour
            };
            let grain = noise.next();
            for channel in colour {
                rgba.push((channel + grain).round().clamp(0.0, 255.0) as u8);
            }
            rgba.push(255);
        }
    }
    Scene { rgba, corners }
}

/// Largest distance between a found corner and the true one, as a share of
/// the photo's size.
fn worst_corner(found: &[Point; 4], truth: &[Point; 4]) -> f64 {
    found
        .iter()
        .zip(truth)
        .map(|(a, b)| (a.x - b.x).abs().max((a.y - b.y).abs()))
        .fold(0.0, f64::max)
}

#[test]
fn finds_a_wall_seen_square_on() {
    let scene = render(SQUARE_ON, &furnishings());
    let outline = find_wall(&scene.photo()).expect("an outline");
    assert_eq!(outline.sides.count(), 4, "{:?}", outline.sides);
    let off = worst_corner(&outline.corners, &scene.fractions());
    assert!(off < 0.012, "corners off by {off}: {:?}", outline.corners);
}

#[test]
fn finds_a_wall_seen_at_an_angle() {
    let scene = render(FROM_THE_LEFT, &furnishings());
    let outline = find_wall(&scene.photo()).expect("an outline");
    assert_eq!(outline.sides.count(), 4, "{:?}", outline.sides);
    let off = worst_corner(&outline.corners, &scene.fractions());
    assert!(off < 0.012, "corners off by {off}: {:?}", outline.corners);
}

#[test]
fn finds_the_floor_line_rather_than_the_baseboard_or_a_floorboard() {
    // The bare wall has two decoys for the bottom line: the top of the
    // baseboard just above the floor, and floorboard seams below it.
    let scene = render(SQUARE_ON, &[]);
    let outline = find_wall(&scene.photo()).expect("an outline");
    let truth = scene.fractions();
    for corner in [2, 3] {
        let off = (outline.corners[corner].y - truth[corner].y).abs();
        assert!(off < 0.012, "bottom corner {corner} off by {off}");
    }
}

#[test]
fn a_side_that_is_out_of_shot_falls_back_to_the_border() {
    let scene = render(RIGHT_END_CUT_OFF, &[]);
    let outline = find_wall(&scene.photo()).expect("an outline");
    let sides = outline.sides;
    assert!(sides.top && sides.bottom && sides.left, "{sides:?}");
    assert!(!sides.right, "the right-hand end is not in the photo");
    // The guessed corners sit on the photo's right-hand border.
    assert!((outline.corners[1].x - 1.0).abs() < 1e-6);
    assert!((outline.corners[2].x - 1.0).abs() < 1e-6);
    // The ones that were seen are still placed properly.
    let truth = scene.fractions();
    for corner in [0, 3] {
        let (found, truth) = (outline.corners[corner], truth[corner]);
        assert!((found.x - truth.x).abs() < 0.012 && (found.y - truth.y).abs() < 0.012);
    }
}

#[test]
fn finds_nothing_in_a_photo_with_no_wall_in_it() {
    let flat = vec![128u8; WIDTH * HEIGHT * 4];
    assert_eq!(find_wall(&Photo::new(&flat, WIDTH, HEIGHT).unwrap()), None);

    let mut noise = Noise(3);
    let speckled: Vec<u8> = (0..WIDTH * HEIGHT * 4)
        .map(|_| (128.0 + 20.0 * noise.next()) as u8)
        .collect();
    assert_eq!(
        find_wall(&Photo::new(&speckled, WIDTH, HEIGHT).unwrap()),
        None
    );
}

#[test]
fn a_photo_must_match_its_stated_size() {
    let pixels = vec![0u8; 4 * 4 * 4];
    assert!(Photo::new(&pixels, 4, 4).is_some());
    assert!(Photo::new(&pixels, 4, 5).is_none());
    assert!(Photo::new(&pixels, 0, 4).is_none());
    assert!(Photo::new(&[], 0, 0).is_none());
    assert!(Photo::new(&pixels, usize::MAX, 2).is_none());
}

/// Checks that everything in `expected` was found, of the right kind and
/// within `tolerance` millimetres on every edge, and nothing else was.
fn assert_matches(found: &[Obstruction], expected: &[Obstruction], tolerance: f64) {
    assert_eq!(
        found.len(),
        expected.len(),
        "found {found:#?}\nexpected {expected:#?}"
    );
    // Both lists run left to right.
    for (found, expected) in found.iter().zip(expected) {
        assert_eq!(found.kind, expected.kind, "{found:?} for {expected:?}");
        let (a, b) = (found.rect, expected.rect);
        let edges = [
            (a.x, b.x),
            (a.y, b.y),
            (a.right(), b.right()),
            (a.top(), b.top()),
        ];
        for (found_edge, expected_edge) in edges {
            let off = (found_edge - expected_edge).as_mm().abs();
            assert!(off <= tolerance, "{found:?} is {off} mm off {expected:?}");
        }
    }
}

#[test]
fn finds_what_is_on_a_wall_seen_square_on() {
    let expected = furnishings();
    let scene = render(SQUARE_ON, &expected);
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &scene.fractions());
    // A pixel of the photo is 6 mm of this wall.
    assert_matches(&found, &expected, 25.0);
}

#[test]
fn finds_what_is_on_a_wall_seen_at_an_angle() {
    let expected = furnishings();
    let scene = render(FROM_THE_LEFT, &expected);
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &scene.fractions());
    assert_matches(&found, &expected, 30.0);
}

#[test]
fn a_door_always_stands_on_the_floor() {
    let scene = render(SQUARE_ON, &furnishings());
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &scene.fractions());
    let door = found
        .iter()
        .find(|item| item.kind == ObstructionKind::Door)
        .expect("a door");
    assert_eq!(door.rect.y, Length::ZERO);
}

#[test]
fn everything_found_fits_on_the_wall() {
    // Corners that are a little out, as a user's would be.
    let scene = render(FROM_THE_LEFT, &furnishings());
    let mut corners = scene.fractions();
    corners[0].x -= 0.01;
    corners[2].y += 0.012;
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &corners);
    assert!(!found.is_empty());
    for item in &found {
        let rect = item.rect;
        assert!(
            rect.width.is_positive() && rect.height.is_positive(),
            "{item:?}"
        );
        assert!(rect.x >= Length::ZERO && rect.y >= Length::ZERO, "{item:?}");
        assert!(
            rect.right() <= WALL_WIDTH && rect.top() <= WALL_HEIGHT,
            "{item:?}"
        );
    }
}

#[test]
fn a_bare_wall_has_nothing_on_it() {
    let scene = render(SQUARE_ON, &[]);
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &scene.fractions());
    assert_eq!(found, Vec::new());
}

#[test]
fn corners_that_do_not_outline_a_wall_find_nothing() {
    let scene = render(SQUARE_ON, &furnishings());
    let mut crossed = scene.fractions();
    crossed.swap(0, 1);
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &crossed);
    assert_eq!(found, Vec::new());
}

#[test]
fn the_two_detectors_work_end_to_end() {
    // The outline from one feeds the other, as it does in the app.
    let expected = furnishings();
    let scene = render(FROM_THE_LEFT, &expected);
    let outline = find_wall(&scene.photo()).expect("an outline");
    let found = find_obstructions(&scene.photo(), WALL_WIDTH, WALL_HEIGHT, &outline.corners);
    assert_matches(&found, &expected, 45.0);
}
