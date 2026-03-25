use std::{
    cmp::Ordering,
    ops::{Add, AddAssign, Sub, SubAssign},
};

pub use self::carrying_add::{CarryingAdd, WrappingU6, WrappingUsize};

mod carrying_add;

/// Describes a specific bit in a `[AtomicU64;N]`
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub struct Pos<const MAX: usize> {
    /// Index, which [`AtomicU64`](std::sync::atomic::AtomicU64) this position is refering to.
    pub chunk: WrappingUsize<MAX>,
    /// Index into the specific bit of the chunk pointed to by [`Self::chunk`].
    ///
    /// # Invariants
    /// - The upper two bits have to be **always** `0`.
    ///   Or phrased differently: `index & 0b1100_0000 == 0` always holds.
    ///   If rust allowed it, this could be an `u6`
    pub index: WrappingU6,
}

impl<const MAX: usize> Pos<MAX> {
    /// Split flat index refering to a single bit into chunk and index
    fn from_flat_index(flat_index: usize) -> Self {
        Pos {
            chunk: WrappingUsize::<MAX>::new(flat_index >> 6),
            index: WrappingU6::new((flat_index & ((1 << 6) - 1)) as u8),
        }
    }
}

impl<const MAX: usize> Pos<MAX> {}

impl<const MAX: usize> CarryingAdd for Pos<MAX> {
    const ZERO: Self = Self { chunk: WrappingUsize::<MAX>::ZERO, index: WrappingU6::ZERO };
    const ONE: Self = Self { chunk: WrappingUsize::<MAX>::ZERO, index: WrappingU6::ONE };
    const MAX: Self = Self { chunk: WrappingUsize::<MAX>::MAX, index: WrappingU6::ONE };

    type Out = Self;

    fn carrying_add(self, rhs: Pos<MAX>, carry: bool) -> (Self::Out, bool) {
        let (index, overflow) = self.index.carrying_add(rhs.index, carry);
        let (chunk, overflow) = self.chunk.carrying_add(rhs.chunk, overflow);

        (Self { chunk, index }, overflow)
    }
    fn borrowing_sub(self, rhs: Pos<MAX>, borrow: bool) -> (Self::Out, bool) {
        let (index, overflow) = self.index.borrowing_sub(rhs.index, borrow);
        let (chunk, overflow) = self.chunk.borrowing_sub(rhs.chunk, overflow);

        (Self { chunk, index }, overflow)
    }

    fn checked_sub(self, rhs: Pos<MAX>) -> Option<Self::Out> {
        let (val, borrow) = self.borrowing_sub(rhs, false);
        if borrow { Some(val) } else { None }
    }
}

impl<const MAX: usize> Add for Pos<MAX> {
    type Output = Pos<MAX>;
    fn add(self, rhs: Pos<MAX>) -> Self::Output {
        self.carrying_add(rhs, false).0
    }
}

impl<const MAX: usize> Sub for Pos<MAX> {
    type Output = Pos<MAX>;
    fn sub(self, rhs: Pos<MAX>) -> Self::Output {
        self.borrowing_sub(rhs, false).0
    }
}

impl<const MAX: usize> AddAssign for Pos<MAX> {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}
impl<const MAX: usize> SubAssign for Pos<MAX> {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl<const MAX: usize> CarryingAdd<WrappingUsize<MAX>> for Pos<MAX> {
    const ZERO: Self = <Self as CarryingAdd>::ZERO;
    const ONE: Self = <Self as CarryingAdd>::ONE;
    const MAX: Self = <Self as CarryingAdd>::MAX;

    type Out = Self;
    fn carrying_add(self, flat_index: WrappingUsize<MAX>, carry: bool) -> (Self::Out, bool) {
        self.carrying_add(Pos::from_flat_index(*flat_index), carry)
    }
    fn borrowing_sub(self, flat_index: WrappingUsize<MAX>, borrow: bool) -> (Self::Out, bool) {
        self.borrowing_sub(Pos::from_flat_index(*flat_index), borrow)
    }

    fn checked_sub(self, rhs: WrappingUsize<MAX>) -> Option<Self::Out> {
        let (val, borrow) = self.borrowing_sub(rhs, false);
        if borrow { Some(val) } else { None }
    }
}

impl<const MAX: usize> PartialEq<WrappingUsize<MAX>> for Pos<MAX> {
    fn eq(&self, other: &WrappingUsize<MAX>) -> bool {
        self.eq(&Pos::from_flat_index(**other))
    }
}

impl<const MAX: usize> PartialOrd<WrappingUsize<MAX>> for Pos<MAX> {
    fn partial_cmp(&self, other: &WrappingUsize<MAX>) -> Option<Ordering> {
        Some(self.cmp(&Pos::from_flat_index(**other)))
    }
}

impl<const MAX: usize> Add<WrappingUsize<MAX>> for Pos<MAX> {
    type Output = Pos<MAX>;
    fn add(self, rhs: WrappingUsize<MAX>) -> Self::Output {
        self.carrying_add(rhs, false).0
    }
}
impl<const MAX: usize> Sub<WrappingUsize<MAX>> for Pos<MAX> {
    type Output = Pos<MAX>;
    fn sub(self, rhs: WrappingUsize<MAX>) -> Self::Output {
        self.borrowing_sub(Pos::from_flat_index(*rhs), false).0
    }
}
impl<const MAX: usize> AddAssign<WrappingUsize<MAX>> for Pos<MAX> {
    fn add_assign(&mut self, rhs: WrappingUsize<MAX>) {
        *self = *self + rhs;
    }
}
impl<const MAX: usize> SubAssign<WrappingUsize<MAX>> for Pos<MAX> {
    fn sub_assign(&mut self, rhs: WrappingUsize<MAX>) {
        *self = *self - rhs;
    }
}

impl<const MAX: usize> Add<Pos<MAX>> for WrappingUsize<MAX> {
    type Output = Pos<MAX>;
    fn add(self, rhs: Pos<MAX>) -> Self::Output {
        Pos::from_flat_index(*self).carrying_add(rhs, false).0
    }
}
impl<const MAX: usize> Sub<Pos<MAX>> for WrappingUsize<MAX> {
    type Output = Pos<MAX>;
    fn sub(self, rhs: Pos<MAX>) -> Self::Output {
        Pos::from_flat_index(*self).borrowing_sub(rhs, false).0
    }
}
