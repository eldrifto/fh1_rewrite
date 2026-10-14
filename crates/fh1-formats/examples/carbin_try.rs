//! carbin_try <file.carbin>: parse with the FH1 / FM4 and FM3 layouts and print what came out.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    println!("{} bytes, type {:#x}", d.len(), u32::from_be_bytes(d[..4].try_into().unwrap()));
    match fh1_formats::carbin::parse(&d) {
        Ok(c) => {
            println!("parsed: type {} complete {} sections {}", c.type_id, c.complete, c.sections.len());
            for s in c.sections.iter().take(40) {
                println!("  {:<28} lod {} verts, lod0 {} verts, {} subsections", s.name, s.lod_vertices.len(), s.lod0_vertices.len(), s.subsections.len());
            }
        }
        Err(e) => println!("parse error: {e}"),
    }
}
