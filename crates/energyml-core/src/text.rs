//! Numbers as plain text.

/// A number without useless zeros: `10`, not `10.0`; any other value as Rust
/// writes it.
///
/// Only a value that is exactly integral **and** below 1e15 in magnitude is
/// written as an integer: beyond that the integer cast would saturate
/// (`1e20 as i64` is `i64::MAX`) and print a number nobody wrote. A value that
/// is not finite is written as Rust writes it (`NaN`, `inf`), never as a
/// number it is not.
pub fn plain_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::plain_number;

    /// Break it: drop the `abs() < 1e15` guard (1e20 then prints `i64::MAX`).
    #[test]
    fn plain_number_drops_useless_zeros_and_never_saturates() {
        assert_eq!(plain_number(10.0), "10");
        assert_eq!(plain_number(-3.0), "-3");
        assert_eq!(plain_number(0.5), "0.5");
        assert_eq!(plain_number(1e20), "100000000000000000000");
        assert_eq!(plain_number(f64::NAN), "NaN");
        assert_eq!(plain_number(f64::INFINITY), "inf");
    }
}
