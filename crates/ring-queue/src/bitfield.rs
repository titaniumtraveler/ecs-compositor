#![allow(dead_code)]

pub(crate) trait BitField<const WIDTH: u8, S: Storage<{ WIDTH }>> {
    const WIDTH: u8 = WIDTH;

    const MASK: S = S::MASK;
    const CAP: S = S::CAP;
}

pub(crate) trait Storage<const WIDTH: u8> {
    const WIDTH: u8 = WIDTH;

    const MASK: Self;
    const CAP: Self;
}

impl<const WIDTH: u8> Storage<WIDTH> for u16 {
    const CAP: Self = 1 << WIDTH;
    const MASK: Self = <Self as Storage<WIDTH>>::CAP - 1;
}

impl<const WIDTH: u8> Storage<WIDTH> for u32 {
    const CAP: Self = 1 << WIDTH;
    const MASK: Self = <Self as Storage<WIDTH>>::CAP - 1;
}
