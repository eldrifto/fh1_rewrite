//! Probe: models whose groups cannot be paired; prints the first few with their group / tail shader sizes. `fm2_gaps <bin.zip> [max]`
use fh1_formats::{fm2, zip::Archive};
use fh1_shaders::fm2 as sh;
fn be(d: &[u8], o: usize) -> Option<u32> { d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) }
fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let max: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(10);
    let mut ar = Archive::open(&a[1]).unwrap();
    let mut shown = 0;
    for e in ar.entries.clone() {
        if !e.name.ends_with(".rmb.bin") { continue; }
        let d = ar.read(&e).unwrap();
        let (Some(g), Some(dt)) = (section(&d, b".gpu"), section(&d, b".data")) else { continue };
        let Ok(model) = fm2::parse_model(&d) else { continue };
        let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
        let shaders = sh::tail_shaders(gpu, model.tail_start);
        let groups = sh::data_groups(data);
        let pairs = sh::pair_groups(&groups, &shaders);
        if pairs.iter().any(|(v, p)| v.is_none() || p.is_none()) {
            println!("{}: tail from {:#x} gpu {:#x}", e.name, model.tail_start, g);
            for s in &shaders { println!("   tail {} @{:#x} dwords {}", if s.vertex { "VS" } else { "PS" }, s.code_offset, s.dwords); }
            let mut end = (model.tail_start + 15) & !15;
            for sref in &shaders {
                let start = sref.code_offset - 16 * sref.literals.len();
                let gap = &gpu[end.min(start)..start];
                let nz: Vec<usize> = (0..gap.len() / 16).filter(|k| gap[k * 16..k * 16 + 16].iter().any(|b| *b != 0)).collect();
                if !nz.is_empty() { println!("   gap {:#x}..{:#x}: {} nonzero 16-byte rows, first at {:#x}: {:02x?}", end, start, nz.len(), end + nz[0] * 16, &gap[nz[0] * 16..nz[0] * 16 + 16]); }
                end = sref.code_offset + sref.dwords * 4;
            }
            for (g, p) in groups.iter().zip(&pairs) { println!("   group ps {} vs {} -> {p:?}", g.ps_dwords, g.vs_dwords); }
            shown += 1;
            if shown >= max { break; }
        }
    }
}
