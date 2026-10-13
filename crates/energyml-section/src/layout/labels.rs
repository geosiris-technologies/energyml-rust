//! Label placement without overlap, measured against a real font.
//!
//! The width of a label is `energyml_sheet::text::string_width_pt` - the
//! drilling sheet's own table of advances, not a second one
//! (ADR-0015). That function works in points, and a point is
//! only a size: asked at a size in pixels it answers in pixels.

use energyml_sheet::text;

/// The widest a label is allowed to grow, in pixels, before it is cut with "…".
pub const MAX_WIDTH_PX: f64 = 150.0;
/// Free space kept between two labels on the same level.
pub const GAP_PX: f64 = 4.0;

/// The width of a string at a font size, in pixels.
pub fn width_px(text: &str, size_px: f64) -> f64 {
    text::string_width_pt(text, size_px)
}

/// A line cut to `max` pixels, ending in "…" when it was cut.
pub fn fit(line: &str, size_px: f64, max: f64) -> String {
    if width_px(line, size_px) <= max {
        return line.to_string();
    }
    let ellipsis = "…";
    let mut out = String::new();
    for ch in line.chars() {
        let mut next = out.clone();
        next.push(ch);
        if width_px(&next, size_px) + width_px(ellipsis, size_px) > max {
            break;
        }
        out = next;
    }
    out.push_str(ellipsis);
    out
}

/// A rectangle on the drawing, in pixels: left, top, right, bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect2 {
    /// Left edge.
    pub x0: f64,
    /// Top edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Bottom edge.
    pub y1: f64,
}

impl Rect2 {
    /// Whether two rectangles overlap, `gap` pixels of air kept between them.
    pub fn hits(&self, other: &Rect2, gap: f64) -> bool {
        self.x0 < other.x1 + gap
            && other.x0 < self.x1 + gap
            && self.y0 < other.y1 + gap
            && other.y0 < self.y1 + gap
    }

    /// Whether this rectangle lies wholly inside `bounds`.
    pub fn within(&self, bounds: &Rect2) -> bool {
        self.x0 >= bounds.x0 && self.x1 <= bounds.x1 && self.y0 >= bounds.y0 && self.y1 <= bounds.y1
    }
}

/// Labels placed one after the other, each in the **first** of its positions
/// that stays inside the bounds and clear of every label placed before
/// (alternately above and below, or shifted progressively to the left or right,
/// with a reset once the following lines are far enough). The order of the positions is the
/// caller's - it is what a label *prefers* - and the reset comes by itself: a
/// label far from the others finds its first position free.
pub struct Placer {
    bounds: Rect2,
    taken: Vec<Rect2>,
}

impl Placer {
    /// An empty drawing area.
    pub fn new(bounds: Rect2) -> Self {
        Self {
            bounds,
            taken: Vec::new(),
        }
    }

    /// A place nothing may take - a text or a mark already drawn there.
    pub fn block(&mut self, rect: Rect2) {
        self.taken.push(rect);
    }

    /// The index of the first of `positions` that is free, taken; `None` when
    /// none is - the label is folded, and the caller counts it.
    pub fn place(&mut self, positions: &[Rect2]) -> Option<usize> {
        let at = positions
            .iter()
            .position(|r| r.within(&self.bounds) && !self.taken.iter().any(|t| t.hits(r, GAP_PX)))?;
        self.taken.push(positions[at]);
        Some(at)
    }
}
