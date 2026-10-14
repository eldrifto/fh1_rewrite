//! Probe: per material name over every FM2 car zip, how its uv (value / 1280) sits on the car's `nodamage.xds` atlas.
//! `cargo run -p fh1-formats --release --example fm2_mat_survey -- <Media/cars dir>`
use fh1_formats::{carbin_fm2, xds, zip::Archive};
use std::collections::BTreeMap;

#[derive(Default)]
struct Acc {
    cars: usize,
    verts: usize,
    in01: usize,
    on_opaque: usize,
    spread_small: usize,
    parts: BTreeMap<String, usize>,
}

fn main() {
    let dir = std::env::args().nth(1).expect("cars dir");
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "zip") || p.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("shared")) {
            continue;
        }
        let Ok(mut ar) = Archive::open(&p) else { continue };
        let find = |ar: &Archive<_>, n: &str| ar.entries.iter().find(|x| x.name.to_ascii_lowercase().ends_with(n)).cloned();
        let (Some(cb), Some(nd)) = (find(&ar, ".carbin"), find(&ar, "nodamage.xds")) else { continue };
        let Ok(car) = carbin_fm2::parse(&ar.read(&cb).unwrap()) else { continue };
        let Ok((_, img)) = xds::decode_base(&ar.read(&nd).unwrap()) else { continue };
        let Ok(rgba) = xds::to_rgba8(&img) else { continue };
        let (w, h) = (img.width as usize, img.height as usize);
        let mut seen: Vec<String> = Vec::new();
        for part in &car.parts {
            for r in part.runs.iter().filter(|r| r.lod == 1) {
                let m = part.materials.get(r.material).cloned().unwrap_or_default();
                let a = acc.entry(m.clone()).or_default();
                if !seen.contains(&m) {
                    seen.push(m.clone());
                    a.cars += 1;
                }
                *a.parts.entry(part.name.trim_end_matches(|c: char| c.is_ascii_lowercase() && part.name.len() > 4).to_owned()).or_default() += 1;
                let mut used: Vec<u32> = r.indices.clone();
                used.sort_unstable();
                used.dedup();
                let (mut mn, mut mx) = ([f32::MAX; 2], [f32::MIN; 2]);
                for &i in &used {
                    let uv = part.vertices[i as usize].uv;
                    a.verts += 1;
                    for k in 0..2 {
                        mn[k] = mn[k].min(uv[k]);
                        mx[k] = mx[k].max(uv[k]);
                    }
                    if (0.0..1.0).contains(&uv[0]) && (0.0..1.0).contains(&uv[1]) {
                        a.in01 += 1;
                        let (x, y) = ((uv[0] * w as f32) as usize, (uv[1] * h as f32) as usize);
                        if rgba[(y * w + x) * 4 + 3] > 128 {
                            a.on_opaque += 1;
                        }
                    }
                }
                if used.len() > 0 && (mx[0] - mn[0]).max(mx[1] - mn[1]) < 0.02 {
                    a.spread_small += used.len();
                }
            }
        }
    }
    println!("{:<22} {:>5} {:>8} {:>7} {:>9} {:>8}  top parts", "material", "cars", "verts", "in01", "onOpaque", "tinyUV");
    for (m, a) in &acc {
        let mut parts: Vec<_> = a.parts.iter().collect();
        parts.sort_by(|x, y| y.1.cmp(x.1));
        let top: Vec<String> = parts.iter().take(5).map(|(n, c)| format!("{n}:{c}")).collect();
        let f = |x: usize| x as f32 / a.verts.max(1) as f32;
        println!("{:<22} {:>5} {:>8} {:>7.2} {:>9.2} {:>8.2}  {}", m, a.cars, a.verts, f(a.in01), f(a.on_opaque), f(a.spread_small), top.join(" "));
    }
}
