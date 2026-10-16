//! Probe: every shader container (`0x102A1100` PS / `0x102A1101` VS) of an FM2 model with its literal constants (`def cN`) and named constants.
//! `cargo run -p fh1-shaders --example fm2_defs -- <bin.zip> <model number>`
use fh1_formats::zip::Archive;
use fh1_shaders::container::ShaderBlob;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let suffix = format!(".{:0>5}.rmb.bin", a[2].parse::<usize>().unwrap());
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let mut o = 0;
    while o + 8 <= d.len() {
        let m = u32::from_be_bytes(d[o..o + 4].try_into().unwrap());
        if (m & 0xFFFF_FF00) == 0x102A_1100 {
            let mut copy = d[o..].to_vec();
            copy[3] = if m & 1 == 1 { 1 } else { 0 };
            if m != 0x102A_1100 && m != 0x102A_1101 {
                println!("-- container magic {m:#x} at {o:#x}");
            }
            if let Ok(b) = ShaderBlob::parse(&copy, o) {
                println!("== {:?} @{o:#x} (data {:#x}) {} microcode dwords", b.stage, o, b.microcode.len());
                for c in &b.constants { println!("   const {:?} reg {} x{} {} default {:?}", c.set, c.register, c.count, c.name, c.default.as_ref().map(|d| d.iter().map(|w| f32::from_bits(*w)).collect::<Vec<_>>())); }
                for (r, v) in &b.float_defs { println!("   def c{r} = {v:?}"); }
                for (r, v) in &b.int_defs { println!("   defi i{r} = {v:?}"); }
            }
        }
        o += 4;
    }
}
