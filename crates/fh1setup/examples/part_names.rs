//! part_names <disc folder> <car MediaName> [fm3]: the carbin section names of one car.
//!
//! The node names an import produces are the carbin section names, so this is the source of
//! truth for "which parts does this car model keep separate" (docs/ATTACHMENT.md).
//! Reads `<disc>/media/cars/<MediaName>.zip` -> `<MediaName>.carbin` (and `_lod0`), and prints
//! every section with its triangle count at the finest LOD, so a zero-triangle placeholder is
//! visible rather than silently counted.
//!
//! Example:
//!   cargo run -p fh1setup --example part_names -- data/work_fm3_disc1/disc NIS_SkylineGTRVSpecII_02
//!
//! Read-only: it opens the disc and prints; it writes nothing.

use std::path::Path;

use fh1_formats::carbin::{self, Section};
use fh1_formats::zip::Archive;

fn tris(s: &Section) -> usize {
    let Some(min) = s.subsections.iter().map(|x| x.lod).min() else { return 0 };
    s.subsections.iter().filter(|x| x.lod == min).map(|x| x.indices.len() / 3).sum()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: part_names <disc folder> <car MediaName>");
        std::process::exit(2);
    }
    let disc = Path::new(&a[1]);
    let media = &a[2];
    let zip_path = disc.join("media/cars").join(format!("{media}.zip"));
    let Ok(mut ar) = Archive::open(&zip_path) else {
        eprintln!("cannot open {}", zip_path.display());
        std::process::exit(1);
    };

    for suffix in ["", "_lod0"] {
        let name = format!("{media}{suffix}.carbin");
        let Some(e) = ar.entries.iter().find(|e| e.name.eq_ignore_ascii_case(&name)).cloned() else {
            println!("{name}: (absent)");
            continue;
        };
        let data = ar.read(&e).unwrap();
        match carbin::parse(&data) {
            Ok(c) => {
                println!("{name}: {} sections, complete={}", c.sections.len(), c.complete);
                for s in &c.sections {
                    println!("  {:<28} {:>6} tris", s.name, tris(s));
                }
            }
            Err(e) => println!("{name}: parse error: {e}"),
        }
    }
}
