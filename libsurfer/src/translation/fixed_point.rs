use num::{BigUint, One, Zero};
use std::cmp::Ordering;
use std::ops::BitAnd;

/// Convert a `BigUint` to a string interpreted as unsigned fixed point value.
///
/// The output is equivalent to `uint / (2 ** lg_scaling_factor)` (without integer truncation
/// through division)
pub(crate) fn big_uint_to_ufixed(uint: &BigUint, lg_scaling_factor: i64) -> String {
    match lg_scaling_factor.cmp(&0) {
        Ordering::Less => {
            format!("{}", uint << (-lg_scaling_factor))
        }
        Ordering::Equal => format!("{}", uint),
        Ordering::Greater => {
            let mask = (BigUint::one() << lg_scaling_factor) - 1_u32;

            // Split fixed point value into integer and remainder
            let integer_part = uint >> lg_scaling_factor;
            let mut remainder = uint.bitand(&mask);

            if remainder.is_zero() {
                integer_part.to_string() // No fractional part
            } else {
                let mut fractional_part = String::new();

                // Scale up the remainder to extract fractional digits
                for _ in 0..lg_scaling_factor {
                    remainder *= 10_u32;
                    let digit = &remainder >> lg_scaling_factor;
                    fractional_part.push_str(&digit.to_string());
                    remainder &= &mask;

                    // Stop if the scaled remainder becomes zero
                    if remainder.is_zero() {
                        break;
                    }
                }

                format!("{}.{}", integer_part, fractional_part)
            }
        }
    }
}

/// Convert a `BigUint` to a string interpreted as signed fixed point value.
///
/// The output is equivalent to `as_signed(uint) / (2 ** lg_scaling_factor)` (without integer
/// truncation through division)
/// where `as_signed()` interprets the `uint` as a signed value using two's complement.
pub(crate) fn big_uint_to_sfixed(uint: &BigUint, num_bits: u64, lg_scaling_factor: i64) -> String {
    if num_bits == 0 {
        return "".to_string();
    }
    if uint.bit(num_bits - 1) {
        let inverted_uint = (BigUint::one() << num_bits) - uint;
        format!("-{}", big_uint_to_ufixed(&inverted_uint, lg_scaling_factor))
    } else {
        big_uint_to_ufixed(uint, lg_scaling_factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn ucheck(value: impl Into<BigUint>, lg_scaling_factor: i64, expected: impl Into<String>) {
        let value = value.into();
        let result = big_uint_to_ufixed(&value, lg_scaling_factor);
        assert_eq!(result, expected.into());
    }

    fn scheck(
        value: impl Into<BigUint>,
        num_bits: u64,
        lg_scaling_factor: i64,
        expected: impl Into<String>,
    ) {
        let value = value.into();
        let result = big_uint_to_sfixed(&value, num_bits, lg_scaling_factor);
        assert_eq!(result, expected.into());
    }

    #[test]
    fn zero_scaling_factor() {
        ucheck(32_u32, 0, "32");
        scheck(32_u32, 6, 0, "-32");
    }

    #[test]
    fn zero_width_signed() {
        scheck(32_u32, 0, 0, "");
    }

    #[test]
    fn test_exact_integer() {
        ucheck(256_u32, 8, "1");
    }

    #[test]
    fn test_fractional_value() {
        ucheck(48225_u32, 8, "188.37890625");
        ucheck(100_u32, 10, "0.09765625");
        ucheck(8192_u32, 15, "0.25");
        ucheck(16384_u32, 15, "0.5");
    }

    #[test]
    fn test_large_value() {
        ucheck(
            BigUint::from_str("12345678901234567890").unwrap(),
            20,
            "11773756886705.9401416778564453125",
        )
    }

    #[test]
    fn test_value_less_than_one() {
        ucheck(1_u32, 10, "0.0009765625")
    }

    #[test]
    fn test_zero_value() {
        ucheck(0_u32, 16, "0")
    }

    #[test]
    fn test_negative_scaling_factor() {
        ucheck(500_u32, -1, "1000")
    }

    #[test]
    fn test_negative_fractional_value() {
        scheck(0x1E000_u32, 17, 15, "-0.25");
        scheck(0x1FF_u32, 9, 7, "-0.0078125");
    }

    #[test]
    fn test_negative_exact_integer() {
        scheck(0xFCC_u32, 12, 2, "-13");
        scheck(0x100_u32, 9, 7, "-2");
        scheck(256_u32, 9, 8, "-1")
    }

    #[test]
    fn test_negative_non_signed_values() {
        scheck(0x034_u32, 12, 2, "13");
        scheck(0x02000_u32, 17, 15, "0.25");
        scheck(0x0FF_u32, 9, 7, "1.9921875");
    }

    #[test]
    fn large_bit_widths() {
        ucheck(0x123456789ABCDEF0_u64, 20, "1250999896491.8044281005859375");
    }
}
