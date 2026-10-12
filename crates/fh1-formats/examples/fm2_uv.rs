//! Scratch probe: for each FM2 model's draws, the value range of every vertex uv pair, and the lightmap-atlas tiles the
//! draw binds. `cargo run -p fh1-formats --example fm2_uv -- <bin.zip> <track.pvs> [model ...]`.
use fh1_formats::{fm2, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (zip, pvs_path) = (&a[1], &a[2]);
    let want: Vec<usize> = a[3..].iter().filter_map(|s| s.parse().ok()).collect();
    let pvs = fm2::parse_pvs(&std::fs::read(pvs_path).unwrap()).unwrap();
    println!("pvs: {} textures", pvs.textures.len());
    let mut ar = Archive::open(zip).unwrap();
    for e in ar.entries.clone() {
        let name = e.name.to_ascii_lowercase();
        let Some(number) = name.strip_suffix(".rmb.bin").and_then(|s| s.rsplit('.').next()).and_then(|s| s.parse::<usize>().ok()) else { continue };
        if !want.is_empty() && !want.contains(&number) {
            continue;
        }
        let m = match fm2::parse_model(&ar.read(&e).unwrap()) {
            Ok(m) => m,
            Err(err) => {
                println!("model {number}: parse {err}");
                continue;
            }
        };
        let list = pvs.models.get(number).cloned().unwrap_or_default();
        for (di, d) in m.draws.iter().enumerate() {
            let lms: Vec<&fm2::Texture> = d
                .items
                .iter()
                .filter_map(|&it| list.get(it as usize).and_then(|&t| pvs.textures.get(t as usize)))
                .filter(|t| matches!(t.flags, 8 | 9 | 12))
                .collect();
            if lms.is_empty() {
                continue;
            }
            let mut pairs = String::new();
            let mut o = 12;
            while o + 4 <= d.stride {
                let uv = d.uv_at(o);
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for p in &uv {
                    for k in 0..2 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                pairs.push_str(&format!(" +{o}:[{:.4}..{:.4},{:.4}..{:.4}]", lo[0], hi[0], lo[1], hi[1]));
                o += 2;
            }
            let tiles: Vec<String> = lms.iter().map(|t| format!("f{}({:.5},{:.5},{:.5},{:.5})", t.flags, t.tile[0], t.tile[1], t.tile[2], t.tile[3])).collect();
            println!("model {number} draw {di}: stride {} n {} items {:?} lmtiles {:?}{}", d.stride, d.positions.len(), d.items, tiles, pairs);
        }
    }
}
