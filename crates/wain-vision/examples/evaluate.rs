//! Scores `find_obstructions` against real photos whose contents have been
//! marked up by hand, and helps with doing that marking up.
//!
//! The photos are public ones from Wikimedia Commons. They are not kept in
//! the repository: `eval/walls.txt` names each one and records where the
//! wall is in it and what is on the wall, and `scripts/fetch-eval-photos.sh`
//! downloads them into `eval/photos/`.
//!
//! ```sh
//! scripts/fetch-eval-photos.sh
//! cargo run --release -p wain-vision --example evaluate -- score
//! cargo run --release -p wain-vision --example evaluate -- score d050 --draw /tmp/out
//! ```
//!
//! Marking up a new photo takes two steps, each of which writes a picture to
//! look at:
//!
//! ```sh
//! # The photo with a grid over it, to read the wall's corners off.
//! cargo run --release -p wain-vision --example evaluate -- grid photo.jpg grid.png
//! # The wall squared up with a millimetre grid, to read what is on it off.
//! cargo run --release -p wain-vision --example evaluate -- view d050 view.png
//! ```
//!
//! See `eval/walls.txt` for the file format.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage, RgbaImage};
use wain_core::{Homography, Length, Obstruction, ObstructionKind, Point, Rect};
use wain_vision::{Photo, find_obstructions, find_wall};

/// One wall in one photo, and what is really on it.
struct Wall {
    id: String,
    file: String,
    width: Length,
    height: Length,
    /// Fractions of the photo: top-left, top-right, bottom-right, bottom-left.
    corners: [Point; 4],
    truth: Vec<Obstruction>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("eval");
    match args.first().map(String::as_str) {
        Some("grid") if args.len() == 3 => grid(&args[1], &args[2]),
        Some("view") if args.len() == 3 => {
            let walls = read_walls(&root.join("walls.txt"));
            let wall = walls
                .iter()
                .find(|wall| wall.id == args[1])
                .unwrap_or_else(|| panic!("no wall called {}", args[1]));
            let photo = open(&root.join("photos").join(&wall.file));
            draw_view(wall, &photo, &[], 250).save(&args[2]).unwrap();
            println!("{}", describe_shape(wall, &photo));
        }
        Some("score") => {
            let draw = args
                .iter()
                .position(|arg| arg == "--draw")
                .map(|at| PathBuf::from(&args[at + 1]));
            let only: Vec<&String> = args[1..]
                .iter()
                .take_while(|arg| !arg.starts_with("--"))
                .collect();
            let walls = read_walls(&root.join("walls.txt"));
            score(&walls, &root.join("photos"), &only, draw.as_deref());
        }
        _ => {
            eprintln!("usage: evaluate grid <photo> <out.png>");
            eprintln!("       evaluate view <wall id> <out.png>");
            eprintln!("       evaluate score [<wall id>...] [--draw <dir>]");
            std::process::exit(2);
        }
    }
}

fn open(path: &Path) -> RgbaImage {
    let image = image::open(path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}. Run scripts/fetch-eval-photos.sh first.",
            path.display()
        )
    });
    // The web app hands over photos about this size.
    image.thumbnail(1024, 1024).to_rgba8()
}

fn kind_name(kind: ObstructionKind) -> &'static str {
    match kind {
        ObstructionKind::Door => "door",
        ObstructionKind::Opening => "opening",
        ObstructionKind::Window => "window",
        ObstructionKind::Outlet => "outlet",
        ObstructionKind::Switch => "switch",
        ObstructionKind::Vent => "vent",
        ObstructionKind::Other => "other",
    }
}

/// Reads `walls.txt`. Lines are `photo <id> <file name>`, then for each wall
/// in that photo `wall <width mm> <height mm> <four corners as x,y>`, then
/// one line for each thing on the wall: `<kind> <from left> <above floor>
/// <width> <height>` in millimetres.
fn read_walls(path: &Path) -> Vec<Wall> {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut walls: Vec<Wall> = Vec::new();
    let (mut id, mut file) = (String::new(), String::new());
    let mut count = 0;
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        let Some((word, rest)) = line.split_once(' ') else {
            continue;
        };
        let fail = |what: &str| -> ! { panic!("walls.txt line {}: {what}", number + 1) };
        let numbers = |text: &str| -> Vec<i64> {
            text.split_whitespace()
                .map(|value| {
                    value
                        .parse()
                        .unwrap_or_else(|_| fail("expected whole millimetres"))
                })
                .collect()
        };
        match word {
            "photo" => {
                // The rest of the line is the photo's name on Wikimedia
                // Commons, which only the download script needs.
                id = rest
                    .split(' ')
                    .next()
                    .unwrap_or_else(|| fail("photo <id> <file>"))
                    .to_owned();
                file = format!("{id}.jpg");
                count = 0;
            }
            "wall" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() != 6 {
                    fail("wall <width> <height> <4 corners>");
                }
                let size = numbers(&parts[..2].join(" "));
                let corners = std::array::from_fn(|index| {
                    let (x, y) = parts[2 + index]
                        .split_once(',')
                        .unwrap_or_else(|| fail("corner as x,y"));
                    Point::new(
                        x.parse().unwrap_or_else(|_| fail("corner x")),
                        y.parse().unwrap_or_else(|_| fail("corner y")),
                    )
                });
                count += 1;
                walls.push(Wall {
                    // A second wall in the same photo is `id.2`, and so on.
                    id: if count == 1 {
                        id.clone()
                    } else {
                        format!("{id}.{count}")
                    },
                    file: file.clone(),
                    width: Length::mm(size[0]),
                    height: Length::mm(size[1]),
                    corners,
                    truth: Vec::new(),
                });
            }
            kind => {
                let kind = match kind {
                    "door" => ObstructionKind::Door,
                    "opening" => ObstructionKind::Opening,
                    "window" => ObstructionKind::Window,
                    "outlet" => ObstructionKind::Outlet,
                    "switch" => ObstructionKind::Switch,
                    "other" => ObstructionKind::Other,
                    _ => fail("unknown kind"),
                };
                let n = numbers(rest);
                if n.len() != 4 {
                    fail("<kind> <x> <y> <width> <height>");
                }
                let wall = walls.last_mut().unwrap_or_else(|| fail("no wall yet"));
                wall.truth.push(Obstruction {
                    kind,
                    rect: Rect::new(
                        Length::mm(n[0]),
                        Length::mm(n[1]),
                        Length::mm(n[2]),
                        Length::mm(n[3]),
                    ),
                });
            }
        }
    }
    walls
}

// ---- Scoring ----------------------------------------------------------------

/// Doors, windows and plates are counted separately. An opening counts as a
/// door, and an outlet found as a switch (or the reverse) still counts as a
/// plate found: which one it is depends only on its height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Door,
    Window,
    Plate,
    /// Marked up so that finding it is not a mistake, but never expected.
    Ignored,
}

fn class(kind: ObstructionKind) -> Class {
    match kind {
        ObstructionKind::Door | ObstructionKind::Opening => Class::Door,
        ObstructionKind::Window => Class::Window,
        ObstructionKind::Outlet | ObstructionKind::Switch => Class::Plate,
        ObstructionKind::Vent | ObstructionKind::Other => Class::Ignored,
    }
}

fn overlap(a: &Rect, b: &Rect) -> f64 {
    let w = (a.right().min(b.right()) - a.x.max(b.x)).as_mm().max(0.0);
    let h = (a.top().min(b.top()) - a.y.max(b.y)).as_mm().max(0.0);
    w * h
}

fn area(rect: &Rect) -> f64 {
    rect.width.as_mm() * rect.height.as_mm()
}

/// Whether something found is the thing marked up: for doors and windows,
/// the two rectangles mostly coincide; for a plate, they touch.
fn same(found: &Obstruction, truth: &Obstruction) -> bool {
    let shared = overlap(&found.rect, &truth.rect);
    match class(truth.kind) {
        Class::Plate => class(found.kind) == Class::Plate && shared > 0.0,
        Class::Ignored => shared > 0.3 * area(&found.rect),
        wanted => {
            let union = area(&found.rect) + area(&truth.rect) - shared;
            class(found.kind) == wanted && shared / union >= 0.5
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Tally {
    found: usize,
    missed: usize,
    wrong: usize,
}

fn score(walls: &[Wall], photos: &Path, only: &[&String], draw: Option<&Path>) {
    let mut totals: BTreeMap<Class, Tally> = BTreeMap::new();
    let mut edge_errors: Vec<f64> = Vec::new();
    for wall in walls {
        if !only.is_empty() && !only.iter().any(|id| **id == wall.id) {
            continue;
        }
        let image = open(&photos.join(&wall.file));
        let (width, height) = image.dimensions();
        let photo = Photo::new(image.as_raw(), width as usize, height as usize).unwrap();
        let found = find_obstructions(&photo, wall.width, wall.height, &wall.corners);

        let mut matched = vec![false; wall.truth.len()];
        let mut notes: Vec<String> = Vec::new();
        let mut marks: Vec<(Rect, Rgb<u8>)> = Vec::new();
        for item in &found {
            let hit = wall
                .truth
                .iter()
                .enumerate()
                .position(|(index, truth)| !matched[index] && same(item, truth));
            match hit {
                Some(index) => {
                    matched[index] = true;
                    let truth = &wall.truth[index];
                    if class(truth.kind) != Class::Ignored {
                        totals.entry(class(truth.kind)).or_default().found += 1;
                        let (a, b) = (item.rect, truth.rect);
                        if class(truth.kind) != Class::Plate {
                            for (x, y) in [
                                (a.x, b.x),
                                (a.y, b.y),
                                (a.right(), b.right()),
                                (a.top(), b.top()),
                            ] {
                                edge_errors.push((x - y).as_mm().abs());
                            }
                        }
                    }
                    marks.push((item.rect, Rgb([0, 220, 60])));
                }
                None => {
                    totals.entry(class(item.kind)).or_default().wrong += 1;
                    notes.push(format!(
                        "wrong {} at {:.0},{:.0} {:.0}x{:.0}",
                        kind_name(item.kind),
                        item.rect.x.as_mm(),
                        item.rect.y.as_mm(),
                        item.rect.width.as_mm(),
                        item.rect.height.as_mm()
                    ));
                    marks.push((item.rect, Rgb([255, 30, 30])));
                }
            }
        }
        for (truth, hit) in wall.truth.iter().zip(&matched) {
            if !hit && class(truth.kind) != Class::Ignored {
                totals.entry(class(truth.kind)).or_default().missed += 1;
                notes.push(format!(
                    "missed {} at {:.0},{:.0}",
                    kind_name(truth.kind),
                    truth.rect.x.as_mm(),
                    truth.rect.y.as_mm()
                ));
                marks.push((truth.rect, Rgb([255, 230, 0])));
            }
        }
        let expected = wall
            .truth
            .iter()
            .filter(|truth| class(truth.kind) != Class::Ignored)
            .count();
        let hits = wall
            .truth
            .iter()
            .zip(&matched)
            .filter(|(truth, hit)| **hit && class(truth.kind) != Class::Ignored)
            .count();
        println!("{:8} {hits}/{expected}  {}", wall.id, notes.join("; "));
        if let Some(dir) = draw {
            std::fs::create_dir_all(dir).unwrap();
            draw_view(wall, &image, &marks, 500)
                .save(dir.join(format!("{}.png", wall.id)))
                .unwrap();
        }
    }

    println!();
    println!(
        "{:8} {:>6} {:>6} {:>6} {:>7} {:>10}",
        "", "found", "missed", "wrong", "recall", "precision"
    );
    let mut all = Tally::default();
    for (class, tally) in &totals {
        if *class == Class::Ignored {
            continue;
        }
        print_tally(&format!("{class:?}").to_lowercase(), tally);
        all.found += tally.found;
        all.missed += tally.missed;
        all.wrong += tally.wrong;
    }
    print_tally("all", &all);
    if !edge_errors.is_empty() {
        edge_errors.sort_by(f64::total_cmp);
        println!(
            "door and window edges: median {:.0} mm off, 90% within {:.0} mm",
            edge_errors[edge_errors.len() / 2],
            edge_errors[edge_errors.len() * 9 / 10]
        );
    }
}

fn print_tally(name: &str, tally: &Tally) {
    let share = |part: usize, whole: usize| {
        if whole == 0 {
            "-".to_owned()
        } else {
            format!("{:.0}%", 100.0 * part as f64 / whole as f64)
        }
    };
    println!(
        "{name:8} {:>6} {:>6} {:>6} {:>7} {:>10}",
        tally.found,
        tally.missed,
        tally.wrong,
        share(tally.found, tally.found + tally.missed),
        share(tally.found, tally.found + tally.wrong)
    );
}

// ---- Pictures for marking up --------------------------------------------------

/// The wall squared up, at 720 pixels on its longer side, with a grid every
/// `step` millimetres from the bottom-left corner (heavier and numbered, in
/// units of 100 mm, at every other line), the marked-up contents in white
/// and any `marks` on top.
fn draw_view(wall: &Wall, photo: &RgbaImage, marks: &[(Rect, Rgb<u8>)], step: i64) -> RgbImage {
    let (pw, ph) = photo.dimensions();
    let in_pixels = wall
        .corners
        .map(|p| Point::new(p.x * pw as f64, p.y * ph as f64));
    let homography =
        Homography::for_wall(wall.width, wall.height, &in_pixels).expect("corners outline a wall");
    let (width_mm, height_mm) = (wall.width.as_mm(), wall.height.as_mm());
    let scale = width_mm.max(height_mm) / 720.0;
    let (w, h) = (
        (width_mm / scale).ceil() as u32,
        (height_mm / scale).ceil() as u32,
    );
    // Twice the size, so lines and numbers do not hide what is under them.
    let zoom = 2;
    let mut out = RgbImage::new(w * zoom, h * zoom);
    for j in 0..h * zoom {
        for i in 0..w * zoom {
            let x = (i as f64 + 0.5) * scale / zoom as f64;
            let y = height_mm - (j as f64 + 0.5) * scale / zoom as f64;
            let p = homography.project(
                Length::from_nm((x * 1e6) as i64),
                Length::from_nm((y * 1e6) as i64),
            );
            let (u, v) = (p.x.round(), p.y.round());
            if u >= 0.0 && v >= 0.0 && u < pw as f64 && v < ph as f64 {
                let pixel = photo.get_pixel(u as u32, v as u32);
                out.put_pixel(i, j, Rgb([pixel[0], pixel[1], pixel[2]]));
            }
        }
    }
    let to_px = |x: f64, y: f64| {
        (
            x / scale * zoom as f64,
            (height_mm - y) / scale * zoom as f64,
        )
    };
    let grey = Rgb([150, 150, 150]);
    let mut at = 0;
    while (at as f64) < width_mm {
        let (x, _) = to_px(at as f64, 0.0);
        let heavy = at % (2 * step) == 0;
        dotted(
            &mut out,
            (x, 0.0),
            (x, (h * zoom) as f64),
            grey,
            if heavy { 2 } else { 6 },
        );
        if heavy {
            number(
                &mut out,
                x as u32 + 2,
                h * zoom - 14,
                (at / 100) as usize,
                Rgb([255, 255, 0]),
            );
        }
        at += step;
    }
    let mut at = 0;
    while (at as f64) < height_mm {
        let (_, y) = to_px(0.0, at as f64);
        let heavy = at % (2 * step) == 0;
        dotted(
            &mut out,
            (0.0, y),
            ((w * zoom) as f64, y),
            grey,
            if heavy { 2 } else { 6 },
        );
        if heavy && at > 0 {
            number(
                &mut out,
                2,
                y as u32 + 2,
                (at / 100) as usize,
                Rgb([255, 255, 0]),
            );
        }
        at += step;
    }
    let mut outline = |rect: &Rect, colour: Rgb<u8>, grow: f64| {
        let (x0, y0) = to_px(rect.x.as_mm(), rect.top().as_mm());
        let (x1, y1) = to_px(rect.right().as_mm(), rect.y.as_mm());
        let (x0, y0, x1, y1) = (x0 - grow, y0 - grow, x1 + grow, y1 + grow);
        for (a, b) in [
            ((x0, y0), (x1, y0)),
            ((x1, y0), (x1, y1)),
            ((x1, y1), (x0, y1)),
            ((x0, y1), (x0, y0)),
        ] {
            dotted(&mut out, a, b, colour, 1);
        }
    };
    for truth in &wall.truth {
        outline(&truth.rect, Rgb([255, 255, 255]), 0.0);
    }
    for (rect, colour) in marks {
        outline(rect, *colour, 2.0);
        outline(rect, *colour, 3.0);
    }
    out
}

/// What the corners say about the wall's proportions, for checking the
/// width and height written in `walls.txt`.
fn describe_shape(wall: &Wall, photo: &RgbaImage) -> String {
    let (w, h) = photo.dimensions();
    let pixels = wall.corners.map(|p| (p.x * w as f64, p.y * h as f64));
    match aspect_ratio(&pixels, w as f64, h as f64) {
        Some(ratio) => format!(
            "The corners suggest a wall {ratio:.2} times as wide as it is tall: {:.0} mm wide if it is {:.0} mm tall. walls.txt says {:.0} mm.",
            ratio * wall.height.as_mm(),
            wall.height.as_mm(),
            wall.width.as_mm()
        ),
        None => "The corners do not pin down the wall's proportions.".to_owned(),
    }
}

/// The width-to-height ratio of a rectangle from its four corners in a
/// photo (top-left, top-right, bottom-right, bottom-left, in pixels),
/// assuming the lens points at the middle of the picture. From Zhang and
/// He, "Whiteboard scanning and image enhancement" (2007).
fn aspect_ratio(corners: &[(f64, f64); 4], width: f64, height: f64) -> Option<f64> {
    let centred = |(x, y): (f64, f64)| [x - width / 2.0, y - height / 2.0, 1.0];
    // The paper's order: two corners along the width, then the two opposite.
    let (m1, m2, m3, m4) = (
        centred(corners[3]),
        centred(corners[2]),
        centred(corners[0]),
        centred(corners[1]),
    );
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let k2 = dot(cross(m1, m4), m3) / dot(cross(m2, m4), m3);
    let k3 = dot(cross(m1, m4), m2) / dot(cross(m3, m4), m2);
    let n2 = [k2 * m2[0] - m1[0], k2 * m2[1] - m1[1], k2 - 1.0];
    let n3 = [k3 * m3[0] - m1[0], k3 * m3[1] - m1[1], k3 - 1.0];
    let flat = |n: [f64; 3]| n[0] * n[0] + n[1] * n[1];
    let ratio = if n2[2].abs() < 1e-4 || n3[2].abs() < 1e-4 {
        // Seen square-on along one direction; the focal length drops out.
        flat(n2) / flat(n3)
    } else {
        let focal_squared = -(n2[0] * n3[0] + n2[1] * n3[1]) / (n2[2] * n3[2]);
        if focal_squared <= 0.0 {
            return None;
        }
        (flat(n2) / focal_squared + n2[2] * n2[2]) / (flat(n3) / focal_squared + n3[2] * n3[2])
    };
    (ratio.is_finite() && ratio > 0.0).then(|| ratio.sqrt())
}

/// The photo with a grid every tenth of its width and height, numbered 1 to
/// 9, and the outline `find_wall` picks drawn in green.
fn grid(input: &str, output: &str) {
    let photo = open(Path::new(input));
    let (w, h) = photo.dimensions();
    let mut out = image::DynamicImage::ImageRgba8(photo.clone()).to_rgb8();
    for tenth in 1..10 {
        let (x, y) = ((w * tenth / 10) as f64, (h * tenth / 10) as f64);
        dotted(&mut out, (x, 0.0), (x, h as f64), Rgb([255, 255, 0]), 3);
        dotted(&mut out, (0.0, y), (w as f64, y), Rgb([255, 255, 0]), 3);
        number(
            &mut out,
            x as u32 + 2,
            2,
            tenth as usize,
            Rgb([255, 255, 0]),
        );
        number(
            &mut out,
            2,
            y as u32 + 2,
            tenth as usize,
            Rgb([255, 255, 0]),
        );
    }
    let found = find_wall(&Photo::new(photo.as_raw(), w as usize, h as usize).unwrap());
    if let Some(outline) = found {
        let corners = outline.corners.map(|p| (p.x * w as f64, p.y * h as f64));
        for index in 0..4 {
            dotted(
                &mut out,
                corners[index],
                corners[(index + 1) % 4],
                Rgb([0, 230, 60]),
                1,
            );
        }
        let text: Vec<String> = outline
            .corners
            .iter()
            .map(|p| format!("{:.3},{:.3}", p.x, p.y))
            .collect();
        println!("outline {}  sides {:?}", text.join(" "), outline.sides);
    } else {
        println!("no outline found");
    }
    out.save(output).unwrap();
}

/// A line with every `every`th pixel drawn (1 for solid).
fn dotted(canvas: &mut RgbImage, a: (f64, f64), b: (f64, f64), colour: Rgb<u8>, every: i64) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0) as i64;
    for step in (0..=steps).filter(|step| step % every == 0) {
        let t = step as f64 / steps as f64;
        let (x, y) = (
            (a.0 + (b.0 - a.0) * t).round(),
            (a.1 + (b.1 - a.1) * t).round(),
        );
        if x >= 0.0 && y >= 0.0 && x < canvas.width() as f64 && y < canvas.height() as f64 {
            canvas.put_pixel(x as u32, y as u32, colour);
        }
    }
}

/// Draws a whole number in a small blocky typeface on a black backing.
fn number(canvas: &mut RgbImage, x: u32, y: u32, value: usize, colour: Rgb<u8>) {
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    let text = value.to_string();
    let mut put = |px: u32, py: u32, colour: Rgb<u8>| {
        if px < canvas.width() && py < canvas.height() {
            canvas.put_pixel(px, py, colour);
        }
    };
    for dy in 0..12 {
        for dx in 0..text.len() as u32 * 8 + 2 {
            put(x + dx, y + dy, Rgb([0, 0, 0]));
        }
    }
    for (index, digit) in text.bytes().enumerate() {
        for (row, bits) in DIGITS[(digit - b'0') as usize].iter().enumerate() {
            for col in 0..3 {
                if bits >> (2 - col) & 1 == 1 {
                    for (a, b) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        put(
                            x + 1 + index as u32 * 8 + col * 2 + a,
                            y + 1 + row as u32 * 2 + b,
                            colour,
                        );
                    }
                }
            }
        }
    }
}
