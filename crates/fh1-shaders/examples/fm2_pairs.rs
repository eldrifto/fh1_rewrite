//! Probe: pair every draw of FM2 models with its tail vertex / pixel shader and print where each sampler's texture coordinate comes from
//! (vertex dword + literal scale). `cargo run -p fh1-shaders --example fm2_pairs -- <bin.zip> <model number>...`
use fh1_formats::{fm2, zip::Archive};
use fh1_shaders::fm2 as sh;

fn be(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    for num in a[2..].iter().filter_map(|v| v.parse::<usize>().ok()) {
        let suffix = format!(".{num:0>5}.rmb.bin");
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
        let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
        let model = fm2::parse_model(&d).unwrap();
        let shaders = sh::tail_shaders(gpu, model.tail_start);
        let (vss, pss): (Vec<_>, Vec<_>) = shaders.iter().partition(|s| s.vertex);
        for t in &shaders {
            println!("   tail {} @{:#x} literals {} dwords {}", if t.vertex { "VS" } else { "PS" }, t.code_offset, t.literals.len(), t.dwords);
        }
        let groups = sh::data_groups(data);
        for g in &groups { println!("   group ps {:#x} ({} dwords) vs {:?} ({} dwords) place2d {:?}", g.ps, g.ps_dwords, g.vs, g.vs_dwords, g.place2d); }
        let mats = sh::matrix_blocks(data);
        // Matrix blocks are consumed in group order, then constant table order.
        let mut next = 0;
        let mut group_mats: Vec<Vec<(u32, [[f32; 4]; 3])>> = Vec::new();
        for g in &groups {
            let mut v = Vec::new();
            for (_, reg) in &g.place2d {
                if let Some(m) = mats.get(next) {
                    v.push((*reg, *m));
                }
                next += 1;
            }
            group_mats.push(v);
        }
        println!("== model {num}: tail from {:#x}, {} VS + {} PS blocks, {} groups, {} matrices ({} wanted)", model.tail_start, vss.len(), pss.len(), groups.len(), mats.len(), next);
        for (i, d) in model.draws.iter().enumerate() {
            let gi = d.ps_container.and_then(|c| groups.iter().position(|g| g.ps == c));
            println!("  draw {i}: stride {} tris {} items {:?} group {gi:?}", d.stride, d.indices.len() / 3, d.items);
            let Some(gi) = gi else { continue };
            let (Some(vs), Some(ps)) = (vss.get(gi), pss.get(gi)) else { continue };
            let uv = sh::vs_uv_sources(vs);
            let samp = sh::ps_sampler_sources(ps);
            println!("     VS outputs {uv:?}  PS direct samplers {samp:?}");
            for (s, r) in &samp {
                let Some(u) = uv.get(r) else { println!("       sampler {s} <- r{r} <- (not plain)"); continue };
                // Resolve to an affine map on (h0, h1).
                let m = match u.map {
                    sh::UvMap::Scale(k) => Some([[k, 0.0, 0.0], [0.0, k, 0.0]]),
                    sh::UvMap::Raw => Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
                    sh::UvMap::Matrix { reg } => group_mats[gi].iter().find(|(r0, _)| *r0 == reg).map(|(_, m)| [[m[0][0], m[0][1], m[0][3]], [m[1][0], m[1][1], m[1][3]]]),
                };
                let Some(m) = m else { println!("       sampler {s} <- r{r} <- dword {} matrix without values", u.dword); continue };
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for v in 0..d.positions.len() {
                    let o = v * d.stride + u.dword as usize * 4;
                    let h = |k: usize| i16::from_be_bytes([d.vertices[o + k], d.vertices[o + k + 1]]) as f32;
                    let (h0, h1) = (h(0), h(2));
                    for c in 0..2 {
                        let val = m[c][0] * h0 + m[c][1] * h1 + m[c][2];
                        lo[c] = lo[c].min(val);
                        hi[c] = hi[c].max(val);
                    }
                }
                println!("       sampler {s} <- r{r} <- dword {} map {:?} => u {:.3}..{:.3} v {:.3}..{:.3}", u.dword, m, lo[0], hi[0], lo[1], hi[1]);
            }
        }
    }
}
