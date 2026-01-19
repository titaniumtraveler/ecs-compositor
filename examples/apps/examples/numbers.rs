fn main() {
    let size = 1024;
    for i in 1..=size {
        if ((i - 1) & ((1 << 4) - 1)) == 0 && (i - 1) != 0 {
            println!()
        }
        if ((i - 1) & ((1 << 8) - 1)) == 0 && (i - 1) != 0 {
            println!()
        }

        let brightness = 0x8000;

        let val = brightness * i / size;
        let val: u16 = std::cmp::min(val, u16::MAX as u32) as u16;

        print!("{val:04X} ");
    }
    println!()
}
