fn main() {
    let slice = std::ptr::slice_from_raw_parts_mut(std::ptr::null_mut::<u8>(), 16);
    println!("{slice:?}");
}
