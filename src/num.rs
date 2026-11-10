//! Numeric conversions with the reasoning written down once.
//!
//! UI geometry mixes counts (`usize`), byte totals (`u64`), readings (`f64`)
//! and pixels (`f32`). Each conversion below names its intent; the casts it
//! wraps are exact or saturate as documented, which is what every caller wants.
//!
//! Narrowing (`f64` to `f32`, float to integer) has no lossless form in `std`,
//! so this module is the one place those casts are allowed.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

/// A count or index used in pixel arithmetic. Exact up to 65535 and capped
/// there (rows, columns, samples: all far smaller).
#[must_use]
pub fn count_f32(n: usize) -> f32 {
    u16::try_from(n).map_or(f32::from(u16::MAX), f32::from)
}

/// A pixel size stored as an integer (window sizes). Exact up to 65535 px and
/// capped there.
#[must_use]
pub fn u32_f32(n: u32) -> f32 {
    u16::try_from(n).map_or(f32::from(u16::MAX), f32::from)
}

/// A count in `f64` arithmetic (averages). Exact up to 2^32 and capped there.
#[must_use]
pub fn count_f64(n: usize) -> f64 {
    u32::try_from(n).map_or(f64::from(u32::MAX), f64::from)
}

/// A byte or sector total in `f64`. Exact below 2^53 (8 PiB).
#[must_use]
pub fn total_f64(n: u64) -> f64 {
    let high = u32::try_from(n >> 32).unwrap_or(u32::MAX);
    let low = u32::try_from(n & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    f64::from(high) * 4_294_967_296.0 + f64::from(low)
}

/// A reading or fraction narrowed to `f32` for drawing; display precision.
#[must_use]
pub fn to_f32(x: f64) -> f32 {
    x as f32
}

/// Round to the nearest `u8`, saturating at 0 and 255 (NaN gives 0):
/// colour channels and alpha.
#[must_use]
pub fn to_u8(x: f32) -> u8 {
    x.round() as u8
}

/// Round to the nearest `u32`, saturating (NaN gives 0): window sizes.
#[must_use]
pub fn to_u32(x: f32) -> u32 {
    x.round() as u32
}

/// Round to the nearest `u64`, saturating (NaN gives 0): slider values.
#[must_use]
pub fn to_u64(x: f32) -> u64 {
    x.round() as u64
}

/// Round down to a `usize`, saturating (NaN gives 0): how many items fit.
#[must_use]
pub fn floor_usize(x: f32) -> usize {
    x.floor() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_conversions_round_and_saturate() {
        assert_eq!(to_u8(127.6), 128);
        assert_eq!(to_u8(-5.0), 0);
        assert_eq!(to_u8(300.0), 255);
        assert_eq!(to_u8(f32::NAN), 0);
        assert_eq!(to_u32(1099.5), 1100);
        assert_eq!(to_u64(250.4), 250);
        assert_eq!(floor_usize(3.99), 3);
        assert_eq!(floor_usize(-1.0), 0);
    }

    #[test]
    fn widening_conversions_are_exact_for_realistic_values() {
        assert_eq!(count_f32(300), 300.0);
        assert_eq!(count_f64(16), 16.0);
        assert_eq!(total_f64(68_719_476_736), 68_719_476_736.0);
        assert_eq!(total_f64(u64::from(u32::MAX) + 1), 4_294_967_296.0);
        assert_eq!(count_f32(70_000), 65_535.0, "capped");
        assert_eq!(to_f32(0.25), 0.25);
    }
}
