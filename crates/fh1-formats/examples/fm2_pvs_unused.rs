//! Probe: PVS texture records no model lists (candidates for the sky / pre models). `fm2_pvs_unused <track.pvs>`
use fh1_formats::fm2;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pvs = fm2::parse_pvs(&std::fs::read(&a[1]).unwrap()).unwrap();
    let used: std::collections::HashSet<u32> = pvs.models.iter().flatten().copied().collect();
    println!("{} textures, {} used by models", pvs.textures.len(), used.len());
    for (i, t) in pvs.textures.iter().enumerate().filter(|(i, _)| !used.contains(&(*i as u32))) {
        println!("unused {i}: file {:#x} flags {} object {} tile {:?}", t.file_id, t.flags, t.object, t.tile);
    }
}
