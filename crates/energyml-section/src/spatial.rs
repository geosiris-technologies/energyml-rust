//! A static R-tree over 2D boxes, with an overflow list for what is added after
//! the build (ADR-0010).
//!
//! **Written to move as is.** This module imports nothing from the rest of the
//! crate (nor any dependency): the design lets the heads and the spatial layer
//! be born in `energyml-section` and move down to the future
//! `energyml-cache` crate by copying this one file.
//!
//! # What it is
//!
//! - **Static, packed by STR** (sort-tile-recursive, Leutenegger et al. 1997):
//!   the boxes are sorted by the x of their centre, cut into vertical slices,
//!   each slice sorted by y and cut into nodes of [`FAN_OUT`] boxes; the nodes
//!   are packed the same way, level by level, up to one root. No box is ever
//!   moved after the build, so there is no balancing to get wrong.
//! - **An overflow list**: [`Index::insert`] after the build appends to a flat
//!   list scanned on every query; once it outgrows a share of the tree
//!   ([`Index::needs_rebuild`]) the next insert folds it in and rebuilds.
//! - **Exact by construction**: a query returns every item whose box meets the
//!   query box (closed boxes: touching counts), and **only** those - the same
//!   answer as a linear scan, in another order. The tests confront the two on
//!   random data. What the tree saves is the boxes it never opens, not any
//!   precision; a query by a polygon is its bounding box ([`Rect::of_points`]),
//!   then the caller's exact test on what comes back.
//!
//! A box with a non-finite bound is not indexable: [`Rect::new`] refuses it
//! (`None`), so the caller decides what such an item means rather than the tree
//! losing it silently.

/// Boxes per node. Eight keeps a 5 000-item tree four levels deep.
pub const FAN_OUT: usize = 8;

/// The overflow is folded into the tree when it holds more than this many
/// items **and** more than a quarter of the tree.
pub const OVERFLOW_FLOOR: usize = 64;

/// A closed 2D box: `min <= max` on both axes, both finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// The lower corner, `[x, y]`.
    pub min: [f64; 2],
    /// The upper corner, `[x, y]`.
    pub max: [f64; 2],
}

impl Rect {
    /// The box of two corners, in any order; `None` if a coordinate is not finite.
    pub fn new(a: [f64; 2], b: [f64; 2]) -> Option<Self> {
        if !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
            return None;
        }
        Some(Self {
            min: [a[0].min(b[0]), a[1].min(b[1])],
            max: [a[0].max(b[0]), a[1].max(b[1])],
        })
    }

    /// A point, as a degenerate box; `None` if not finite.
    pub fn point(p: [f64; 2]) -> Option<Self> {
        Self::new(p, p)
    }

    /// The bounding box of some points (a polygon's, for a query by polygon);
    /// `None` if there are none or one is not finite.
    pub fn of_points(points: &[[f64; 2]]) -> Option<Self> {
        let first = *points.first()?;
        let mut rect = Self::point(first)?;
        for p in &points[1..] {
            rect = rect.union(&Self::point(*p)?);
        }
        Some(rect)
    }

    /// This box grown by `margin` on every side (a negative margin shrinks it,
    /// and an emptied box stays a box at its centre).
    pub fn grown(&self, margin: f64) -> Self {
        let mut r = *self;
        for axis in 0..2 {
            r.min[axis] -= margin;
            r.max[axis] += margin;
            if r.min[axis] > r.max[axis] {
                let mid = (self.min[axis] + self.max[axis]) / 2.0;
                r.min[axis] = mid;
                r.max[axis] = mid;
            }
        }
        r
    }

    /// The smallest box holding both.
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])],
            max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])],
        }
    }

    /// The two boxes share a point (touching counts).
    pub fn intersects(&self, other: &Self) -> bool {
        self.min[0] <= other.max[0]
            && other.min[0] <= self.max[0]
            && self.min[1] <= other.max[1]
            && other.min[1] <= self.max[1]
    }

    fn centre(&self, axis: usize) -> f64 {
        (self.min[axis] + self.max[axis]) / 2.0
    }
}

#[derive(Clone, Debug)]
struct Node {
    rect: Rect,
    /// Indices into the level below: entries for a leaf, nodes otherwise.
    kids: Vec<usize>,
    leaf: bool,
}

/// The index: items of type `T` filed under their box.
#[derive(Clone, Debug)]
pub struct Index<T> {
    /// Every item, built and overflowed alike; `entries[..built]` are in the tree.
    entries: Vec<(Rect, T)>,
    built: usize,
    nodes: Vec<Node>,
    root: Option<usize>,
    rebuilds: usize,
}

impl<T> Default for Index<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            built: 0,
            nodes: Vec::new(),
            root: None,
            rebuilds: 0,
        }
    }
}

impl<T> Index<T> {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// An index built at once over `items`.
    pub fn build(items: Vec<(Rect, T)>) -> Self {
        let mut index = Self {
            entries: items,
            ..Self::default()
        };
        index.rebuild();
        index
    }

    /// How many items it holds, in the tree and in the overflow.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No item at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many items wait in the overflow list.
    pub fn overflow(&self) -> usize {
        self.entries.len() - self.built
    }

    /// How many times the tree was packed (the build counts).
    pub fn rebuilds(&self) -> usize {
        self.rebuilds
    }

    /// The overflow has outgrown its share: more than [`OVERFLOW_FLOOR`] items
    /// and more than a quarter of the tree.
    pub fn needs_rebuild(&self) -> bool {
        self.overflow() > OVERFLOW_FLOOR && self.overflow() * 4 > self.built
    }

    /// Add an item. It is found by the very next query; the tree is packed
    /// again when the overflow has outgrown its share ([`Self::needs_rebuild`]).
    pub fn insert(&mut self, rect: Rect, item: T) {
        self.entries.push((rect, item));
        if self.needs_rebuild() {
            self.rebuild();
        }
    }

    /// Pack every item into a fresh tree and empty the overflow.
    pub fn rebuild(&mut self) {
        self.rebuilds += 1;
        self.built = self.entries.len();
        self.nodes.clear();
        self.root = None;
        if self.entries.is_empty() {
            return;
        }
        // Level 0: the entries, tiled.
        let rects: Vec<Rect> = self.entries.iter().map(|(r, _)| *r).collect();
        let mut level: Vec<usize> = (0..rects.len()).collect();
        let mut leaf = true;
        let mut rects_of_level = rects;
        loop {
            let order = tile(&rects_of_level, FAN_OUT);
            let mut next_nodes: Vec<usize> = Vec::new();
            let mut next_rects: Vec<Rect> = Vec::new();
            for group in order.chunks(FAN_OUT) {
                let kids: Vec<usize> = group.iter().map(|&i| level[i]).collect();
                let rect = group
                    .iter()
                    .map(|&i| rects_of_level[i])
                    .reduce(|a, b| a.union(&b))
                    .expect("non-empty chunk");
                next_nodes.push(self.nodes.len());
                next_rects.push(rect);
                self.nodes.push(Node { rect, kids, leaf });
            }
            if next_nodes.len() == 1 {
                self.root = Some(next_nodes[0]);
                return;
            }
            level = next_nodes;
            rects_of_level = next_rects;
            leaf = false;
        }
    }

    /// Every item whose box meets `window`, the tree's and the overflow's - the
    /// set a linear scan gives, in no particular order.
    pub fn query(&self, window: &Rect) -> Vec<&T> {
        let mut out = Vec::new();
        self.visit(window, &mut |item| out.push(item));
        out
    }

    /// [`Self::query`], calling `found` for each instead of collecting.
    pub fn visit<'a>(&'a self, window: &Rect, found: &mut dyn FnMut(&'a T)) {
        if let Some(root) = self.root {
            let mut stack = vec![root];
            while let Some(at) = stack.pop() {
                let node = &self.nodes[at];
                if !node.rect.intersects(window) {
                    continue;
                }
                if node.leaf {
                    for &k in &node.kids {
                        let (rect, item) = &self.entries[k];
                        if rect.intersects(window) {
                            found(item);
                        }
                    }
                } else {
                    stack.extend(node.kids.iter().copied());
                }
            }
        }
        for (rect, item) in &self.entries[self.built..] {
            if rect.intersects(window) {
                found(item);
            }
        }
    }
}

/// The STR order of some boxes: by the x of the centre, cut into vertical
/// slices of `ceil(sqrt(n / fan)) * fan` boxes, each slice by the y of the
/// centre. Returns indices into `rects`.
fn tile(rects: &[Rect], fan: usize) -> Vec<usize> {
    let n = rects.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| rects[a].centre(0).total_cmp(&rects[b].centre(0)));
    let nodes = n.div_ceil(fan);
    let slices = (nodes as f64).sqrt().ceil() as usize;
    let per_slice = slices.max(1) * fan;
    for slice in order.chunks_mut(per_slice) {
        slice.sort_by(|&a, &b| rects[a].centre(1).total_cmp(&rects[b].centre(1)));
    }
    order
}
