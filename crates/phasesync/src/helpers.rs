use std::{
    num::NonZero,
    sync::atomic::{
        AtomicU64,
        Ordering::{Acquire, Release},
    },
};

/// Create a bitmask that selects the `lower..=upper` bits of an [`u64`].
///
/// # Panics
///
/// Panics if either end of the range are outside of the bits of an `u64`,
/// so the following has to hold:
/// - `0 <= lower && lower <= 63`
/// - `0 <= upper && upper <= 63`
pub const fn bitmask_range(lower: u8, upper: u8) -> u64 {
    assert!(lower <= 63);
    assert!(upper <= 63);

    let (lower, upper) = (lower, upper + 1);

    match (lower, upper) {
        (l, u) if u <= l => 0,
        (64.., _) => 0,
        (l, 64..) => u64::MAX - ((1 << l) - 1),
        (l, u) => (1 << u) - (1 << l),
    }
}

/// Get the index of the first 1 bit in `val`.
/// Returns [`None`] when the value is 0.
///
/// Based on [`u64::lowest_one()`]
///
/// FIXME: Replace when [`int_lowest_highest_one` `#145203`](https://github.com/rust-lang/rust/issues/145203) gets stabilized.
pub const fn lowest_one(val: u64) -> Option<u8> {
    let Some(val) = NonZero::new(val) else {
        return None;
    };

    Some((u64::BITS - 1 - val.leading_zeros()) as u8)
}

/// Loop until `cond(val)` is false, or `val` is successfully updated to `f(val)`.
/// Returns whether the update was successful.
pub fn try_while(
    chunk: &AtomicU64,
    mut val: u64,
    cond: impl FnMut(u64) -> bool,
    f: impl FnMut(u64) -> u64,
) -> bool {
    try_while_mut(chunk, &mut val, cond, f)
}

/// Loop until `cond(val)` is false, or `val` is successfully updated to `f(val)`.
/// Returns whether the update was successful.
///
/// Updates `*val` to the latest read value.
pub fn try_while_mut(
    chunk: &AtomicU64,
    val: &mut u64,
    mut cond: impl FnMut(u64) -> bool,
    mut f: impl FnMut(u64) -> u64,
) -> bool {
    while cond(*val) {
        match chunk.compare_exchange(*val, f(*val), Release, Acquire) {
            Ok(_old) => return true,
            Err(actual) => *val = actual,
        }
    }

    false
}

pub struct WrapArgs<Lhs, Rhs, Lower, Upper, Diff> {
    pub lhs: Lhs,
    pub rhs: Rhs,

    pub lower: Lower,
    pub upper: Upper,

    pub diff: Diff,
}
