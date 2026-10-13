//! Checking the inputs before anything is read (ADR-0015).
//!
//! Contract: [`validate`] and [`InputsError`].
//!
//! Every refusal names the field and the value it found: a width of `-3` is
//! said as such, never as "invalid settings". The checks go one step past what
//! the type says, to what the calculation downstream would refuse in its own
//! words (`Corridor` wants a width on at least one side and no zero-length
//! segment): refusing here keeps the message about *the inputs*.

use crate::model::{CrossWidths, Crosses, Inputs};

/// The most cross sections one set of inputs may ask for: a guard against a
/// spacing of a millimetre on a trace of kilometres, not a limit anyone meets.
pub const MAX_CROSSES: u64 = 10_000;

/// Inputs that cannot make a catalogue — each refusal names what is wrong.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InputsError {
    /// The trace is not a trace (fewer than two distinct points, a coordinate
    /// that is not a number…).
    #[error("the trace {0}")]
    Trace(String),
    /// A width, spacing or count that is not usable.
    #[error("{0}")]
    Setting(String),
}

/// Whether these inputs can make a catalogue.
pub fn validate(inputs: &Inputs) -> Result<(), InputsError> {
    let trace = &inputs.trace;
    if trace.len() < 2 {
        return Err(InputsError::Trace(format!(
            "needs at least two points, got {}",
            trace.len()
        )));
    }
    if let Some(i) = trace.iter().position(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Err(InputsError::Trace(format!(
            "point {i} has a coordinate that is not a finite number: [{}, {}]",
            trace[i][0], trace[i][1]
        )));
    }
    if let Some(i) = (1..trace.len()).find(|&i| trace[i] == trace[i - 1]) {
        return Err(InputsError::Trace(format!(
            "points {} and {i} coincide ([{}, {}]): a segment of no length has no direction, \
             so no side",
            i - 1,
            trace[i][0],
            trace[i][1]
        )));
    }
    width("the left width", inputs.left)?;
    width("the right width", inputs.right)?;
    if inputs.left == 0.0 && inputs.right == 0.0 {
        return Err(InputsError::Setting(
            "the long section's left and right widths are both 0: its corridor would keep nothing"
                .to_string(),
        ));
    }
    if let Some(f) = inputs.time_factor
        && (!f.is_finite() || f <= 0.0)
    {
        return Err(InputsError::Setting(format!(
            "the time factor must be a finite number above 0, got {f}"
        )));
    }
    match &inputs.crosses {
        Crosses::None => {}
        Crosses::Every { spacing, common, own } => {
            if !spacing.is_finite() || *spacing <= 0.0 {
                return Err(InputsError::Setting(format!(
                    "the cross sections' spacing must be a finite number above 0, got {spacing}"
                )));
            }
            widths("the common cross sections'", common)?;
            for o in own {
                widths(&format!("cross section {}'s", o.index), &o.widths)?;
            }
            distinct(own.iter().map(|o| o.index))?;
        }
        Crosses::Count { count, common, own } => {
            if *count < 1 {
                return Err(InputsError::Setting(format!(
                    "the number of cross sections must be at least 1, got {count}"
                )));
            }
            if u64::from(*count) > MAX_CROSSES {
                return Err(InputsError::Setting(format!(
                    "{count} cross sections is over the limit of {MAX_CROSSES}"
                )));
            }
            widths("the common cross sections'", common)?;
            for o in own {
                widths(&format!("cross section {}'s", o.index), &o.widths)?;
            }
            distinct(own.iter().map(|o| o.index))?;
        }
    }
    Ok(())
}

fn width(name: &str, value: f64) -> Result<(), InputsError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(InputsError::Setting(format!(
            "{name} must be a finite number, not negative, got {value}"
        )))
    }
}

fn widths(who: &str, w: &CrossWidths) -> Result<(), InputsError> {
    width(&format!("{who} length before"), w.before)?;
    width(&format!("{who} length after"), w.after)?;
    width(&format!("{who} left width"), w.left)?;
    width(&format!("{who} right width"), w.right)?;
    if w.before + w.after <= 0.0 {
        return Err(InputsError::Setting(format!(
            "{who} lengths before and after are both 0: a cross section of no length has no \
             direction"
        )));
    }
    if w.left == 0.0 && w.right == 0.0 {
        return Err(InputsError::Setting(format!(
            "{who} left and right widths are both 0: the corridor would keep nothing"
        )));
    }
    Ok(())
}

fn distinct(indices: impl Iterator<Item = u32>) -> Result<(), InputsError> {
    let mut seen = std::collections::HashSet::new();
    for i in indices {
        if !seen.insert(i) {
            return Err(InputsError::Setting(format!(
                "cross section {i} is set apart twice"
            )));
        }
    }
    Ok(())
}
