mod bitfield;
mod helpers;
pub mod reader;
mod sync_point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct WaylandPos {
    // 18 bits
    // maximum wayland length frame length is `1 << 16`
    // we fit at most `1 << 16 * 4` in data -> `16 + 2` bits
    data: u32,
    // 10 bits
    // at most 1024 file descriptors in buffer (for mostly arbitrary reasons) -> 10 bits
    ctrl: u16,
    // 15 bits
    // each wayland message is at least 8 byte long
    // `1 << 16 * 4 / 8` -> `16 + 2 - 3` bits
    slot: u16,
}

#[allow(dead_code)]
impl WaylandPos {
    const fn from_u64(val: u64) -> Self {
        Self {
            data: ((val >> 32) & ((1 << 18) - 1)) as u32,
            ctrl: ((val >> 16) & ((1 << 10) - 1)) as u16,
            slot: (val & ((1 << 15) - 1)) as u16,
        }
    }
    const fn into_64(self) -> u64 {
        (((self.data & ((1 << 18) - 1)) as u64) << 32)
            | (((self.ctrl & ((1 << 10) - 1)) as u64) << 16)
            | (self.slot & ((1 << 15) - 1)) as u64
    }
}

#[test]
fn t() {
    let foo = WaylandPos { data: 200_000, ctrl: 500, slot: 30_000 };

    let val = foo.into_64();
    println!("{val:0x}");

    assert_eq!(foo, WaylandPos::from_u64(val))
}
