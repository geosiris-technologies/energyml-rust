//! Base 64, RFC 4648 section 4: the one codec of the workspace.
//!
//! No dependency for it: the alphabet is 64 bytes, and `energyml-core` stays
//! free of anything that does not compile to WebAssembly. `xsd:base64Binary`
//! (attachment `Content`) is read by [`decode`], and the pictures an SVG embeds
//! are written by [`encode`]; there is no second copy anywhere.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base 64 with padding, no line breaks (RFC 4648 section 4).
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode base 64; white space is ignored, as `xsd:base64Binary` allows, and
/// anything else outside the alphabet - or a bad padding - is refused.
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    let mut sextets: Vec<u8> = Vec::with_capacity(text.len());
    let mut padding = 0usize;
    for c in text.chars().filter(|c| !c.is_ascii_whitespace()) {
        if c == '=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err("a character follows the `=` padding".to_string());
        }
        let value = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            other => return Err(format!("`{other}` is not in the base 64 alphabet")),
        };
        sextets.push(value);
    }
    if padding > 2 || !(sextets.len() + padding).is_multiple_of(4) || sextets.len() % 4 == 1 {
        return Err("the length is not that of padded base 64".to_string());
    }
    let mut out = Vec::with_capacity(sextets.len() * 3 / 4);
    for group in sextets.chunks(4) {
        let n = group
            .iter()
            .enumerate()
            .fold(0u32, |n, (at, v)| n | (u32::from(*v) << (18 - 6 * at)));
        out.push((n >> 16) as u8);
        if group.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if group.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    /// RFC 4648 section 10, the test vectors, both ways.
    ///
    /// Break it: change a character of `ALPHABET`.
    #[test]
    fn the_vectors_of_rfc_4648_section_10() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, coded) in cases {
            assert_eq!(encode(plain.as_bytes()), coded, "RFC 4648 section 10: {plain:?}");
            assert_eq!(decode(coded).unwrap(), plain.as_bytes());
        }
    }

    /// White space is ignored; a stray character, a short group and a
    /// character after the padding are refused.
    ///
    /// Break it: accept `!` in the `match` of `decode`.
    #[test]
    fn decode_ignores_white_space_and_refuses_the_rest() {
        assert_eq!(decode("Zm9v\r\nYmFy").unwrap(), b"foobar");
        assert!(decode("Zm9v!").is_err());
        assert!(decode("Zm9").is_err());
        assert!(decode("Zg==Zg==").is_err());
        assert!(decode("Z").is_err());
    }
}
