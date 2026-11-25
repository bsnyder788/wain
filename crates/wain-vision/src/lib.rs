//! Looks at a photo of a wall and finds the wall in it.
//!
//! Two questions are answered, both with classical image processing (edge
//! detection, line fitting, perspective correction) rather than a trained
//! model, so it runs in the browser in a few milliseconds and the photo
//! never has to leave the device:
//!
//! - [`find_wall`]: where are the wall's four corners in the photo?
//! - [`find_obstructions`]: once the corners are known, where on the wall
//!   are its doors, windows, outlets and switches?
//!
//! Both are best guesses for the user to check, not measurements.

mod lines;
mod obstructions;
mod raster;
mod wall;

use wain_core::{Length, Obstruction, Point};

pub use wall::{SidesFound, WallOutline, WallTrace};

/// A photo's pixels: 8-bit RGBA, row by row from the top-left.
#[derive(Clone, Copy)]
pub struct Photo<'a> {
    rgba: &'a [u8],
    width: usize,
    height: usize,
}

impl<'a> Photo<'a> {
    /// `None` when the buffer is not `width * height` RGBA pixels, or the
    /// photo is empty.
    pub fn new(rgba: &'a [u8], width: usize, height: usize) -> Option<Photo<'a>> {
        let bytes = width.checked_mul(height)?.checked_mul(4)?;
        (width > 0 && height > 0 && rgba.len() == bytes).then_some(Photo {
            rgba,
            width,
            height,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

/// Finds the wall's outline in a photo. `None` when no outline stands out,
/// for instance in a close-up with no ceiling, floor or corner in view.
pub fn find_wall(photo: &Photo) -> Option<WallOutline> {
    wall::find(photo)
}

/// Finds what is on a wall: doors, windows, outlets and switches, placed in
/// the wall's own measurements.
///
/// `corners` are the wall's corners in the photo, ordered top-left,
/// top-right, bottom-right, bottom-left, as fractions of the photo's width
/// and height; `wall_width` and `wall_height` are its measured size. Returns
/// nothing when the corners do not outline a wall.
///
/// Sizes come from the photo, so they are only as good as the corners and
/// the measurements, and anything rectangular can be mistaken for a window.
/// Treat the results as suggestions.
pub fn find_obstructions(
    photo: &Photo,
    wall_width: Length,
    wall_height: Length,
    corners: &[Point; 4],
) -> Vec<Obstruction> {
    obstructions::find(photo, wall_width, wall_height, corners)
}

/// As [`find_wall`], also returning every line that was considered.
#[doc(hidden)]
pub fn trace_wall(photo: &Photo) -> WallTrace {
    wall::trace(photo)
}
