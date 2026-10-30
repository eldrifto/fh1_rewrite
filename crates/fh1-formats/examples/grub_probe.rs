//! Probe: FM6 `burG` containers. `grub_probe <folder> [-v]` checks every .modelbin/.swatchbin/.materialbin
//! under the folder against the layout rules in `fh1_formats::grub`; `-v` lists each file's chunks.
use fh1_formats::grub;
use std::collections::BTreeMap;

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if matches!(p.extension().and_then(|s| s.to_str()), Some("modelbin" | "swatchbin" | "materialbin")) {
            out.push(p);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let verbose = a.iter().any(|s| s == "-v");
    let mut files = Vec::new();
    walk(std::path::Path::new(&a[1]), &mut files);
    let (mut ok, mut bad) = (0, 0);
    let mut tags: BTreeMap<String, (usize, u64, Vec<u32>)> = BTreeMap::new();
    for f in &files {
        let d = std::fs::read(f).unwrap();
        let g = match grub::parse(&d) {
            Ok(g) => g,
            Err(e) => {
                println!("PARSE FAIL {}: {e}", f.display());
                bad += 1;
                continue;
            }
        };
        let probs = g.problems();
        if probs.is_empty() { ok += 1 } else { bad += 1; println!("{}: {:?}", f.display(), probs) }
        for c in &g.chunks {
            let t = tags.entry(c.tag.clone()).or_default();
            t.0 += 1;
            t.1 += c.size as u64;
            if !t.2.contains(&c.version) { t.2.push(c.version) }
            if verbose {
                println!("{} {} v{:#x} meta {}@{:#x} data {}@{:#x}", f.file_name().unwrap().to_string_lossy(), c.tag, c.version, c.meta_len, c.meta_offset, c.size, c.data_offset);
            }
        }
    }
    println!("{} files: {ok} layout ok, {bad} with problems", files.len());
    for (t, (n, bytes, vers)) in &tags {
        println!("  {t}: {n} chunks, {bytes} bytes, versions {vers:x?}");
    }
}
