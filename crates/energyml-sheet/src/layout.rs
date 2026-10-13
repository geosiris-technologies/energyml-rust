//! The drilling sheet's page layout — pure data, no package read.
//!
//! The sheet design is the specification this module answers to; every
//! non-obvious choice below states the rule that decided it. This module turns a [`Board`] (what to draw:
//! an ordered list of [`Track`]s) and [`Params`] (how big, how much text) into
//! a [`Layout`] — sections with their vertical positions, cells already
//! wrapped into lines, ticks, and the notes a caller or a legend must show.
//! [`crate::svg`] is the only thing that turns a [`Layout`] into bytes; this
//! module is what gets **tested**, in numbers.
//!
//! # What is deliberately not here
//!
//! Reading an EPC — the frame, the strata, the property, the CBT — is
//! `crate::read`, not written yet. Nothing in
//! this module opens a package or a value array; a [`Board`] is exactly as
//! much data as its caller hands over.
//!
//! # The two registries
//!
//! A [`ProportionMode`] turns sections into vertical positions
//! ([`ModeRegistry`], built-ins [`ToScale`] and [`Readable`]); a
//! [`TrackRender`] turns one interval into what a cell shows
//! ([`TrackRegistry`], built-ins [`Fill`] and [`TextRender`]). Both follow the
//! repository's rule: a trait, a registry, the built-ins pushed in by
//! default, and [`ModeRegistry::register`] / [`TrackRegistry::register`] put
//! a caller's addition **first**, so it is tried before — and can replace — a
//! built-in of the same id.

use energyml_domain::color::Rgba;

// ---------------------------------------------------------------------------
// Constants that are this crate's own choice, not the specification's
// ---------------------------------------------------------------------------

/// 1 point = 1/72 inch, and 1 inch = 25.4 mm exactly — a physical fact, not a
/// choice. `pub(crate)`: [`crate::svg`] needs the same conversion for its own
/// (unrelated) horizontal-layout heuristic.
pub(crate) const PT_TO_MM: f64 = 25.4 / 72.0;

/// The leading (line height ÷ font size) this crate draws text at. Common
/// typographic practice, not a number the specification states; revisit once a
/// real sheet has been proof-read against a printed page.
const LINE_HEIGHT_FACTOR: f64 = 1.2;

/// Half a line of white space a cell keeps above and below its text, so
/// letters with descenders never touch a section's boundary line. This
/// crate's own choice, not the specification's.
const CELL_MARGIN_LINES: f64 = 0.5;

/// How close two measured depths must be, in metres, to be treated as the
/// same boundary — the corpus's coordinates are `f64`, and two independent
/// callers computing "the same" MD rarely land on the identical bit pattern.
const MD_EPSILON: f64 = 1e-6;

pub(crate) fn line_height_mm(size_pt: f64) -> f64 {
    size_pt * LINE_HEIGHT_FACTOR * PT_TO_MM
}

/// The minimum height a cell's content demands (*"a section requires the
/// height of its tallest content"*) — its line count, plus the margin.
fn cell_min_height_mm(lines: usize, size_pt: f64) -> f64 {
    (lines.max(1) as f64 + CELL_MARGIN_LINES) * line_height_mm(size_pt)
}

/// A measured depth, printed without a French or English word attached
/// (no hardcoded label) — just the number, to the nearest whole metre
/// unless it needs a fraction.
fn format_md(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

// ---------------------------------------------------------------------------
// What is asked: the board and its parameters
// ---------------------------------------------------------------------------

/// A drilling sheet, before any layout is computed.
///
/// Plain data: everything a caller already knows, and nothing this crate
/// would have to read a package to find out.
pub struct Board {
    /// The sheet's title, shown once in the header. `None` draws no title —
    /// never an invented one (no French, no English, hardcoded here).
    pub title: Option<String>,
    /// The tracks, left to right, in the order they are drawn — "the checkboxes
    /// of the request become the list of tracks, orderable".
    pub tracks: Vec<Track>,
}

/// One column of the sheet.
pub enum Track {
    /// The depth column — one per sheet, drawn from the sections' own
    /// boundaries: a track by node, in the taxonomy of the track kinds,
    /// and the reason it creates no section of its own.
    Md(MdTrack),
    /// A column of `[md_top, md_base]` intervals — the only kind of track
    /// that cuts the sheet into sections.
    Intervals(IntervalTrack),
}

/// The depth column.
pub struct MdTrack {
    /// What a person reads above it — supplied by the caller, since no
    /// French or English word for "depth" is hardcoded here.
    pub header: String,
}

/// A column of intervals: the general shape behind strata, a categorical
/// property with its CBT, or any other banded reading of a well —
/// this increment does not care which, only that each interval states an
/// optional colour and an optional text.
pub struct IntervalTrack {
    /// What a person reads above the column.
    pub header: String,
    /// The intervals, in no particular order — [`compute`] does not assume
    /// they are sorted or contiguous.
    pub intervals: Vec<Interval>,
    /// Which [`TrackRender`]s of the registry draw this track's cells, tried
    /// in order — e.g. `["fill", "text"]` for a coloured column with a label
    /// on it, or just `["text"]` for a text-only column (the "flat fill" and
    /// "text" applied to the same track, as the demand's stratigraphy
    /// column does).
    pub renders: Vec<String>,
    /// The column's width, in characters — fixes how wide a
    /// "text" render wraps to. Unused by a track with no text render.
    pub max_chars: usize,
    /// The floor `max_chars` may not go under, so a column is never so
    /// narrow that every word sits alone on its own line.
    pub min_chars: usize,
}

impl IntervalTrack {
    /// What the BSS sections cut at — the crate's own default,
    /// not a magic number chosen here.
    pub const DEFAULT_MAX_CHARS: usize = 32;
    /// The default floor under `max_chars`.
    pub const DEFAULT_MIN_CHARS: usize = 8;

    /// A track with the repository's stated defaults for `max_chars` and
    /// `min_chars` — override either with
    /// [`IntervalTrack::with_max_chars`] / [`IntervalTrack::with_min_chars`].
    pub fn new(header: impl Into<String>, intervals: Vec<Interval>, renders: Vec<String>) -> Self {
        IntervalTrack {
            header: header.into(),
            intervals,
            renders,
            max_chars: Self::DEFAULT_MAX_CHARS,
            min_chars: Self::DEFAULT_MIN_CHARS,
        }
    }

    /// Override the column's width, in characters.
    pub fn with_max_chars(mut self, max_chars: usize) -> Self {
        self.max_chars = max_chars;
        self
    }

    /// Override the floor under `max_chars`.
    pub fn with_min_chars(mut self, min_chars: usize) -> Self {
        self.min_chars = min_chars;
        self
    }
}

/// One `[md_top, md_base]` interval of an [`IntervalTrack`].
pub struct Interval {
    /// The MD of the top, in metres.
    pub md_top: f64,
    /// The MD of the base, in metres. Greater than `md_top` for an ordinary
    /// interval; equal is a zero-thickness marker, refused nowhere here (it
    /// draws as a section with no MD extent, not as an error).
    pub md_base: f64,
    /// The text a "text" render draws — `None` draws nothing, which is not
    /// the same as an empty string (an empty string is still a case to
    /// wrap, drawing one empty line; `None` draws no case at all).
    pub text: Option<String>,
    /// The colour a "fill" render paints — `None` paints nothing. Never
    /// defaulted to a colour of this crate's choosing (rule: nothing is written
    /// without evidence).
    pub colour: Option<Rgba>,
    /// The object this interval represents — an ETP URI, or any identifier
    /// the caller wants a hover or a click to carry — written as `data-uri`.
    /// `None` when there is nothing to link to.
    pub uri: Option<String>,
    /// The row of a CBT this interval's text or colour came from — written
    /// as `data-row`, so an application can highlight the row a hover
    /// is over. `None` when there is none.
    pub row: Option<String>,
}

/// What decides the sheet's overall size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeightGoal {
    /// Millimetres of paper per metre of MD — 5.0 for 1:200 (1 cm per 2 m).
    Scale(f64),
    /// A total height for the sheet's body, in millimetres.
    Total(f64),
}

/// How big to draw, and how much text a column may carry.
///
/// No field here has a library default: the design leaves the
/// scale, the page height and the text size **open questions** — they are
/// not chosen yet, and this crate does not guess on anyone's behalf. A
/// caller states all four every time.
pub struct Params {
    /// The id of the [`ProportionMode`] to use, looked up in the registry
    /// [`compute`] is given.
    pub proportion: String,
    /// The sheet's target size — the mode may still grow past it
    /// ([`Layout::grown`]).
    pub height: HeightGoal,
    /// The page's width, in millimetres.
    pub page_width_mm: f64,
    /// The text size, in points (text is sized in points, geometry in
    /// millimetres).
    pub text_size_pt: f64,
}

// ---------------------------------------------------------------------------
// The proportion-mode registry
// ---------------------------------------------------------------------------

/// One section's MD thickness and the minimum height its content demands —
/// what a [`ProportionMode`] positions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionSpan {
    /// `md_base - md_top`, in metres.
    pub thickness_m: f64,
    /// [`cell_min_height_mm`]'s answer, maximised over every track's cell in
    /// this section.
    pub min_mm: f64,
}

/// What a [`ProportionMode`] answers.
pub struct ModeResult {
    /// The top of section 0, then the boundary after every section — one
    /// more entry than there are sections. Monotonically non-decreasing:
    /// [`compute`]'s own invariant, checked by `tests/layout.rs`.
    pub y_mm: Vec<f64>,
    /// The actual total used — equal to the requested target unless the
    /// mode grew the sheet.
    pub total_mm: f64,
    /// Whether the sheet had to grow past what was asked.
    pub grown: bool,
    /// What the mode has to say about how it placed things.
    pub notes: Vec<String>,
}

/// A function from sections to vertical positions — a registry entry.
/// Must be monotone, and must say what its axis measures.
pub trait ProportionMode: Send + Sync {
    /// The registry id — what [`Params::proportion`] names.
    fn id(&self) -> &'static str;
    /// What a person reads on the sheet as the axis's meaning ("the
    /// mode is written on the sheet").
    fn axis_label(&self) -> &'static str;
    /// Whether this mode distorts MD — when it does, [`compute`] adds a
    /// [`Ruler`] in the margin so a reader can still find the true position.
    fn deformed(&self) -> bool;
    /// Position every section, given a target total height in millimetres.
    fn positions(&self, sections: &[SectionSpan], target_mm: f64) -> ModeResult;
}

/// y proportional to MD thickness — no section receives more or less than
/// its share of `target_mm`, whatever its content needs.
///
/// A section whose content does not fit its imposed height is **not**
/// enlarged: [`compute`] truncates its text with an ellipsis instead
/// (the second fallback for a too-small section — the first, a trapezoid connecting a displaced
/// case to its section, is not implemented this increment; said, not
/// pretended).
pub struct ToScale;

impl ProportionMode for ToScale {
    fn id(&self) -> &'static str {
        "to-scale"
    }

    fn axis_label(&self) -> &'static str {
        "measured depth, to scale"
    }

    fn deformed(&self) -> bool {
        false
    }

    fn positions(&self, sections: &[SectionSpan], target_mm: f64) -> ModeResult {
        let total_thickness: f64 = sections.iter().map(|s| s.thickness_m).sum();
        let mm_per_m = if total_thickness > 0.0 {
            target_mm / total_thickness
        } else {
            0.0
        };
        let mut y = Vec::with_capacity(sections.len() + 1);
        y.push(0.0);
        let mut acc = 0.0;
        for s in sections {
            acc += s.thickness_m * mm_per_m;
            y.push(acc);
        }
        ModeResult {
            y_mm: y,
            total_mm: acc,
            grown: false,
            notes: Vec::new(),
        }
    }
}

/// Every section gets at least its minimum; what remains of `target_mm` is
/// shared in proportion to MD thickness — a level-filling algorithm, exactly
/// as the rule states it: *"sections whose proportional share is below
/// their minimum receive their minimum; the rest is shared in proportion
/// among the others; repeat until none falls below its minimum"*.
///
/// If the minimums alone exceed `target_mm`, the sheet grows to fit them
/// exactly — there is nothing left to share proportionally in that case —
/// and [`ModeResult::grown`] and [`ModeResult::notes`] say so.
pub struct Readable;

impl ProportionMode for Readable {
    fn id(&self) -> &'static str {
        "readable"
    }

    fn axis_label(&self) -> &'static str {
        "measured depth, distorted to keep every section readable"
    }

    fn deformed(&self) -> bool {
        true
    }

    fn positions(&self, sections: &[SectionSpan], target_mm: f64) -> ModeResult {
        let n = sections.len();
        if n == 0 {
            return ModeResult {
                y_mm: vec![0.0],
                total_mm: 0.0,
                grown: false,
                notes: Vec::new(),
            };
        }
        let total_min: f64 = sections.iter().map(|s| s.min_mm).sum();
        let mut notes = Vec::new();
        let mut heights = vec![0.0f64; n];
        let mut grown = false;

        if total_min > target_mm {
            for (i, s) in sections.iter().enumerate() {
                heights[i] = s.min_mm;
            }
            grown = true;
            notes.push(format!(
                "the sheet grows to {total_min:.1} mm instead of the requested {target_mm:.1} mm: the \
                 minimum height of its {n} section(s) already sums to that much"
            ));
        } else {
            let mut frozen = vec![false; n];
            let mut remaining_budget = target_mm;
            let mut remaining_thickness: f64 = sections.iter().map(|s| s.thickness_m).sum();

            // At most `n` rounds: a round that does not converge freezes at
            // least one more section, and there are only `n` to freeze.
            for _round in 0..n {
                let active_count = frozen.iter().filter(|f| !**f).count();
                if active_count == 0 {
                    break;
                }
                let mut to_freeze: Vec<usize> = Vec::new();
                for (i, s) in sections.iter().enumerate() {
                    if frozen[i] {
                        continue;
                    }
                    let share = if remaining_thickness > 0.0 {
                        remaining_budget * (s.thickness_m / remaining_thickness)
                    } else {
                        // Every still-active section has zero MD thickness —
                        // nothing to divide the budget by, so split evenly.
                        remaining_budget / active_count as f64
                    };
                    if share < s.min_mm {
                        to_freeze.push(i);
                    }
                }
                if to_freeze.is_empty() {
                    break;
                }
                for i in to_freeze {
                    heights[i] = sections[i].min_mm;
                    remaining_budget -= sections[i].min_mm;
                    remaining_thickness -= sections[i].thickness_m;
                    frozen[i] = true;
                }
            }

            let active_thickness: f64 = sections
                .iter()
                .enumerate()
                .filter(|(i, _)| !frozen[*i])
                .map(|(_, s)| s.thickness_m)
                .sum();
            let active_count = frozen.iter().filter(|f| !**f).count();
            for (i, s) in sections.iter().enumerate() {
                if frozen[i] {
                    continue;
                }
                heights[i] = if active_thickness > 0.0 {
                    remaining_budget * (s.thickness_m / active_thickness)
                } else {
                    remaining_budget / active_count.max(1) as f64
                };
            }
        }

        let mut y = Vec::with_capacity(n + 1);
        y.push(0.0);
        let mut acc = 0.0;
        for h in &heights {
            acc += h;
            y.push(acc);
        }
        ModeResult {
            y_mm: y,
            total_mm: acc,
            grown,
            notes,
        }
    }
}

/// The proportion-mode registry: built-ins first, callers' additions tried
/// before them (the repository's rule for every registry).
pub struct ModeRegistry(Vec<Box<dyn ProportionMode>>);

impl ModeRegistry {
    /// [`ToScale`] and [`Readable`], and nothing else.
    pub fn with_builtins() -> Self {
        ModeRegistry(vec![Box::new(ToScale), Box::new(Readable)])
    }

    /// Add a mode, tried before every one already registered — so it can
    /// replace a built-in of the same id, not just extend the list.
    pub fn register(&mut self, mode: Box<dyn ProportionMode>) {
        self.0.insert(0, mode);
    }

    /// The mode of this id, if one is registered.
    pub fn find(&self, id: &str) -> Option<&dyn ProportionMode> {
        self.0
            .iter()
            .find(|mode| mode.id() == id)
            .map(|mode| mode.as_ref())
    }
}

impl Default for ModeRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

// ---------------------------------------------------------------------------
// The track-render registry
// ---------------------------------------------------------------------------

/// One cell of an [`IntervalTrack`], after every render has had its turn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaidCell {
    /// What a "fill" render paints, if any and if the interval had one.
    pub colour: Option<Rgba>,
    /// What a "text" render draws, if any and if the interval had one.
    pub text: Option<TextCell>,
    /// [`Interval::uri`] of the interval this cell was drawn from, when the
    /// section matched exactly one.
    pub uri: Option<String>,
    /// [`Interval::row`], likewise.
    pub row: Option<String>,
    /// Neither a colour nor a text — an interval with nothing to say, or no
    /// interval at all here: "an interval without a value is a stated
    /// blank, never filled in". Computed, not asked for: true exactly when
    /// both fields above are `None`.
    pub blank: bool,
}

/// Text already wrapped to a column's width.
#[derive(Clone, Debug, PartialEq)]
pub struct TextCell {
    /// The lines, in reading order.
    pub lines: Vec<String>,
    /// The untruncated text, always kept — an SVG `<title>` reads it in
    /// full even when [`TextCell::lines`] had to be cut short.
    pub full_text: String,
    /// How many of [`TextCell::lines`] were cut with no separator to break
    /// after.
    pub hard_cuts: usize,
    /// Whether the section's imposed height was too short for every line
    /// [`crate::text::wrap`] produced, so the last one drawn ends in "…".
    pub truncated: bool,
}

/// One track's contribution to a cell, from one interval — a registry entry.
pub trait TrackRender: Send + Sync {
    /// The registry id — what [`IntervalTrack::renders`] names.
    fn id(&self) -> &'static str;
    /// Add this render's contribution to `cell`. `max_width_pt` is the
    /// column's text budget (`max_chars` × the font's mean advance);
    /// a render that draws no text ignores it.
    fn apply(&self, interval: &Interval, style: crate::text::Style, max_width_pt: f64, cell: &mut LaidCell);
}

/// Paints the interval's colour (a flat fill).
pub struct Fill;

impl TrackRender for Fill {
    fn id(&self) -> &'static str {
        "fill"
    }

    fn apply(
        &self,
        interval: &Interval,
        _style: crate::text::Style,
        _max_width_pt: f64,
        cell: &mut LaidCell,
    ) {
        if let Some(colour) = interval.colour {
            cell.colour = Some(colour);
        }
    }
}

/// Wraps the interval's text to the column's width (a text cell; the wrapping rules for
/// the wrapping itself).
pub struct TextRender;

impl TrackRender for TextRender {
    fn id(&self) -> &'static str {
        "text"
    }

    fn apply(&self, interval: &Interval, style: crate::text::Style, max_width_pt: f64, cell: &mut LaidCell) {
        if let Some(text) = &interval.text {
            let wrapped = crate::text::wrap(text, max_width_pt, style.size_pt);
            cell.text = Some(TextCell {
                lines: wrapped.lines,
                full_text: text.clone(),
                hard_cuts: wrapped.hard_cuts,
                truncated: false,
            });
        }
    }
}

/// The track-render registry: built-ins first, callers' additions tried
/// before them.
pub struct TrackRegistry(Vec<Box<dyn TrackRender>>);

impl TrackRegistry {
    /// [`Fill`] and [`TextRender`], and nothing else.
    pub fn with_builtins() -> Self {
        TrackRegistry(vec![Box::new(Fill), Box::new(TextRender)])
    }

    /// Add a render, tried before every one already registered.
    pub fn register(&mut self, render: Box<dyn TrackRender>) {
        self.0.insert(0, render);
    }

    /// The render of this id, if one is registered.
    pub fn find(&self, id: &str) -> Option<&dyn TrackRender> {
        self.0
            .iter()
            .find(|render| render.id() == id)
            .map(|render| render.as_ref())
    }
}

impl Default for TrackRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

// ---------------------------------------------------------------------------
// What comes back
// ---------------------------------------------------------------------------

/// One section of the sheet — the union of every interval track's bounds,
/// positioned.
#[derive(Clone, Debug, PartialEq)]
pub struct LaidSection {
    /// The MD of its top, in metres.
    pub md_top: f64,
    /// The MD of its base, in metres.
    pub md_base: f64,
    /// Its top, in millimetres from the body's own top (excluding the
    /// header — [`Layout::header_height_mm`]).
    pub y_top_mm: f64,
    /// Its base, likewise.
    pub y_base_mm: f64,
    /// The height its own content asks for: the tallest of its cells
    /// once wrapped, or one line for the MD label. A deformed mode honours it
    /// (`y_base_mm - y_top_mm >= min_mm`); a to-scale one may not, and
    /// truncates instead. Kept so that a test — and a page — can tell the two
    /// apart without recomputing it.
    pub min_mm: f64,
}

/// One tick of the MD column — a boundary between two sections, or the very
/// top or base of the sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct MdTick {
    /// The MD it marks, in metres.
    pub md: f64,
    /// Its position, in millimetres from the body's top.
    pub y_mm: f64,
    /// The number a person reads next to it.
    pub label: String,
}

/// One column, laid out.
#[derive(Debug)]
pub enum LaidTrack {
    /// The depth column.
    Md {
        /// [`MdTrack::header`], unchanged — assumed to fit one line: this
        /// increment does not allocate a column width for it (that is
        /// [`crate::svg`]'s heuristic, not part of what is tested here), so
        /// wrapping it would need a number this module has no basis for.
        header: String,
        /// One tick per section boundary — [`compute`]'s own MDs, not a
        /// caller's list (the MD column has no data of its own).
        /// Empty when no interval track is present to name a boundary: an MD
        /// column alone has nothing to derive a tick from.
        ticks: Vec<MdTick>,
    },
    /// An interval column.
    Intervals {
        /// [`IntervalTrack::header`].
        header: String,
        /// The header, wrapped to the column's text width when it renders
        /// text ("a header too long for its column wraps
        /// like a cell") — one line, unwrapped, for a column with
        /// no text render (a pure fill has no width this module derives).
        header_lines: Vec<String>,
        /// [`IntervalTrack::max_chars`], carried through for [`crate::svg`].
        max_chars: usize,
        /// [`IntervalTrack::renders`], carried through likewise.
        renders: Vec<String>,
        /// One cell per section of the sheet, in the same order as
        /// [`Layout::sections`].
        cells: Vec<LaidCell>,
    },
}

/// The to-scale ruler a deformed mode draws in the margin (the third rule of the deformed modes), so a reader can still find a section's true position.
#[derive(Clone, Debug, PartialEq)]
pub struct Ruler {
    /// Where every section boundary would sit at true scale, sharing the
    /// same total height as the deformed drawing — one entry per boundary,
    /// same length and same order as a [`LaidTrack::Md`]'s ticks.
    pub y_true_mm: Vec<f64>,
    /// The ruler's own total height, in millimetres — equal to the sheet's
    /// actual body height, since the ruler and the deformed drawing are two
    /// readings of the same span.
    pub total_mm: f64,
}

/// A drilling sheet, laid out — what `tests/layout.rs` checks in numbers,
/// and the only thing [`crate::svg::write`] reads.
#[derive(Debug)]
pub struct Layout {
    /// [`Board::title`], unchanged.
    pub title: Option<String>,
    /// The [`ProportionMode`] used, by id.
    pub mode_id: String,
    /// [`ProportionMode::axis_label`] of the mode used.
    pub axis_label: String,
    /// What the deformed modes require written on the sheet, under the header — the
    /// mode's id, what its axis measures, and whether the sheet grew.
    pub mode_note: String,
    /// [`ProportionMode::deformed`] of the mode used.
    pub deformed: bool,
    /// [`Params::page_width_mm`], unchanged.
    pub page_width_mm: f64,
    /// [`Params::text_size_pt`], unchanged — [`crate::svg`] needs it again to
    /// size text and to lay columns out horizontally, and reading it back
    /// off the [`Layout`] means a caller never has to keep the original
    /// [`Params`] around just for that.
    pub text_size_pt: f64,
    /// The header's height, in millimetres — the title (if any), the mode
    /// note, and the tallest wrapped column header.
    pub header_height_mm: f64,
    /// The body's total height, in millimetres — the sum of every
    /// [`LaidSection`]'s height; equal to the mode's requested target unless
    /// [`Layout::grown`].
    pub body_height_mm: f64,
    /// Whether the body had to grow past the requested height.
    pub grown: bool,
    /// The sections, top to bottom.
    pub sections: Vec<LaidSection>,
    /// The columns, left to right, same order as [`Board::tracks`].
    pub tracks: Vec<LaidTrack>,
    /// The margin ruler, when the mode is deformed.
    pub ruler: Option<Ruler>,
    /// What could not be drawn as asked, or had to be said rather than
    /// silently done — an unknown render id, a blank track, a truncation, a
    /// grown sheet.
    pub notes: Vec<String>,
}

/// Why [`compute`] refused to lay out a board — named, never a guess (refuse
/// rather than draw).
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// A column narrower than its own floor.
    #[error(
        "track `{track}`: max_chars ({max_chars}) is below min_chars ({min_chars}) — a column this narrow \
         would put every word alone on its own line, which is exactly what min_chars exists to refuse"
    )]
    MaxCharsBelowMinChars {
        /// The track's header, for the message.
        track: String,
        /// What was asked.
        max_chars: usize,
        /// The floor it fell under.
        min_chars: usize,
    },
    /// [`Params::proportion`] named nothing in the registry it was given.
    #[error("no proportion mode is registered as `{id}`")]
    UnknownMode {
        /// The id that was asked for.
        id: String,
    },
}

// ---------------------------------------------------------------------------
// Computing a layout
// ---------------------------------------------------------------------------

/// Lay out `board` with `params`, using only the built-in modes and renders.
pub fn compute(board: &Board, params: &Params) -> Result<Layout, Refusal> {
    compute_with(board, params, &ModeRegistry::default(), &TrackRegistry::default())
}

/// [`compute`], with the caller's own registries — built-ins plus whatever
/// was [`ModeRegistry::register`]ed or [`TrackRegistry::register`]ed.
pub fn compute_with(
    board: &Board,
    params: &Params,
    modes: &ModeRegistry,
    renders: &TrackRegistry,
) -> Result<Layout, Refusal> {
    // Refuse a column that could never wrap sensibly, before
    // anything else is computed from it.
    for track in &board.tracks {
        if let Track::Intervals(interval_track) = track
            && interval_track.max_chars < interval_track.min_chars
        {
            return Err(Refusal::MaxCharsBelowMinChars {
                track: interval_track.header.clone(),
                max_chars: interval_track.max_chars,
                min_chars: interval_track.min_chars,
            });
        }
    }

    let mode = modes
        .find(&params.proportion)
        .ok_or_else(|| Refusal::UnknownMode {
            id: params.proportion.clone(),
        })?;

    // Only interval tracks cut the sheet — a track by node (the MD
    // column is exactly that: one value per node, not per interval) creates
    // no section, because it never runs this loop.
    let mut bounds: Vec<f64> = Vec::new();
    for track in &board.tracks {
        if let Track::Intervals(interval_track) = track {
            for interval in &interval_track.intervals {
                bounds.push(interval.md_top);
                bounds.push(interval.md_base);
            }
        }
    }
    // `total_cmp` rather than `partial_cmp`: an MD is never meant to be
    // `NaN`, but a total order never panics on one either, which a refusal
    // this deep in a sort would be an odd place to raise.
    bounds.sort_by(f64::total_cmp);
    bounds.dedup_by(|a, b| (*a - *b).abs() < MD_EPSILON);
    let section_bounds: Vec<(f64, f64)> = bounds.windows(2).map(|pair| (pair[0], pair[1])).collect();

    let mut notes: Vec<String> = Vec::new();
    let has_md_track = board.tracks.iter().any(|track| matches!(track, Track::Md(_)));
    let mut min_mm = vec![
        if has_md_track {
            cell_min_height_mm(1, params.text_size_pt)
        } else {
            0.0
        };
        section_bounds.len()
    ];

    // One pass per interval track: the cells it draws, and its contribution
    // to each section's minimum height.
    let mut track_cells: Vec<Option<Vec<LaidCell>>> = Vec::with_capacity(board.tracks.len());
    for track in &board.tracks {
        let Track::Intervals(interval_track) = track else {
            track_cells.push(None);
            continue;
        };
        let max_width_pt =
            interval_track.max_chars as f64 * crate::text::average_char_width_pt(params.text_size_pt);
        let style = crate::text::Style {
            size_pt: params.text_size_pt,
        };
        let mut cells = Vec::with_capacity(section_bounds.len());
        let mut blanks = 0usize;
        for (index, (top, base)) in section_bounds.iter().enumerate() {
            let mut cell = LaidCell::default();
            let covering = interval_track.intervals.iter().find(|interval| {
                interval.md_top <= *top + MD_EPSILON && interval.md_base >= *base - MD_EPSILON
            });
            if let Some(interval) = covering {
                cell.uri = interval.uri.clone();
                cell.row = interval.row.clone();
                for render_id in &interval_track.renders {
                    match renders.find(render_id) {
                        Some(render) => render.apply(interval, style, max_width_pt, &mut cell),
                        None => notes.push(format!(
                            "track `{}`: no render is registered as `{render_id}`",
                            interval_track.header
                        )),
                    }
                }
            }
            cell.blank = cell.colour.is_none() && cell.text.is_none();
            if cell.blank {
                blanks += 1;
            }
            let lines = cell.text.as_ref().map_or(0, |text| text.lines.len());
            min_mm[index] = min_mm[index].max(cell_min_height_mm(lines, params.text_size_pt));
            cells.push(cell);
        }
        if blanks > 0 {
            // "An interval without a value is a stated blank, never
            // filled in" — said here, not just silently left empty in the SVG.
            notes.push(format!(
                "track `{}`: {blanks} of {} section(s) are blank — no colour and no text named for them",
                interval_track.header,
                section_bounds.len()
            ));
        }
        track_cells.push(Some(cells));
    }

    let total_thickness_m: f64 = section_bounds.iter().map(|(top, base)| base - top).sum();
    let target_mm = match params.height {
        HeightGoal::Scale(mm_per_m) => mm_per_m * total_thickness_m,
        HeightGoal::Total(total) => total,
    };
    let spans: Vec<SectionSpan> = section_bounds
        .iter()
        .zip(&min_mm)
        .map(|((top, base), min)| SectionSpan {
            thickness_m: base - top,
            min_mm: *min,
        })
        .collect();
    let result = mode.positions(&spans, target_mm);
    notes.extend(result.notes);

    let line_height = line_height_mm(params.text_size_pt);
    let mut sections: Vec<LaidSection> = Vec::with_capacity(section_bounds.len());
    for (index, (top, base)) in section_bounds.iter().enumerate() {
        let y_top = result.y_mm[index];
        let y_base = result.y_mm[index + 1];
        sections.push(LaidSection {
            md_top: *top,
            md_base: *base,
            y_top_mm: y_top,
            y_base_mm: y_base,
            min_mm: spans[index].min_mm,
        });

        // The second fallback: a section whose imposed height (only
        // possible in a mode that ignores the minimum, i.e. `ToScale`) is
        // shorter than its content is truncated, with the cut counted.
        let available = y_base - y_top;
        let max_lines = if line_height > 0.0 {
            (available / line_height).floor().max(0.0) as usize
        } else {
            0
        };
        for cells in track_cells.iter_mut().flatten() {
            if let Some(text) = cells[index].text.as_mut()
                && text.lines.len() > max_lines
            {
                if max_lines == 0 {
                    text.lines = vec!["…".to_string()];
                } else {
                    text.lines.truncate(max_lines);
                    if let Some(last) = text.lines.last_mut() {
                        last.push('…');
                    }
                }
                text.truncated = true;
                notes.push(format!(
                    "section MD {} to {}: its text does not fit the height this mode gives it and was cut",
                    format_md(*top),
                    format_md(*base)
                ));
            }
        }
    }

    let ticks: Vec<MdTick> = bounds
        .iter()
        .enumerate()
        .map(|(index, md)| MdTick {
            md: *md,
            y_mm: result
                .y_mm
                .get(index)
                .copied()
                .unwrap_or_else(|| result.y_mm.last().copied().unwrap_or(0.0)),
            label: format_md(*md),
        })
        .collect();

    let mut laid_tracks: Vec<LaidTrack> = Vec::with_capacity(board.tracks.len());
    for (track, cells) in board.tracks.iter().zip(track_cells) {
        match track {
            Track::Md(md_track) => {
                laid_tracks.push(LaidTrack::Md {
                    header: md_track.header.clone(),
                    ticks: ticks.clone(),
                });
            }
            Track::Intervals(interval_track) => {
                let header_lines = if interval_track.renders.iter().any(|render| render == "text") {
                    let max_width_pt = interval_track.max_chars as f64
                        * crate::text::average_char_width_pt(params.text_size_pt);
                    crate::text::wrap(&interval_track.header, max_width_pt, params.text_size_pt).lines
                } else {
                    vec![interval_track.header.clone()]
                };
                laid_tracks.push(LaidTrack::Intervals {
                    header: interval_track.header.clone(),
                    header_lines,
                    max_chars: interval_track.max_chars,
                    renders: interval_track.renders.clone(),
                    cells: cells.unwrap_or_default(),
                });
            }
        }
    }

    let column_rows = laid_tracks
        .iter()
        .map(|track| match track {
            LaidTrack::Md { .. } => 1,
            LaidTrack::Intervals { header_lines, .. } => header_lines.len().max(1),
        })
        .max()
        .unwrap_or(1);
    let title_rows = usize::from(board.title.is_some());
    const HEADER_MARGIN_MM: f64 = 2.0;
    let header_height_mm = (title_rows + 1 + column_rows) as f64 * line_height + HEADER_MARGIN_MM;

    let ruler = if mode.deformed() {
        let first_md = bounds.first().copied().unwrap_or(0.0);
        let mm_per_m = if total_thickness_m > 0.0 {
            result.total_mm / total_thickness_m
        } else {
            0.0
        };
        Some(Ruler {
            y_true_mm: bounds.iter().map(|md| (md - first_md) * mm_per_m).collect(),
            total_mm: result.total_mm,
        })
    } else {
        None
    };

    let mode_note = format!(
        "{} — axis: {}{}",
        mode.id(),
        mode.axis_label(),
        if result.grown {
            "; the sheet was grown to fit its minimum heights"
        } else {
            ""
        }
    );

    Ok(Layout {
        title: board.title.clone(),
        mode_id: mode.id().to_string(),
        axis_label: mode.axis_label().to_string(),
        mode_note,
        deformed: mode.deformed(),
        page_width_mm: params.page_width_mm,
        text_size_pt: params.text_size_pt,
        header_height_mm,
        body_height_mm: result.total_mm,
        grown: result.grown,
        sections,
        tracks: laid_tracks,
        ruler,
        notes,
    })
}
