//! The drawing module keeps its own XML escaping, unescaping and base64, because it names no format crate (it can
//! become a crate of its own, `tests/draw/no_format.rs`); the one door of each is `energyml_core`'s (ADR 0045). This
//! test pins every copy to its door, so the two cannot drift apart.

#[cfg(test)]
mod tests {
    use crate::draw::svg;

    const TEXTS: &[&str] = &[
        "",
        "plain",
        "a < b & c > d",
        "\"double\" and 'single'",
        "tab\tline\ncarriage\r",
        "control \u{1} \u{8} \u{1f} dropped",
        "\u{FFFE}\u{FFFF} not XML",
        "accents \u{e9}\u{e8} and \u{1F600}",
    ];

    #[test]
    fn the_drawing_escapes_as_the_core_door_does() {
        for text in TEXTS {
            assert_eq!(
                svg::escape(text),
                energyml_core::xml::escape_attr(text),
                "{text:?}"
            );
        }
        // Break it: drop the `'\''` arm of `draw::svg::escape`.
    }

    #[test]
    fn the_drawing_unescapes_as_the_core_door_does() {
        let mut inputs: Vec<String> = TEXTS
            .iter()
            .map(|text| energyml_core::xml::escape_attr(text))
            .collect();
        for odd in ["&#0;", "&#x41;&#66;", "&nbsp;", "a & b", "&#xD800;", "&#1114112;"] {
            inputs.push(odd.to_string());
        }
        for input in &inputs {
            assert_eq!(
                svg::unescape(input),
                energyml_core::xml::unescape(input),
                "{input:?}"
            );
        }
        // Break it: remove the `is_xml_char` filter of `draw::svg::unescape` (`&#0;` is then read).
    }

    #[test]
    fn the_drawing_encodes_base64_as_the_core_door_does() {
        let bytes: Vec<u8> = (0..=255).collect();
        for len in 0..12 {
            assert_eq!(
                svg::base64(&bytes[..len]),
                energyml_core::base64::encode(&bytes[..len]),
                "{len}"
            );
        }
        assert_eq!(svg::base64(&bytes), energyml_core::base64::encode(&bytes));
        // Break it: write the last group of `draw::svg::base64` without its `=` padding.
    }
}
