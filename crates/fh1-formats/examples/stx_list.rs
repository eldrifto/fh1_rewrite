//! Probe: the entries of a track's `.stx.bin` (FM2): `stx_list <bin.zip> [from] [to]`
use fh1_formats::{caff, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let e = ar.entries.iter().find(|e| e.name.ends_with(".stx.bin")).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let ents = caff::parse_stx(&d).unwrap();
    let (from, to) = (a.get(2).and_then(|v| v.parse().ok()).unwrap_or(0usize), a.get(3).and_then(|v| v.parse().ok()).unwrap_or(usize::MAX));
    println!("{} entries", ents.len());
    for (i, t) in ents.iter().enumerate().filter(|(i, _)| *i >= from && *i <= to) {
        println!("{i}: {}x{} {:?} raw {:#x} mips {} cube {} gpu {}+{}", t.width, t.height, t.format, t.raw_format, t.mip_count, t.cube, t.gpu_offset, t.gpu_len);
    }
}
