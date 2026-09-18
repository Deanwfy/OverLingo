use serde::{Deserialize, Serialize};

/// In the shell's screen units: points on macOS, physical pixels on Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn inset(&self, amount: f64) -> Rect {
        Rect {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - 2.0 * amount).max(0.0),
            height: (self.height - 2.0 * amount).max(0.0),
        }
    }

    /// Whether at least `by` units overlap along both axes.
    pub fn overlaps_by(&self, other: &Rect, by: f64) -> bool {
        let span = |a: f64, a_len: f64, b: f64, b_len: f64| (a + a_len).min(b + b_len) - a.max(b);
        span(self.x, self.width, other.x, other.width) >= by
            && span(self.y, self.height, other.y, other.height) >= by
    }

    /// Equal within the rounding the window system applies.
    pub fn near(&self, other: &Rect) -> bool {
        (self.x - other.x).abs() < 1.0
            && (self.y - other.y).abs() < 1.0
            && (self.width - other.width).abs() < 1.0
            && (self.height - other.height).abs() < 1.0
    }

    pub fn is_finite(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
    }
}
