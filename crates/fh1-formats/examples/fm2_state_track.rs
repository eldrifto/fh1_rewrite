//! Probe: follows the render-state records (type 0x0c = `[state offset, value]`, offsets as `D3DRS_*`: 0x38 cull mode, 0x150 alpha-to-mask enable, 0x154 its offsets)
//! across the draws of FM2 models in stream order and counts the state each draw runs with. MODLOG 122.
//! `cargo run -p fh1-formats --example fm2_state_track -- <bin.zip>`   (a census over every model of the zip)
use fh1_formats::{fm2, zip::Archive};
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let mut census: BTreeMap<(Option<u32>, Option<u32>, Option<u32>), usize> = BTreeMap::new();
    for e in ar.entries.clone() {
        if !e.name.to_ascii_lowercase().ends_with(".rmb.bin") {
            continue;
        }
        let Ok(m) = fm2::parse_model(&ar.read(&e).unwrap()) else { continue };
        let mut cur: BTreeMap<u32, u32> = BTreeMap::new();
        for d in &m.draws {
            for (ty, w) in &d.state {
                if *ty == 0x0c && w.len() >= 2 {
                    cur.insert(w[0], w[1]);
                }
            }
            *census.entry((cur.get(&0x38).copied(), cur.get(&0x150).copied(), cur.get(&0x154).copied())).or_default() += 1;
        }
    }
    println!("(cull 0x38, alpha-to-mask 0x150, offsets 0x154): draws");
    for (k, n) in census {
        println!("{k:x?}: {n}");
    }
}
