//! Probe: a model's groups with their `c_place2d_*` registers and the placement matrix blocks found in `.data`. `fm2_mats <bin.zip> <model>`
use fh1_formats::{fm2, zip::Archive};
use fh1_shaders::fm2 as sh;
fn be(d: &[u8], o: usize) -> Option<u32> { d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) }
fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let suffix = format!(".{:0>5}.rmb.bin", a[2].parse::<usize>().unwrap());
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
    let _ = fm2::parse_model(&d).unwrap();
    let data = &d[0x190 + g..0x190 + g + dt];
    for (i, g) in sh::data_groups(data).iter().enumerate() {
        println!("group {i}: ps @{:#x} ({} dw) vs {:?} ({} dw) place2d {:?}", g.ps, g.ps_dwords, g.vs, g.vs_dwords, g.place2d);
    }
    for g in sh::data_groups(data) {
        if let Some(vs) = g.vs {
            let mut copy = data[vs..].to_vec();
            copy[3] = 1;
            if let Ok(b) = fh1_shaders::container::ShaderBlob::parse(&copy, vs) {
                for c in b.constants.iter().filter(|c| c.name.starts_with("c_place2d")) {
                    let d: Option<Vec<f32>> = c.default.as_ref().map(|w| w.iter().map(|x| f32::from_bits(*x)).collect());
                    println!("  VS@{vs:#x} {} reg {} count {} rows {} cols {} default {:?}", c.name, c.register, c.count, c.rows, c.columns, d);
                }
            }
        }
    }
    // Every aligned 4x4 block whose last row is (0,0,0,1), accepted or not.
    let f = |o: usize| (0..4).map(|k| f32::from_bits(be(data, o + 4 * k).unwrap_or(0))).collect::<Vec<_>>();
    let mut o = 0;
    while o + 64 <= data.len() {
        let r: Vec<Vec<f32>> = (0..4).map(|k| f(o + 16 * k)).collect();
        if r[3] == [0.0, 0.0, 0.0, 1.0] && r[0].iter().all(|x| x.is_finite()) && r[0].iter().any(|x| *x != 0.0) && r.iter().flatten().all(|x| x.is_finite() && x.abs() < 1e6) {
            println!("  block @{o:#x}: {:?} {:?} {:?}", r[0], r[1], r[2]);
        }
        o += 4;
    }
    println!("accepted by matrix_blocks: {}", sh::matrix_blocks(data).len());
}
