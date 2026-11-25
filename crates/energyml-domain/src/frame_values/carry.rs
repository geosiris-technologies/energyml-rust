//! The values of a wellbore's frames, carried by measured depth onto another
//! representation of the same wellbore.
//!
//! [`crate::strata_by_md::carry`] does it for the stratigraphic units; this does
//! it for **any property** read by [`super::of`], with the **same rule** for
//! when two MDs can be compared ([`crate::strata_by_md::comparable`]): one
//! trajectory, or one datum, one MD domain and one unit up to a conversion. A
//! frame that fails the rule is set aside **by name**, never shifted; none left
//! is a refusal.
//!
//! # What a node takes
//!
//! The value the frame holds at its MD ([`super::FrameValues::at_md`], the side
//! below on a node between two intervals - the rule of [`crate::well_strata`]),
//! or nothing:
//!
//! - an MD outside every frame - above the first, below the last, in a gap
//!   between two frames - is [`Carry::Outside`], counted, never given a
//!   neighbour's value;
//! - an MD where a frame holds no value (an absent value, a discrete property
//!   between two nodes) is [`Carry::Absent`]: *not known here*, which does not
//!   disagree with another frame;
//! - two frames naming **different** values at one MD are a [`Carry::Disagreement`]:
//!   the node has none, it is counted and named in the notes, and neither frame
//!   is chosen.
//!
//! Nothing is averaged. A continuous property between two nodes of one frame is
//! interpolated by the frame's own rule (`at_md`, linear), and never over a hole.

use crate::along_md::median_step;
use crate::strata_by_md::{MdReference, comparable};

use super::{AtMdRefusal, FrameValues, Refusal, Side, Value};

/// A frame's values, with how its MDs are counted.
pub struct Stated<'a> {
    /// What the frame says.
    pub values: &'a FrameValues,
    /// Against what its MDs are counted ([`crate::strata_by_md::md_reference`]).
    pub reference: &'a MdReference,
}

/// What one node of the other representation is given.
#[derive(Clone, Debug, PartialEq)]
pub enum Carry {
    /// A value, from the frames.
    Value(Value),
    /// The frames hold the MD and say nothing there.
    Absent,
    /// No frame holds the MD.
    Outside,
    /// Two frames name two different values.
    Disagreement,
}

/// A representation given the values its wellbore's frames state.
#[derive(Clone, Debug, PartialEq)]
pub struct Carried {
    /// The representation's uuid, lowercased.
    pub representation: String,
    /// Its title.
    pub title: String,
    /// One per node, in the order the representation is drawn.
    pub nodes: Vec<Carry>,
    /// The frames the values were read from, lowercased.
    pub from: Vec<String>,
    /// What was set aside or could not be decided.
    pub notes: Vec<String>,
}

impl Carried {
    /// Nodes that were given a value.
    pub fn named(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| matches!(node, Carry::Value(_)))
            .count()
    }

    /// Nodes no frame holds.
    pub fn outside(&self) -> usize {
        self.nodes.iter().filter(|node| **node == Carry::Outside).count()
    }

    /// Nodes where two frames name two values.
    pub fn disagreements(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| **node == Carry::Disagreement)
            .count()
    }
}

/// A frame's nodes, checked to run down: an MD lookup needs strictly increasing
/// finite depths.
fn runs_down(values: &FrameValues) -> Result<(), String> {
    let axis = values.support.node_md();
    if axis.iter().any(|md| !md.is_finite()) {
        return Err(format!("`{}` has a node with no finite MD", values.title));
    }
    if let Some(at) = axis.windows(2).position(|pair| pair[1] <= pair[0]) {
        return Err(format!(
            "`{}`: node {} is not deeper than the node before it; an MD is looked up in depths that run down",
            values.title,
            at + 1
        ));
    }
    Ok(())
}

/// Give the nodes at `mds` - counted against `target` - the values the frames in
/// `stated` hold at them.
///
/// Frames whose MDs cannot be compared ([`comparable`]), or whose depths do not
/// run down, are set aside **by name** in the notes. None left:
/// [`Refusal::Incomparable`], each reason given.
pub fn carry(
    representation: &str,
    title: &str,
    mds: &[f64],
    target: &MdReference,
    stated: &[Stated<'_>],
) -> Result<Carried, Refusal> {
    let mut notes = Vec::new();
    let mut set_aside = Vec::new();
    // (the frame, its factor from the target's unit)
    let mut used: Vec<(&FrameValues, f64)> = Vec::new();
    for one in stated {
        match comparable(target, one.reference).and_then(|factor| runs_down(one.values).map(|()| factor)) {
            Ok(factor) => used.push((one.values, factor)),
            Err(why) => set_aside.push(format!("`{}`: {why}", one.values.title)),
        }
    }
    if used.is_empty() {
        return Err(Refusal::Incomparable {
            what: format!("`{title}`"),
            why: format!(
                "the frames of its wellbore that hold this property cannot be read against its MDs - {}",
                set_aside.join("; ")
            ),
        });
    }
    for why in &set_aside {
        notes.push(format!("set aside, {why}"));
    }

    let mut disputed: Vec<String> = Vec::new();
    let nodes: Vec<Carry> = mds
        .iter()
        .map(|md| {
            let mut held = false;
            // (the value, the frame that names it)
            let mut named: Vec<(Value, usize)> = Vec::new();
            for (which, (values, factor)) in used.iter().enumerate() {
                match values.at_md(md * factor, Side::Below, "linear") {
                    Ok(sample) => {
                        held = true;
                        if let Some(value) = sample.value
                            && !named.iter().any(|(already, _)| *already == value)
                        {
                            named.push((value, which));
                        }
                    }
                    // Not on this frame: outside it, which is no disagreement.
                    Err(AtMdRefusal::Placement(_)) | Err(AtMdRefusal::NothingAbove { .. }) => {}
                    // The frame holds the MD and has no value there to give.
                    Err(_) => held = true,
                }
            }
            match named.len() {
                0 if held => Carry::Absent,
                0 => Carry::Outside,
                1 => Carry::Value(named.remove(0).0),
                _ => {
                    if disputed.len() < 5 {
                        disputed.push(format!(
                            "at MD {md}, {}",
                            named
                                .iter()
                                .map(|(value, which)| format!("`{}` says {value}", used[*which].0.title))
                                .collect::<Vec<_>>()
                                .join(" and ")
                        ));
                    }
                    Carry::Disagreement
                }
            }
        })
        .collect();

    let mut carried = Carried {
        representation: representation.to_ascii_lowercase(),
        title: title.to_string(),
        nodes,
        from: used.iter().map(|(values, _)| values.frame.clone()).collect(),
        notes,
    };
    let disagreements = carried.disagreements();
    if disagreements > 0 {
        carried.notes.push(format!(
            "{disagreements} node(s) where two frames of the wellbore name two different values - left without one, \
             neither frame chosen: {}{}",
            disputed.join("; "),
            if disagreements > disputed.len() { "; …" } else { "" }
        ));
    }
    let outside = carried.outside();
    if outside > 0 {
        carried.notes.push(format!(
            "{outside} of {} node(s) lie outside every frame that holds the property - left without a value, never \
             given a neighbour's",
            carried.nodes.len()
        ));
    }
    // A frame of one node has no step to snap by: said once, not refused.
    if used
        .iter()
        .any(|(values, _)| median_step(&values.support.node_md()).is_err())
    {
        carried
            .notes
            .push("a frame of fewer than two nodes only answers on its own node".to_string());
    }
    Ok(carried)
}
