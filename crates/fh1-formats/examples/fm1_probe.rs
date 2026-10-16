//! `cargo run -p fh1-formats --example fm1_probe -- <FM1 media folder> [car cab name]`: reads `Cars.ca2` and one `car_<name>.cab`.
use fh1_formats::fm1;

fn main() {
    let mut a = std::env::args().skip(1);
    let media = std::path::PathBuf::from(a.next().expect("usage: fm1_probe <media folder> [car_xxx.cab]"));
    let cab = a.next().unwrap_or_else(|| "car_abflugrx7.cab".into());
    let ca2 = fm1::unchunk(&std::fs::read(media.join("cabs/Cars.ca2")).unwrap());
    let entries = fm1::archive(&ca2).unwrap();
    let files = fm1::car_files(&entries);
    println!("Cars.ca2: {} files, {} cars, {} engines", entries.len(), files.cars.len(), files.engines.len());
    let entries = fm1::archive(&std::fs::read(media.join("cabs").join(&cab)).unwrap()).unwrap();
    println!("{cab}: {} files", entries.len());
    let blob = entries.iter().find(|e| fm1::is_car_blob(&e.data)).expect("no car blob");
    let car = fm1::parse_car(&blob.data).unwrap();
    println!("hubs {:?}", car.hubs);
    for p in &car.parts {
        let l1: usize = p.subparts.iter().filter(|s| s.lod == 1).map(|s| s.indices.len() / 3).sum();
        let mats: Vec<_> = p.subparts.iter().filter(|s| s.lod == 1).map(|s| fm1::MATERIALS[s.material as usize]).collect();
        println!("  {} verts {} lod1 tris {} {:?}", p.name, p.vertices.len(), l1, mats);
    }
    for e in &entries {
        if let Some((fmt, mips, w, h, d)) = fm1::xpr_info(&e.data) {
            println!("xpr fmt {fmt:#x} mips {mips} {w}x{h}x{d} decode {}", fm1::xpr_decode(&e.data).is_some());
        }
    }
}
