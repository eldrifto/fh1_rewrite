//! Probe: the sky model of an FM2 track (`<track>out.sky.rmb.bin`): its draws, bounding boxes and texture items.
//! `cargo run -p fh1-formats --example fm2_sky -- <bin.zip>`
use fh1_formats::{fm2, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".sky.rmb.bin")).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let m = fm2::parse_model(&d).unwrap();
    println!("{}: {} draws, skipped {:?}", e.name, m.draws.len(), m.skipped);
    for (i, dr) in m.draws.iter().enumerate() {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &dr.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        println!("draw {i}: stride {} verts {} tris {} items {:?} bbox {lo:.0?}..{hi:.0?} ps {:?}", dr.stride, dr.positions.len(), dr.indices.len() / 3, dr.items, dr.ps_container);
    }
}
