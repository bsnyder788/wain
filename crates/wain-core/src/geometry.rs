//! Rectangles on a wall elevation.

use serde::{Deserialize, Serialize};

use crate::units::Length;

/// An axis-aligned rectangle on a wall, seen face-on. The origin is the
/// wall's bottom-left corner at floor level; `x` runs right and `y` runs up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Rect {
    pub x: Length,
    pub y: Length,
    pub width: Length,
    pub height: Length,
}

impl Rect {
    pub const fn new(x: Length, y: Length, width: Length, height: Length) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> Length {
        self.x + self.width
    }

    pub fn top(&self) -> Length {
        self.y + self.height
    }

    /// True when the two rectangles share interior area. Rectangles that only
    /// touch along an edge do not intersect.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.top()
            && other.y < self.top()
    }

    /// Grows the rectangle by `by` on every side.
    pub fn inflate(&self, by: Length) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            width: self.width + by * 2,
            height: self.height + by * 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i64, y: i64, w: i64, h: i64) -> Rect {
        Rect::new(Length::mm(x), Length::mm(y), Length::mm(w), Length::mm(h))
    }

    #[test]
    fn touching_edges_do_not_intersect() {
        let a = rect(0, 0, 10, 10);
        assert!(!a.intersects(&rect(10, 0, 10, 10)));
        assert!(!a.intersects(&rect(0, 10, 10, 10)));
        assert!(a.intersects(&rect(9, 9, 10, 10)));
        assert!(a.intersects(&rect(2, 2, 2, 2)));
    }

    #[test]
    fn inflating_closes_a_gap() {
        let a = rect(0, 0, 10, 10);
        let b = rect(12, 0, 10, 10);
        assert!(!a.intersects(&b));
        assert!(a.inflate(Length::mm(3)).intersects(&b));
    }
}
