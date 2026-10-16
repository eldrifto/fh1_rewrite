//! `fsb_survey <dir>`: parse every `.fsb` under a directory and summarise (version, samples, modes).
use std::collections::BTreeMap;
fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { walk(&p, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("fsb")) { out.push(p) }
    }
}
fn main() {
    let mut files = Vec::new();
    walk(std::path::Path::new(&std::env::args().nth(1).unwrap()), &mut files);
    let (mut ok, mut bad, mut samples) = (0, 0, 0);
    let mut modes: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let b = std::fs::read(f).unwrap();
        match fh1_audio::fsb::parse(&b) {
            Ok(s) => {
                ok += 1;
                samples += s.len();
                for x in &s { *modes.entry(format!("{:?} mode {:#010x} rate {} ch {}", String::from_utf8_lossy(&b[..4]), x.mode, x.rate, x.channels)).or_default() += 1; }
            }
            Err(e) => { bad += 1; if bad <= 8 { println!("{}: {e}", f.display()); } }
        }
    }
    println!("{ok} parsed, {bad} failed, {samples} samples");
    for (k, v) in modes { println!("{v:>7}  {k}"); }
}
