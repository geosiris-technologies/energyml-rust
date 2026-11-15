//! The bridge between the domain's colour (`energyml_domain::color::Rgba`, the one reader and writer of a hex
//! colour) and the drawing's (`draw::Colour`): the drawing module names no format crate, so the bridge lives here.

use energyml_domain::color::Rgba;

use crate::draw::Colour;

impl From<Rgba> for Colour {
    /// The bytes of the domain colour (`Rgba::to_bytes`: rounded, clamped).
    fn from(colour: Rgba) -> Self {
        let [r, g, b, a] = colour.to_bytes();
        Colour::rgba(r, g, b, a)
    }
}

impl From<Colour> for Rgba {
    fn from(colour: Colour) -> Self {
        Rgba::from_bytes([colour.r, colour.g, colour.b, colour.a])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drawing writes the text the domain's writer writes, and a `Colour` and an `Rgba` convert both ways
    /// without losing a byte.
    #[test]
    fn a_colour_is_written_as_the_domain_writes_it_and_converts_both_ways() {
        let colour = Colour::rgba(18, 171, 255, 128);
        assert_eq!(colour.hex(), "#12abff");
        assert_eq!(colour.hex(), Rgba::from(colour).to_hex_rgb());
        assert_eq!(Colour::from(Rgba::from(colour)), colour);
        let read = Rgba::parse("#12abff80").expect("eight digits");
        assert_eq!(Colour::from(read), colour);
        // Break it: make `Colour::hex` write upper-case digits.
    }
}
