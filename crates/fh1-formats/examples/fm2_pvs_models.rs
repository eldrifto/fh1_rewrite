//! Probe: a PVS model's texture list and the 12 words + marker after it. `fm2_pvs_models <track.pvs> <model>...`
use fh1_formats::fm2;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pvs = fm2::parse_pvs(&std::fs::read(&a[1]).unwrap()).unwrap();
    if a.len() == 2 {
        let mut h: std::collections::BTreeMap<String, usize> = Default::default();
        for (w, mark) in &pvs.model_extra {
            *h.entry(format!("{:08x} | {:08x} {:08x} {:08x} {:08x} | {mark:x}", w[0], w[1], w[4], w[8], w[10])).or_default() += 1;
        }
        for (k, n) in h { println!("{n:5} {k}"); }
        return;
    }
    for m in a[2..].iter().filter_map(|v| v.parse::<usize>().ok()) {
        let (w, mark) = pvs.model_extra[m];
        let f: Vec<String> = w.iter().map(|x| format!("{:08x}({})", x, f32::from_bits(*x))).collect();
        println!("model {m}: tex {:?}\n   {}\n   marker {mark:08x}", pvs.models[m], f.join(" "));
    }
}
