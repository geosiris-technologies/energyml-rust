//! Measuring text against the font the sheet is written in, and wrapping it.
//!
//! Sheet design: measuring and wrapping. The sheet is a document meant
//! to be read on paper or in a browser with no font file of its own — so a
//! line break has to be computed here, in Rust, from a table of measured
//! advances ([`widths`]), never guessed from a character count. "iiiiii" and
//! "MMMMMM" are the same length and nowhere near the same width.
//!
//! Everything in this module works in **points** (pt), the unit the sheet
//! writes text in. Converting a line's height to millimetres, for the
//! layout's vertical arithmetic, is [`crate::layout`]'s job — this module
//! never needs to know what a millimetre is.

mod widths;

/// A single text style: only the size matters for measuring, since this
/// crate ships one weight of one font (bold headers are a BSS
/// convention this increment does not reproduce).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// The font size, in points.
    pub size_pt: f64,
}

/// The advance of one character, in points, at `size_pt`.
///
/// [`widths::advance_per_mille`] answers for ASCII and Latin-1 and states a
/// measured default for everything else — never a silent zero, which would
/// let an unmeasured glyph pack for free and every line after it drift.
pub fn char_width_pt(ch: char, size_pt: f64) -> f64 {
    f64::from(widths::advance_per_mille(ch)) / 1000.0 * size_pt
}

/// The width of a string, in points: the sum of its characters' advances.
///
/// No kerning: the table holds advances only (the design says "the horizontal
/// advances... enough to measure a string without a browser"), and
/// kerning pairs are a much larger table this increment does not embed. The
/// error this leaves is the same one `max_chars` already tolerates by
/// design — a column sized in characters, not sub-millimetres.
pub fn string_width_pt(text: &str, size_pt: f64) -> f64 {
    text.chars().map(|ch| char_width_pt(ch, size_pt)).sum()
}

/// The mean width of an ASCII character at `size_pt` — what `max_chars`
/// multiplies to become a column width (*"`max_chars` × the mean
/// width of a character of the font at that size"*).
///
/// [`widths::DEFAULT_ADVANCE_PER_MILLE`] *is* that mean, computed once by the
/// generator from the same table this module measures with — never a second,
/// separately chosen number.
pub fn average_char_width_pt(size_pt: f64) -> f64 {
    f64::from(widths::DEFAULT_ADVANCE_PER_MILLE) / 1000.0 * size_pt
}

/// A block of text, already broken into lines that each fit a width.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Wrapped {
    /// The lines, in reading order. Never empty: an empty input yields one
    /// empty line, so a caller always has a line to lay a box around.
    pub lines: Vec<String>,
    /// How many lines had to be broken **without** a space or one of
    /// `/ - , ;` to fall back on (the "clean cut", counted rather than
    /// hidden).
    pub hard_cuts: usize,
}

/// Separators a word may be broken after, when it alone is wider than one
/// line — *"cut after `/`, `-`, `,` or `;` if it has one, otherwise cut
/// bluntly"*.
const BREAK_AFTER: [char; 4] = ['/', '-', ',', ';'];

/// Wrap `text` to lines no wider than `max_width_pt`, measured at `size_pt`.
///
/// Breaks at spaces first; a word wider than `max_width_pt` on its own is
/// split after the last of `/ - , ;` it carries that still leaves the
/// fragment within the width, or — carrying none, or none early enough — cut
/// at the last character that fits, counted in [`Wrapped::hard_cuts`].
///
/// # What this cannot guarantee
///
/// If a single character is itself wider than `max_width_pt` (a column
/// narrower than one glyph — `min_chars` exists precisely so a caller cannot
/// ask for this, see [`crate::layout::Refusal::MaxCharsBelowMinChars`]), a
/// line carrying only that character will overflow. Every other line is
/// guaranteed within `max_width_pt`.
pub fn wrap(text: &str, max_width_pt: f64, size_pt: f64) -> Wrapped {
    let mut lines: Vec<String> = Vec::new();
    let mut hard_cuts = 0usize;
    let mut current = String::new();
    let mut current_width = 0.0f64;
    let space_width = char_width_pt(' ', size_pt);

    for word in text.split_whitespace() {
        let word_width = string_width_pt(word, size_pt);
        let with_space = if current.is_empty() {
            word_width
        } else {
            current_width + space_width + word_width
        };
        if with_space <= max_width_pt {
            if !current.is_empty() {
                current.push(' ');
                current_width += space_width;
            }
            current.push_str(word);
            current_width += word_width;
            continue;
        }
        // Does not fit after what is already on the line.
        if word_width <= max_width_pt {
            flush(&mut current, &mut lines);
            current.push_str(word);
            current_width = word_width;
            continue;
        }
        // The word alone is wider than a whole line: flush first, so every
        // fragment below gets a fresh, full `max_width_pt` budget.
        flush(&mut current, &mut lines);
        let fragments = split_word(word, max_width_pt, size_pt, &mut hard_cuts);
        let mut fragments = fragments.into_iter().peekable();
        while let Some(fragment) = fragments.next() {
            if fragments.peek().is_some() {
                // A full fragment: it is a line of its own.
                lines.push(fragment);
            } else {
                // The last fragment starts the line that follow-up words may
                // still join.
                current_width = string_width_pt(&fragment, size_pt);
                current = fragment;
            }
        }
    }
    flush(&mut current, &mut lines);
    if lines.is_empty() {
        lines.push(String::new());
    }
    Wrapped { lines, hard_cuts }
}

/// Push `current` onto `lines` if it holds anything, and empty it.
fn flush(current: &mut String, lines: &mut Vec<String>) {
    if !current.is_empty() {
        lines.push(std::mem::take(current));
    }
}

/// Break one word too wide for a line into fragments that each fit
/// `max_width_pt` — the last fragment may still be joined by further words.
fn split_word(word: &str, max_width_pt: f64, size_pt: f64, hard_cuts: &mut usize) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut fragments = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        // The longest run from `start` that fits — always at least one
        // character, so a pathologically wide glyph still makes progress
        // rather than looping forever (see this function's caller's doc).
        let mut end = start;
        let mut width = 0.0;
        while end < chars.len() {
            let w = char_width_pt(chars[end], size_pt);
            if end > start && width + w > max_width_pt {
                break;
            }
            width += w;
            end += 1;
        }
        let mut cut = end;
        if end < chars.len() {
            // Not the word's last fragment: prefer breaking right after the
            // last separator inside this run, so the separator stays with
            // the fragment that precedes it.
            match (start + 1..end).rev().find(|&i| BREAK_AFTER.contains(&chars[i - 1])) {
                Some(sep_at) => cut = sep_at,
                None => *hard_cuts += 1,
            }
        }
        fragments.push(chars[start..cut].iter().collect());
        start = cut;
    }
    fragments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_line_is_wider_than_the_measured_width() {
        // What would break this: a wrap that counts characters instead of
        // measuring them — "iiiiiiiiii" and "MMMMMMMMMM" are both ten
        // characters and nothing alike in width.
        let text = "MMMMMMMMMM iiiiiiiiii MMMMMMMMMM the quick brown fox";
        let size_pt = 10.0;
        let max_width_pt = string_width_pt("MMMMMMMMMM", size_pt);
        let wrapped = wrap(text, max_width_pt, size_pt);
        for line in &wrapped.lines {
            assert!(
                string_width_pt(line, size_pt) <= max_width_pt + 1e-9,
                "line {line:?} measures {} against a budget of {max_width_pt}",
                string_width_pt(line, size_pt)
            );
        }
        assert!(wrapped.lines.len() > 1, "the text does not fit on one line");
    }

    #[test]
    fn a_word_too_long_for_a_line_breaks_after_a_separator_it_carries() {
        // The design's own example shape: a token glued by `/` that is wider than
        // the column. What would break this: ignoring `BREAK_AFTER` and
        // falling straight to a hard cut, which the count below would show.
        let size_pt = 10.0;
        let max_width_pt = string_width_pt("PRE/SABLE", size_pt); // wide enough to force one break, not zero
        let long_word = "PRE/SABLE-ARGILEUX";
        let wrapped = wrap(long_word, max_width_pt, size_pt);
        assert!(wrapped.lines.len() > 1);
        assert!(
            wrapped.lines[0].ends_with('/') || wrapped.lines[0].ends_with('-'),
            "the first line should end right after a separator: {:?}",
            wrapped.lines[0]
        );
    }

    #[test]
    fn a_word_with_no_separator_is_hard_cut_and_it_is_counted() {
        // What would break this: a hard cut that happens but is not counted
        // — the report would then say a column wrapped perfectly cleanly
        // when it did not.
        let size_pt = 10.0;
        let word = "supercalifragilisticexpialidocious";
        let max_width_pt = string_width_pt(&word[..8], size_pt);
        let wrapped = wrap(word, max_width_pt, size_pt);
        assert!(wrapped.lines.len() > 1);
        assert!(wrapped.hard_cuts > 0, "no separator exists in this word; the cut must be counted");
    }

    #[test]
    fn an_empty_string_wraps_to_one_empty_line() {
        let wrapped = wrap("", 100.0, 10.0);
        assert_eq!(wrapped.lines, vec![String::new()]);
        assert_eq!(wrapped.hard_cuts, 0);
    }

    #[test]
    fn widening_the_column_never_adds_lines() {
        // A sanity property rather than a citation: wrapping the same text
        // to a wider budget cannot need more lines. What would break this:
        // an off-by-one in the space-vs-word budget comparison.
        let text = "the quick brown fox jumps over the lazy dog and then some more words follow";
        let size_pt = 9.0;
        let narrow = wrap(text, string_width_pt("the quick", size_pt), size_pt);
        let wide = wrap(text, string_width_pt("the quick brown fox jumps over", size_pt), size_pt);
        assert!(wide.lines.len() <= narrow.lines.len());
    }
}
