//! The unit of a hue, decided from the numbers - a pure function, with no object and no package.
//!
//! RESQML TRG 2.2 section 4.21 (`HsvColor`) says only *"Hue of the color in the HSV model"* and refers to Wikipedia: **the schema
//! fixes no unit**. A producer writes 0-360 degrees (the colour model's), another writes a fraction of a turn (0.602 for a blue).
//! Whoever reads the colours, and whoever rewrites them, has to decide the same way, so the decision on the numbers is here,
//! where the reader (`energyml-domain`'s `color`, which reads the objects), the translator (which writes the hues as fractions) and any
//! other that holds nothing but numbers can all reach it.
//!
//! # The rule
//!
//! It is applied to **a set of colours, never to one colour alone**: all the hues of one colour map (its entries and its null,
//! above and below colours); all the constant colours of one `GraphicalInformationSet`.
//!
//! - every hue in 0 to 1 and at least one **strictly between** 0 and 1: [`HueUnit::Turns`];
//! - a hue above 1 (up to 360), or one that is no number, or below 0: [`HueUnit::Degrees`] - a hue in `]1, 360]` forces
//!   degrees, and a hue outside 0-360 is for the caller to refuse by name;
//! - no hue, or only 0 and 1: [`HueUnit::Undecided`] - read as degrees, with no effect on the colour: 0 and 1 turn give the
//!   same red as 0 and 1 degree, to a degree.
//!
//! The rare cost: a map written in degrees whose hues are **all under one degree** (all red) is read in turns; a caller
//! that reads in turns says so.

/// The unit the hues of one set of colours are written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HueUnit {
    /// 0-1 of a full turn: the hue in degrees is 360 times the number.
    Turns,
    /// 0-360, the colour model's own.
    Degrees,
    /// Nothing in the set says: it holds no hue, or only 0 and 1. Read as degrees; it changes nothing.
    Undecided,
}

/// The unit the hues of **one set of colours** are written in, by the rule of the module.
///
/// `hues` must be all the hues of the set: a hue left out can flip the answer.
pub fn hue_unit(hues: &[f64]) -> HueUnit {
    if hues.is_empty() {
        return HueUnit::Undecided;
    }
    if !hues.iter().all(|hue| (0.0..=1.0).contains(hue)) {
        return HueUnit::Degrees;
    }
    if hues.iter().any(|hue| *hue > 0.0 && *hue < 1.0) {
        HueUnit::Turns
    } else {
        HueUnit::Undecided
    }
}

/// A hue of the unit `unit`, in degrees: 360 times the number in turns, the number itself otherwise.
pub fn to_degrees(hue: f64, unit: HueUnit) -> f64 {
    match unit {
        HueUnit::Turns => hue * 360.0,
        HueUnit::Degrees | HueUnit::Undecided => hue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five hues of a colour map written in turns are turns: all in 0-1, four strictly between.
    ///
    /// Break it: in `hue_unit`, answer `Degrees` whenever the first hue is under 1.
    #[test]
    fn hues_all_in_zero_to_one_with_one_between_are_turns() {
        let hues = [0.6024, 0.1667, 0.9223, 0.9160, 0.0];
        assert_eq!(hue_unit(&hues), HueUnit::Turns);
        assert_eq!(to_degrees(0.5, HueUnit::Turns), 180.0);
    }

    /// A set is decided as a whole: one hue above 1 makes every other hue degrees, even 0.5.
    ///
    /// Break it: in `hue_unit`, decide on `any` hue in 0-1 and not on `all`.
    #[test]
    fn a_hue_above_one_forces_degrees_for_the_whole_set() {
        assert_eq!(hue_unit(&[120.0, 240.0]), HueUnit::Degrees);
        assert_eq!(hue_unit(&[0.5, 120.0]), HueUnit::Degrees);
        assert_eq!(hue_unit(&[0.5, 1.5]), HueUnit::Degrees);
        assert_eq!(to_degrees(0.5, HueUnit::Degrees), 0.5);
    }

    /// 0 and 1 alone decide nothing - the same red in either unit to a degree - and nor does an empty set.
    ///
    /// Break it: in `hue_unit`, count a hue of exactly 1 as strictly between.
    #[test]
    fn zero_and_one_alone_and_no_hue_decide_nothing() {
        assert_eq!(hue_unit(&[0.0, 1.0]), HueUnit::Undecided);
        assert_eq!(hue_unit(&[0.0]), HueUnit::Undecided);
        assert_eq!(hue_unit(&[1.0]), HueUnit::Undecided);
        assert_eq!(hue_unit(&[]), HueUnit::Undecided);
        assert_eq!(to_degrees(1.0, HueUnit::Undecided), 1.0);
    }

    /// What is no hue at all is not turns: a negative number or one that is no number leaves the set in degrees (the caller
    /// refuses it by name).
    ///
    /// Break it: in `hue_unit`, drop the range test.
    #[test]
    fn numbers_that_are_no_hue_leave_the_set_in_degrees() {
        assert_eq!(hue_unit(&[0.5, -0.5]), HueUnit::Degrees);
        assert_eq!(hue_unit(&[0.5, f64::NAN]), HueUnit::Degrees);
    }
}
