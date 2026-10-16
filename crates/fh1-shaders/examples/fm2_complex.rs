//! Probe: for direct pixel-shader samplers whose vertex output is not recognised, the vertex shader lines that write that output. `fm2_complex <bin.zip>`
use fh1_formats::{fm2, zip::Archive};
use fh1_shaders::fm2 as sh;
use std::collections::BTreeMap;
fn be(d: &[u8], o: usize) -> Option<u32> { d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) }
fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let mut seen: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for e in ar.entries.clone() {
        if !e.name.ends_with(".rmb.bin") { continue; }
        let d = ar.read(&e).unwrap();
        let (Some(g), Some(dt)) = (section(&d, b".gpu"), section(&d, b".data")) else { continue };
        let Ok(model) = fm2::parse_model(&d) else { continue };
        let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
        let shaders = sh::tail_shaders(gpu, model.tail_start);
        let groups = sh::data_groups(data);
        let pairs = sh::pair_groups(&groups, &shaders);
        for dr in &model.draws {
            let Some(gi) = dr.ps_container.and_then(|c| groups.iter().position(|g| g.ps == c)) else { continue };
            let (Some(vi), Some(pi)) = pairs[gi] else { continue };
            let (vs, ps) = (&shaders[vi], &shaders[pi]);
            let uv = sh::vs_uv_sources(vs);
            for (_, r) in sh::ps_sampler_sources(ps) {
                if uv.contains_key(&r) { continue; }
                // lines of the VS that mention o{r} as a destination, plus the fetches
                let pat = format!(" o{r}.");
                let lines: Vec<String> = vs.text.lines().filter(|l| l.contains(&pat) || l.contains("vfetch")).map(|l| l.trim().split_whitespace().skip(1).collect::<Vec<_>>().join(" ")).collect();
                let ent = seen.entry(lines.join(" | ")).or_insert((0, e.name.clone()));
                ent.0 += 1;
            }
        }
    }
    let mut v: Vec<_> = seen.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (k, (n, m)) in v.iter().take(14) { println!("{n:5} x {m}\n   {k}"); }
}
