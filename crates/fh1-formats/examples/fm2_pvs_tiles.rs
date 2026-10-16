//! Probe: distribution of the PVS texture records' f32 / u32 pairs by flags. `fm2_pvs_tiles <track.pvs>`
use fh1_formats::fm2;
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pvs = fm2::parse_pvs(&std::fs::read(&a[1]).unwrap()).unwrap();
    let mut m: BTreeMap<(u32, String), usize> = BTreeMap::new();
    for t in &pvs.textures {
        *m.entry((t.flags, format!("{:?}", t.tile))).or_default() += 1;
    }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for ((f, t), n) in v.iter().take(40) { println!("{n:5} flags {f:2} tile {t}"); }
    println!("{} textures, {} models", pvs.textures.len(), pvs.models.len());
}
