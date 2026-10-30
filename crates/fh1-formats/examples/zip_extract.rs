//! Probe: unpack a Forza zip into a folder: `zip_extract <zip> <out dir> [name filter]` (use a git-ignored folder).
//! With a filter, only entries whose name contains it (ignoring case) are written.
use fh1_formats::zip::Archive;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let filter = a.get(3).map(|s| s.to_lowercase());
    let entries: Vec<_> = ar.entries.iter().filter(|e| filter.as_ref().is_none_or(|f| e.name.to_lowercase().contains(f))).cloned().collect();
    for e in &entries {
        let data = match ar.read(e) {
            Ok(d) => d,
            Err(err) => {
                println!("FAIL {} ({err})", e.name);
                continue;
            }
        };
        let path = std::path::Path::new(&a[2]).join(&e.name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }
    println!("{} entries", entries.len());
}
