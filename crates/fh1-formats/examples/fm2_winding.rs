//! Probe: for FM2 draws whose vertices carry a packed normal (stride 16, normal at +12) compare the triangles' geometric normals with it, and print
//! the draw's render-state stream records (`0x05` / `0x08` / `0x0c` / `0x1d`), to find which state flips the winding.
//! `cargo run -p fh1-formats --example fm2_winding -- <bin.zip> [max models]`
use fh1_formats::fm2;
use fh1_formats::zip::Archive;
use std::collections::BTreeMap;

fn dec3n(w: u32) -> [f32; 3] {
    let sx = |v: u32, bits: u32| {
        let v = v & ((1 << bits) - 1);
        (if v >= 1 << (bits - 1) { v as i32 - (1 << bits) } else { v as i32 }) as f32
    };
    [sx(w, 11) / 1023.0, sx(w >> 11, 11) / 1023.0, sx(w >> 22, 10) / 511.0]
}

fn s8(w: u32) -> [f32; 3] {
    let b = |s: u32| (((w >> s) & 255) as u8 as i8) as f32 / 127.0;
    [b(24), b(16), b(8)]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let max: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    // (state signature) -> (draws, draws whose winding agrees with the normals, draws that disagree)
    let mut by_state: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut models = 0;
    // heuristic confusion: [truth as-is correct?][prediction as-is correct?] for (signed volume, up majority)
    let mut vol = [[0usize; 2]; 2];
    let mut up = [[0usize; 2]; 2];
    for e in ar.entries.clone() {
        if !e.name.to_ascii_lowercase().ends_with(".rmb.bin") {
            continue;
        }
        let Ok(m) = fm2::parse_model(&ar.read(&e).unwrap()) else { continue };
        models += 1;
        if models > max {
            break;
        }
        for d in m.draws.iter().filter(|d| d.stride == 16) {
            let mut best: Option<(f32, f32)> = None;
            for dec in [dec3n as fn(u32) -> [f32; 3], s8] {
                let (mut agree, mut tot) = (0usize, 0usize);
                for t in d.indices.chunks_exact(3) {
                    let [p, q, r] = [t[0], t[1], t[2]].map(|i| d.positions[i as usize]);
                    let (u, v) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
                    let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    if f.iter().map(|x| x * x).sum::<f32>() < 1e-12 {
                        continue;
                    }
                    let mut n = [0.0f32; 3];
                    for i in [t[0], t[1], t[2]] {
                        let w = u32::from_be_bytes(d.vertices[i as usize * 16 + 12..i as usize * 16 + 16].try_into().unwrap());
                        let nn = dec(w);
                        for k in 0..3 {
                            n[k] += nn[k];
                        }
                    }
                    tot += 1;
                    agree += (f[0] * n[0] + f[1] * n[1] + f[2] * n[2] > 0.0) as usize;
                }
                if tot >= 4 {
                    let frac = agree as f32 / tot as f32;
                    if best.is_none_or(|b| (frac - 0.5).abs() > (b.0 - 0.5).abs()) {
                        best = Some((frac, tot as f32));
                    }
                }
            }
            let Some((frac, _)) = best else { continue };
            if frac > 0.1 && frac < 0.9 {
                continue; // inconclusive
            }
            let truth = (frac >= 0.9) as usize;
            // signed volume about the centroid and up-facing majority of the horizontal-ish faces
            let c = d.positions.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
            let n = d.positions.len().max(1) as f32;
            let c = [c[0] / n, c[1] / n, c[2] / n];
            let (mut v, mut u, mut dn) = (0.0f64, 0, 0);
            for t in d.indices.chunks_exact(3) {
                let [p, q, r] = [t[0], t[1], t[2]].map(|i| {
                    let x = d.positions[i as usize];
                    [(x[0] - c[0]) as f64, (x[1] - c[1]) as f64, (x[2] - c[2]) as f64]
                });
                v += p[0] * (q[1] * r[2] - q[2] * r[1]) - p[1] * (q[0] * r[2] - q[2] * r[0]) + p[2] * (q[0] * r[1] - q[1] * r[0]);
                let (e, f) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
                let nn = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
                let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt();
                if l > 1e-9 && nn[1].abs() > 0.7 * l {
                    if nn[1] > 0.0 { u += 1 } else { dn += 1 }
                }
            }
            vol[truth][(v > 0.0) as usize] += 1;
            if u + dn > 0 {
                up[truth][(u >= dn) as usize] += 1;
            }
            let sig: Vec<String> = d.state.iter().map(|(t, w)| format!("{t:02x}:{}", w.iter().map(|x| format!("{x:x}")).collect::<Vec<_>>().join("."))).collect();
            let e = by_state.entry(sig.join(" ")).or_default();
            e.0 += 1;
            if frac >= 0.9 {
                e.1 += 1;
            } else {
                e.2 += 1;
            }
        }
    }
    println!("signed-volume heuristic [truth wrong/right][predicted wrong/right] = {vol:?}; up-majority = {up:?}");
    println!("{models} models; state records between draws -> (draws, as-is agrees with normals, flipped)");
    for (k, v) in &by_state {
        println!("{v:?}  {k}");
    }
}
