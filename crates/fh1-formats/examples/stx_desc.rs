//! Probe: raw descriptor words (big-endian u32) of `.stx.bin` entries (FM2): `stx_desc <bin.zip> [from] [to]`. MODLOG 128 (looking for texture
//! fetch-constant fields such as the exponent adjust in the lightmap pages).
use fh1_formats::{caff, zip::Archive};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let e = ar.entries.iter().find(|e| e.name.ends_with(".stx.bin")).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let ents = caff::parse_stx(&d).unwrap();
    let first = u32::from_be_bytes(d[0x48..0x4c].try_into().unwrap()) as usize;
    let mut positions = Vec::new();
    let mut p = first;
    while positions.len() < ents.len() {
        positions.push(p);
        p += 8;
        p += d[p..].windows(8).position(|w| w == b"texture\0").unwrap_or(0);
    }
    let (from, to) = (a.get(2).and_then(|v| v.parse().ok()).unwrap_or(0usize), a.get(3).and_then(|v| v.parse().ok()).unwrap_or(usize::MAX));
    for (i, p) in positions.iter().enumerate().filter(|(i, _)| *i >= from && *i <= to) {
        let t = &ents[i];
        let words: Vec<String> = (0..16).map(|k| format!("{:08x}", u32::from_be_bytes(d[p + 8 + 4 * k..p + 12 + 4 * k].try_into().unwrap()))).collect();
        println!("{i}: {}x{} {:?} | {}", t.width, t.height, t.format, words.join(" "));
    }
}
