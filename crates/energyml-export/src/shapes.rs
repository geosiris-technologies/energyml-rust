//! Choosing, per type, what form an export should take.
//!
//! # The problem this solves
//!
//! [`energyml_geometry::shape::Shape`] lets a caller ask for *one* object in a
//! particular form. But an export is usually a whole package — thirty-seven
//! representations of six different kinds, out of one command — and the answer
//! is not the same for all of them. Someone loading a package into QGIS wants
//! the horizons as polygons and the fault sticks as lines; someone building a
//! point-cloud index wants the nodes of everything.
//!
//! So the caller writes **rules**, and each rule is a pattern and a shape:
//!
//! ```text
//! --as Grid2d=surface --as TriangulatedSet=points --as '*=auto'
//! ```
//!
//! # Why substrings, and not a full glob or regex
//!
//! A rule matches when its pattern appears in the object's **qualified type** —
//! `resqml20.obj_Grid2dRepresentation`, `resqml22.TriangulatedSetRepresentation`
//! — case-insensitively. That one rule covers every level of precision anyone
//! actually needs:
//!
//! | Pattern | Matches |
//! |---|---|
//! | `Grid2d` | both releases' 2D grids |
//! | `resqml20.` | everything from RESQML 2.0.1 |
//! | `IjkGridRepresentation` | exactly that type |
//! | `*` | everything, as a final fallback |
//!
//! A glob or a regex would add escaping, anchoring and a class of typo that
//! fails **silently** — a pattern that matches nothing looks exactly like a
//! pattern whose objects are absent. Substrings have none of that, and
//! [`Selector`] is an enum precisely so a `Selector::Regex` can be added later
//! without changing a single caller or a single command line.
//!
//! # First match wins, and order is the caller's
//!
//! Rules are tried in the order they were given. That is what makes a general
//! rule and an exception composable in one line — `--as Grid2d=surface --as
//! '*=points'` means *grids as polygons, everything else as nodes* — and it is
//! the only ordering a person can predict without reading this file.
//!
//! # A rule that cannot be honoured is an error, not a shrug
//!
//! `--as PolylineSet=surface` does not quietly fall back to lines. The
//! reshaping refuses by name, the export of that object fails, and the caller is
//! told which of their rules met which object. A rule that silently does nothing
//! is worse than one that fails: it teaches the caller that it worked.

use energyml_geometry::shape::Shape;

/// Which objects a rule applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// Every object. Written `*`.
    Any,
    /// Objects whose qualified type contains this, case-insensitively.
    Contains(String),
}

impl Selector {
    /// Parse a pattern. `*` is [`Selector::Any`]; anything else is a substring.
    pub fn parse(pattern: &str) -> Selector {
        match pattern.trim() {
            "*" | "" => Selector::Any,
            other => Selector::Contains(other.to_ascii_lowercase()),
        }
    }

    /// Whether this selector picks that qualified type.
    pub fn matches(&self, qualified_type: &str) -> bool {
        match self {
            Selector::Any => true,
            Selector::Contains(needle) => {
                qualified_type.to_ascii_lowercase().contains(needle.as_str())
            }
        }
    }
}

impl std::fmt::Display for Selector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Selector::Any => f.write_str("*"),
            Selector::Contains(needle) => f.write_str(needle),
        }
    }
}

/// The rules, in the order the caller gave them.
///
/// Empty is the ordinary state and means [`Shape::Auto`] for everything, which
/// is what every export did before there was anything to ask.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shapes {
    rules: Vec<(Selector, Shape)>,
}

impl Shapes {
    /// No rules: every object in its natural shape.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a rule at the end, where it will be tried last.
    pub fn with(mut self, selector: Selector, shape: Shape) -> Self {
        self.rules.push((selector, shape));
        self
    }

    /// Parse one `PATTERN=SHAPE` rule and add it.
    ///
    /// The error says which half was not understood and what the alternatives
    /// are, because this is typed on a command line by someone who will not go
    /// looking for documentation to find out that `polygon` was spelled
    /// `polygons`.
    pub fn parse_rule(self, rule: &str) -> std::result::Result<Self, String> {
        let Some((pattern, shape)) = rule.split_once('=') else {
            return Err(format!(
                "`{rule}` is not a rule: write it as PATTERN=SHAPE, e.g. `Grid2d=surface`"
            ));
        };
        let Some(shape) = Shape::parse(shape) else {
            let names: Vec<&str> = Shape::all().iter().map(|s| s.name()).collect();
            return Err(format!(
                "`{shape}` is not a shape — try one of: {}",
                names.join(", ")
            ));
        };
        Ok(self.with(Selector::parse(pattern), shape))
    }

    /// The shape asked for this qualified type: the first rule that matches, or
    /// [`Shape::Auto`].
    pub fn shape_for(&self, qualified_type: &str) -> Shape {
        self.rules
            .iter()
            .find(|(selector, _)| selector.matches(qualified_type))
            .map(|(_, shape)| *shape)
            .unwrap_or_default()
    }

    /// Whether any rule was given at all.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The rules, for a caller echoing them back.
    pub fn rules(&self) -> &[(Selector, Shape)] {
        &self.rules
    }
}
