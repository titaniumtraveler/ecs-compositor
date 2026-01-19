#![allow(nonstandard_style, dead_code)]

fn main() {}

const wayland_max: u64 = 1 << 16;

const data: u64 = 4 * wayland_max;
const ctrl: u64 = 1024;
const slot: u64 = data / 8;

const bitwidths: [u32; 3] = [data.ilog2(), ctrl.ilog2(), slot.ilog2()];
const bitwidth_sum: u32 = bitwidths[0] + bitwidths[1] + bitwidths[2];
