//! Validation.
//!
//! The derive macro emits *structural* validation: cardinality from
//! `minOccurs`/`maxOccurs` and the XSD facets carried in [`FieldInfo`].
//! Business rules are added separately as [`ValidationRule`]s, which is the
//! extension point the second phase of the project builds on (checking that a
//! referenced object exists in the storage, that a DOR points at an allowed
//! type, and so on).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::reflect::{Facets, FieldInfo, TypeInfo};
use crate::value::{FieldValue, Reflect, Scalar};

/// How serious an issue is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The document violates the schema or a business rule.
    Error,
    /// Suspicious but not invalid.
    Warning,
    /// Informational only.
    Info,
}

/// One validation finding.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    /// Dotted path to the offending value, e.g. `Citation.Title`.
    pub path: String,
    /// Severity.
    pub severity: Severity,
    /// Stable machine-readable code, e.g. `missing-required`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
}

/// The result of validating an object.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Every issue found, in document order.
    pub issues: Vec<Issue>,
}

impl ValidationReport {
    /// Whether the object is free of [`Severity::Error`] issues.
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    /// Only the errors.
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }
}

/// A business rule applied to every object visited during validation.
pub trait ValidationRule {
    /// Stable name, used in issue codes and for enabling/disabling rules.
    fn name(&self) -> &str;

    /// Whether this rule applies to the given type. Defaults to all types.
    fn applies_to(&self, _info: &'static TypeInfo) -> bool {
        true
    }

    /// Inspect one object and report through the context.
    fn check(&self, object: &dyn Reflect, ctx: &mut ValidationCtx<'_>);
}

/// Collects issues while walking an object graph.
pub struct ValidationCtx<'r> {
    path: Vec<String>,
    issues: Vec<Issue>,
    rules: &'r [Box<dyn ValidationRule>],
}

impl<'r> ValidationCtx<'r> {
    /// A context with no business rules.
    pub fn new() -> Self {
        Self {
            path: Vec::new(),
            issues: Vec::new(),
            rules: &[],
        }
    }

    /// A context applying the given business rules.
    pub fn with_rules(rules: &'r [Box<dyn ValidationRule>]) -> Self {
        Self {
            path: Vec::new(),
            issues: Vec::new(),
            rules,
        }
    }

    /// Current dotted path.
    pub fn path(&self) -> String {
        self.path.join(".")
    }

    /// Descend into a named field or indexed element.
    pub fn enter(&mut self, segment: impl Into<String>) {
        self.path.push(segment.into());
    }

    /// Descend into element `index` of the field just entered.
    ///
    /// The index is appended to the current segment - `TrianglePatch` becomes
    /// `TrianglePatch[1]` - rather than pushed as one of its own, because
    /// [`crate::path`] reads `a.b[1].c` and rejects `a.b.[1].c`. A path a
    /// caller cannot follow back to the value is a message that cannot be hung
    /// on the widget showing it, which is the only reason paths are recorded.
    ///
    /// Balance it with [`Self::leave_index`], not [`Self::leave`].
    pub fn enter_index(&mut self, index: usize) {
        match self.path.last_mut() {
            Some(last) => {
                use std::fmt::Write as _;
                let _ = write!(last, "[{index}]");
            }
            // A bare collection validated as the root has no field to attach
            // the index to. No document object has this shape; recording the
            // index alone still beats losing it.
            None => self.path.push(format!("[{index}]")),
        }
    }

    /// Undo the innermost [`Self::enter_index`].
    pub fn leave_index(&mut self) {
        match self.path.last_mut() {
            Some(last) if last.ends_with(']') => match last.rfind('[') {
                // `open == 0` is the rootless case above: the segment is the
                // index and nothing else, so it goes rather than empties.
                Some(0) | None => {
                    self.path.pop();
                }
                Some(open) => last.truncate(open),
            },
            _ => {
                self.path.pop();
            }
        }
    }

    /// Leave the current segment.
    pub fn leave(&mut self) {
        self.path.pop();
    }

    /// Record an issue at the current path.
    pub fn issue(&mut self, severity: Severity, code: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            path: self.path(),
            severity,
            code: code.into(),
            message: message.into(),
        });
    }

    /// Record an error at the current path.
    pub fn error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.issue(Severity::Error, code, message);
    }

    /// Record a warning at the current path.
    pub fn warn(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.issue(Severity::Warning, code, message);
    }

    /// Run the registered business rules against one object.
    pub fn run_rules(&mut self, object: &dyn Reflect) {
        // Copy the `'r` slice reference out of `self` first, so iterating it
        // does not keep `self` borrowed while the callback needs it mutably.
        let rules: &'r [Box<dyn ValidationRule>] = self.rules;
        for rule in rules {
            if rule.applies_to(object.type_info()) {
                rule.check(object, self);
            }
        }
    }

    /// Check a field's cardinality and facets.
    pub fn check_field(&mut self, info: &FieldInfo, value: &FieldValue<'_>) {
        let n = value.occurrences();
        if n < info.occurs.min as usize {
            self.enter(info.xml_name);
            self.error(
                "missing-required",
                format!(
                    "`{}` requires at least {} occurrence(s), found {}",
                    info.xml_name, info.occurs.min, n
                ),
            );
            self.leave();
        }
        if !info.occurs.max.allows(n) {
            self.enter(info.xml_name);
            self.error(
                "too-many-occurrences",
                format!(
                    "`{}` allows at most {:?} occurrence(s), found {}",
                    info.xml_name, info.occurs.max, n
                ),
            );
            self.leave();
        }

        if info.facets.is_empty() {
            return;
        }
        match value {
            FieldValue::Scalar(s) => {
                self.enter(info.xml_name);
                self.check_facets(&info.facets, *s);
                self.leave();
            }
            FieldValue::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if let FieldValue::Scalar(s) = item {
                        self.enter(format!("{}[{}]", info.xml_name, i));
                        self.check_facets(&info.facets, *s);
                        self.leave();
                    }
                }
            }
            _ => {}
        }
    }

    /// Check one scalar against a facet set.
    pub fn check_facets(&mut self, facets: &Facets, value: Scalar<'_>) {
        if let Some(n) = value.as_f64() {
            if let Some(min) = facets.min_inclusive
                && n < min
            {
                self.error("min-inclusive", format!("{n} is below minInclusive {min}"));
            }
            if let Some(max) = facets.max_inclusive
                && n > max
            {
                self.error("max-inclusive", format!("{n} is above maxInclusive {max}"));
            }
            if let Some(min) = facets.min_exclusive
                && n <= min
            {
                self.error("min-exclusive", format!("{n} is not above minExclusive {min}"));
            }
            if let Some(max) = facets.max_exclusive
                && n >= max
            {
                self.error("max-exclusive", format!("{n} is not below maxExclusive {max}"));
            }
        }
        if let Some(fixed) = facets.fixed {
            let same = match value {
                Scalar::Str(s) => s == fixed,
                Scalar::Bool(b) => matches!((b, fixed), (true, "true" | "1") | (false, "false" | "0")),
                Scalar::Int(n) => fixed.trim().parse::<i64>().is_ok_and(|f| f == n),
                Scalar::UInt(n) => fixed.trim().parse::<u64>().is_ok_and(|f| f == n),
                Scalar::Double(n) => fixed.trim().parse::<f64>().is_ok_and(|f| f == n),
            };
            if !same {
                self.error(
                    "fixed",
                    format!(
                        "the schema fixes this value to `{fixed}`, found `{}`",
                        scalar_text(value)
                    ),
                );
            }
        }
        if let Some(kind) = facets.lexical
            && let Some(s) = value.as_str()
            && let Err(why) = crate::lexical::check(kind, s)
        {
            self.error(format!("lexical-{kind}"), why);
        }
        if let Some(s) = value.as_str() {
            let len = s.chars().count() as u32;
            if let Some(min) = facets.min_length
                && len < min
            {
                self.error("min-length", format!("length {len} is below minLength {min}"));
            }
            if let Some(max) = facets.max_length
                && len > max
            {
                self.error("max-length", format!("length {len} exceeds maxLength {max}"));
            }
            if let Some(pattern) = facets.pattern {
                match pattern_matches(pattern, s) {
                    Some(true) => {}
                    Some(false) => {
                        self.error("pattern", format!("`{s}` does not match pattern `{pattern}`"));
                    }
                    // Not a document error, and not a pass either: the pattern
                    // could not be compiled, so this value was **not checked**.
                    None => self.warn(
                        "pattern-uncheckable",
                        format!("the schema pattern `{pattern}` is not supported by the regex engine; `{s}` was not checked"),
                    ),
                }
            }
        }
    }

    /// Finish and take the report.
    pub fn finish(self) -> ValidationReport {
        ValidationReport { issues: self.issues }
    }
}

impl Default for ValidationCtx<'_> {
    fn default() -> Self {
        Self::new()
    }
}

fn scalar_text(value: Scalar<'_>) -> String {
    match value {
        Scalar::Str(s) => s.to_string(),
        Scalar::Bool(b) => b.to_string(),
        Scalar::Int(n) => n.to_string(),
        Scalar::UInt(n) => n.to_string(),
        Scalar::Double(n) => n.to_string(),
    }
}

/// Whether a schema pattern compiles. XSD patterns are implicitly anchored, so
/// the engine is given `^(?:pattern)$`. A test compiles every pattern the
/// generated crates carry (`energyml-tests/tests/validation_audit.rs`), so that
/// a pattern the engine cannot run is found at once rather than by a value
/// that was never checked.
pub fn pattern_compiles(pattern: &str) -> bool {
    Regex::new(&format!("^(?:{pattern})$")).is_ok()
}

/// `Some(matches)`, or `None` when the pattern does not compile. Compiled
/// patterns are cached because the same facet is checked once per value in a
/// large model.
fn pattern_matches(pattern: &'static str, value: &str) -> Option<bool> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Option<Regex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = match cache.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entry = guard
        .entry(pattern)
        .or_insert_with(|| Regex::new(&format!("^(?:{pattern})$")).ok());
    entry.as_ref().map(|re| re.is_match(value))
}

/// Structural validation, implemented by every generated type.
pub trait Validate {
    /// Validate into an existing context.
    fn validate_into(&self, ctx: &mut ValidationCtx<'_>);

    /// Validate standalone, with no business rules.
    fn validate(&self) -> ValidationReport
    where
        Self: Sized,
    {
        let mut ctx = ValidationCtx::new();
        self.validate_into(&mut ctx);
        ctx.finish()
    }
}

/// Validate an object with a set of business rules applied.
pub fn validate_with(object: &dyn Validate, rules: &[Box<dyn ValidationRule>]) -> ValidationReport {
    let mut ctx = ValidationCtx::with_rules(rules);
    object.validate_into(&mut ctx);
    ctx.finish()
}

impl<T: Validate> Validate for Option<T> {
    fn validate_into(&self, ctx: &mut ValidationCtx<'_>) {
        if let Some(v) = self {
            v.validate_into(ctx);
        }
    }
}

impl<T: Validate> Validate for Vec<T> {
    fn validate_into(&self, ctx: &mut ValidationCtx<'_>) {
        for (i, v) in self.iter().enumerate() {
            ctx.enter_index(i);
            v.validate_into(ctx);
            ctx.leave_index();
        }
    }
}

impl<T: Validate> Validate for Box<T> {
    fn validate_into(&self, ctx: &mut ValidationCtx<'_>) {
        (**self).validate_into(ctx);
    }
}

macro_rules! impl_validate_noop {
    ($($ty:ty),* $(,)?) => {
        $(impl Validate for $ty {
            fn validate_into(&self, _ctx: &mut ValidationCtx<'_>) {}
        })*
    };
}

// Facets on primitives are checked by the owning struct, which is the only
// place that knows which facets apply.
impl_validate_noop!(bool, i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, String);
