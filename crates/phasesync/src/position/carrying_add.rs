use std::{
    cmp::PartialOrd,
    ops::{Add, AddAssign, Deref, Sub, SubAssign},
};

pub trait CarryingAdd<Rhs = Self>
where
    Self: Sized + Copy + PartialOrd + PartialOrd<Rhs> + PartialOrd<Self::Out>,
    Self: Add<Self, Output = Self> + Sub<Self, Output = Self> + Sub<Rhs, Output = Self>,
    Self: Sub<Self::Out, Output = Self::Out>,
    //
    Rhs: Sized + Copy + Sub<Self, Output = Self::Out>,
    //
    Self::Out: From<Self> + PartialOrd<Self>,
    Self::Out: Add<Self, Output = Self::Out> + Sub<Self, Output = Self::Out>,
    Self::Out: Add<Self::Out, Output = Self::Out>,
    Self::Out: Sub<Rhs, Output = Self::Out>,
{
    const ZERO: Self;
    const ONE: Self;

    const MAX: Self;

    type Out;

    fn carrying_add(self, rhs: Rhs, carry: bool) -> (Self::Out, bool) {
        {
            let carry_int = if !carry { Self::ZERO } else { Self::ONE };
            let reversed_lhs = Self::MAX - self;
            match Self::checked_sub(reversed_lhs, rhs) {
                Some(val) => {
                    let val: Self::Out = Self::MAX - val;
                    match (val < Self::MAX, carry) {
                        (true, _) | (false, false) => (val + carry_int, false),
                        (false, true) => (Self::ZERO.into(), true),
                    }
                }
                None => (rhs - reversed_lhs - Self::ONE + carry_int, true),
            }
        }
    }
    fn borrowing_sub(self, rhs: Rhs, borrow: bool) -> (Self::Out, bool) {
        {
            let carry_int = if !borrow { Self::ZERO } else { Self::ONE };
            match Self::checked_sub(self, rhs) {
                Some(val) => {
                    let val: Self::Out = val;
                    match (Self::ZERO < val, borrow) {
                        (true, _) | (false, false) => (val - carry_int, false),
                        (false, true) => (Self::ZERO.into(), true),
                    }
                }
                None => (
                    (Into::<Self::Out>::into(Self::MAX)) - rhs + self + Self::ONE - carry_int,
                    true,
                ),
            }
        }
    }

    fn checked_sub(self, rhs: Rhs) -> Option<Self::Out>;
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub struct WrappingU6(u8);

impl WrappingU6 {
    /// Truncates `val` to 6 bytes
    pub const fn new(val: u8) -> Self {
        Self(val & ((1 << 6) - 1))
    }

    pub const fn inner(self) -> u8 {
        self.0
    }
}

impl CarryingAdd for WrappingU6 {
    const ZERO: Self = Self(0);
    const ONE: Self = Self(1);

    const MAX: Self = Self((1 << 6) - 1);

    type Out = Self;

    fn checked_sub(self, rhs: Self) -> Option<Self::Out> {
        u8::checked_sub(self.inner(), rhs.inner()).map(Self)
    }
}

impl Deref for WrappingU6 {
    type Target = u8;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Add for WrappingU6 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.inner().add(rhs.inner()))
    }
}

impl Sub for WrappingU6 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.inner().sub(rhs.inner()))
    }
}

impl AddAssign for WrappingU6 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for WrappingU6 {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

#[test]
fn test_carrying_add() {
    fn carrying_add(lhs: u8, rhs: u8, carry: bool) -> (u8, bool) {
        let (WrappingU6(u8), overflow) = WrappingU6(lhs).carrying_add(WrappingU6(rhs), carry);
        (u8, overflow)
    }

    assert_eq!(carrying_add(10, 30, false), (40, false));
    assert_eq!(carrying_add(30, 35, false), (1, true));

    assert_eq!(carrying_add(10, 30, true), (41, false));
    assert_eq!(carrying_add(30, 35, true), (2, true));

    assert_eq!(carrying_add(62, 1, true), (0, true));
}

#[test]
fn test_borrowing_sub() {
    fn borrowing_sub(lhs: u8, rhs: u8, borrow: bool) -> (u8, bool) {
        let (WrappingU6(u8), overflow) = WrappingU6(lhs).borrowing_sub(WrappingU6(rhs), borrow);
        (u8, overflow)
    }

    assert_eq!(borrowing_sub(40, 30, false), (10, false));
    assert_eq!(borrowing_sub(1, 35, false), (30, true));

    assert_eq!(borrowing_sub(41, 30, true), (10, false));
    assert_eq!(borrowing_sub(2, 35, true), (30, true));

    assert_eq!(borrowing_sub(0, 1, true), (62, true));
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub struct WrappingUsize<const MAX: usize>(usize);

impl<const MAX: usize> WrappingUsize<MAX> {
    pub fn new(val: usize) -> Self {
        Self(val.min(MAX))
    }

    pub fn inner(self) -> usize {
        self.0
    }
}

impl<const MAX: usize> Deref for WrappingUsize<MAX> {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MAX: usize> PartialEq<usize> for WrappingUsize<MAX> {
    fn eq(&self, other: &usize) -> bool {
        self.inner().eq(other)
    }
}

impl<const MAX: usize> Add for WrappingUsize<MAX> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.inner().add(rhs.inner()))
    }
}

impl<const MAX: usize> Sub for WrappingUsize<MAX> {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.inner().sub(rhs.inner()))
    }
}

impl<const MAX: usize> AddAssign for WrappingUsize<MAX> {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl<const MAX: usize> SubAssign for WrappingUsize<MAX> {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl<const MAX: usize> CarryingAdd for WrappingUsize<MAX> {
    const ZERO: Self = Self(0);
    const ONE: Self = Self(1);

    const MAX: Self = Self(MAX);

    type Out = Self;

    fn checked_sub(self, rhs: Self) -> Option<Self::Out> {
        usize::checked_sub(self.inner(), rhs.inner()).map(Self)
    }
}
