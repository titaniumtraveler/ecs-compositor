use crate::helpers::wrapping_add;

/// Describes a specific bit in a `[AtomicU64;N]`
#[derive(Debug, Clone, Copy)]
pub struct Pos {
    /// Index, which [`AtomicU64`](std::sync::atomic::AtomicU64) this position is refering to.
    pub chunk: usize,
    /// Index into the specific bit of the chunk pointed to by [`Self::chunk`].
    ///
    /// # Invariants
    /// - The upper two bits have to be **always** `0`.
    ///   Or phrased differently: `index & 0b1100_0000 == 0` always holds.
    ///   If rust allowed it, this could be an `u6`
    pub index: u8,
}

impl Pos {
    pub(crate) fn wrapping_add<const LEN: usize>(self, rhs: u8) -> Self {
        let Self { chunk, index } = self;
        wrapping_add! (
            index + rhs; 0..64u8;
            no_wrap => |WrapArgs { rhs,  .. }| Pos { chunk, index: index + rhs},
            do_wrap => |WrapArgs { rhs, diff, .. }| Pos { chunk: wrapping_add!(chunk + 1; 0..LEN), index: rhs - diff },
        )
    }
}
