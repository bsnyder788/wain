//! Runs detection on a photo and writes an annotated copy, for seeing how
//! the detectors do on real rooms and for tuning them.
//!
//! ```sh
//! cargo run --release -p wain-vision --example detect -- room.jpg out.png
//! cargo run --release -p wain-vision --example detect -- room.jpg out.png 3600 2400
//! cargo run --release -p wain-vision --example detect -- room.jpg out.png 3600 2400 \
//!     0.1,0.1 0.9,0.12 0.9,0.8 0.1,0.82
//! ```
//!
//! Every line considered is drawn in thin yellow and the chosen outline in
//! green, with sides that fell back to the photo's border in red. Given the
//! wall's width and height in millimetres, whatever is found on the wall is
//! drawn too: doors in blue, windows in cyan, outlets in magenta and
//! switches in orange. Four corners (top-left, top-right, bottom-right,
//! bottom-left, as fractions of the photo) replace the detected outline.

use std::time::Instant;

use image::{Rgb, RgbImage};
use wain_core::{Homography, Length, ObstructionKind, Point};
use wain_vision::{Photo, find_obstructions, trace_wall};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, output, rest @ ..] = args.as_slice() else {
        eprintln!("usage: detect <photo> <annotated.png> [<width mm> <height mm> [<4 corners>]]");
        std::process::exit(2);
    };
    let size = match rest {
        [width, height, ..] => Some((
            Length::mm(width.parse().expect("width in mm")),
            Length::mm(height.parse().expect("height in mm")),
        )),
        _ => None,
    };
    let given: Option<[Point; 4]> = rest.get(2..6).map(|corners| {
        std::array::from_fn(|index| {
            let (x, y) = corners[index].split_once(',').expect("corner as x,y");
            Point::new(x.parse().expect("x"), y.parse().expect("y"))
        })
    });

    let source = image::open(input).expect("could not read the photo");
    // The web app hands over photos about this size.
    let source = source.thumbnail(1024, 1024).to_rgba8();
    let (width, height) = source.dimensions();
    let photo = Photo::new(source.as_raw(), width as usize, height as usize).unwrap();

    let started = Instant::now();
    let trace = trace_wall(&photo);
    println!("wall: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);

    let mut canvas = image::DynamicImage::ImageRgba8(source.clone()).to_rgb8();
    let pixel = |p: Point| (p.x * width as f64, p.y * height as f64);
    for [a, b] in &trace.lines {
        draw_line(&mut canvas, pixel(*a), pixel(*b), Rgb([255, 220, 0]), 0);
    }
    match trace.outline {
        Some(outline) => {
            println!("outline: {:?}", outline);
            let sides = outline.sides;
            let found = [sides.top, sides.right, sides.bottom, sides.left];
            for (index, on_edge) in found.into_iter().enumerate() {
                let colour = if on_edge {
                    Rgb([0, 230, 60])
                } else {
                    Rgb([255, 40, 40])
                };
                let (a, b) = (outline.corners[index], outline.corners[(index + 1) % 4]);
                draw_line(&mut canvas, pixel(a), pixel(b), colour, 2);
            }
        }
        None => println!("outline: none"),
    }

    let corners = given.or(trace.outline.map(|outline| outline.corners));
    if let (Some((wall_width, wall_height)), Some(corners)) = (size, corners) {
        let started = Instant::now();
        let found = find_obstructions(&photo, wall_width, wall_height, &corners);
        println!(
            "obstructions: {:.1} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );

        let in_pixels = corners.map(|p| Point::new(p.x * width as f64, p.y * height as f64));
        if given.is_some() {
            for index in 0..4 {
                let (a, b) = (in_pixels[index], in_pixels[(index + 1) % 4]);
                draw_line(&mut canvas, (a.x, a.y), (b.x, b.y), Rgb([255, 255, 255]), 1);
            }
        }
        let homography = Homography::for_wall(wall_width, wall_height, &in_pixels).unwrap();
        for obstruction in &found {
            let rect = obstruction.rect;
            println!(
                "  {:?}: {:.0} mm from left, {:.0} mm up, {:.0} x {:.0} mm",
                obstruction.kind,
                rect.x.as_mm(),
                rect.y.as_mm(),
                rect.width.as_mm(),
                rect.height.as_mm()
            );
            let colour = match obstruction.kind {
                ObstructionKind::Door | ObstructionKind::Opening => Rgb([40, 90, 255]),
                ObstructionKind::Window => Rgb([0, 230, 230]),
                ObstructionKind::Outlet => Rgb([255, 0, 255]),
                _ => Rgb([255, 140, 0]),
            };
            let ring = [
                homography.project(rect.x, rect.y),
                homography.project(rect.right(), rect.y),
                homography.project(rect.right(), rect.top()),
                homography.project(rect.x, rect.top()),
            ];
            for index in 0..4 {
                let (a, b) = (ring[index], ring[(index + 1) % 4]);
                draw_line(&mut canvas, (a.x, a.y), (b.x, b.y), colour, 1);
            }
        }
    }
    canvas
        .save(output)
        .expect("could not write the annotated image");
}

/// Draws a line `2 * half_width + 1` pixels thick, clipped to the image.
fn draw_line(
    canvas: &mut RgbImage,
    a: (f64, f64),
    b: (f64, f64),
    colour: Rgb<u8>,
    half_width: i64,
) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0) as i64;
    let (w, h) = (canvas.width() as i64, canvas.height() as i64);
    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for dy in -half_width..=half_width {
            for dx in -half_width..=half_width {
                let (px, py) = (x.round() as i64 + dx, y.round() as i64 + dy);
                if (0..w).contains(&px) && (0..h).contains(&py) {
                    canvas.put_pixel(px as u32, py as u32, colour);
                }
            }
        }
    }
}
