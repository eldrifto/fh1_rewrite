//! `carbin_fm2_dump <car.carbin> [out.obj]`: list the parts of an FM2 car (centre, vertices, runs) and optionally write a world-space OBJ of the LOD0 triangles.
use fh1_formats::carbin_fm2;
use std::fmt::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = carbin_fm2::parse(&std::fs::read(&a[1]).unwrap()).unwrap();
    let mut obj = String::new();
    let mut base = 1;
    let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &c.parts {
        let tris: usize = p.runs.iter().filter(|r| r.lod == 1).map(|r| r.indices.len() / 3).sum();
        println!("{:<16} centre {:?} verts {:>5} materials {:?} lod1 tris {}", p.name, p.centre.map(|c| c.map(|x| (x * 1000.0).round() / 1000.0)), p.vertices.len(), p.materials, tris);
        let Some(ce) = p.centre else { continue };
        writeln!(obj, "o {}", p.name).unwrap();
        for v in &p.vertices {
            let w = [v.position[0] + ce[0], v.position[1] + ce[1], v.position[2] + ce[2]];
            for k in 0..3 {
                mn[k] = mn[k].min(w[k]);
                mx[k] = mx[k].max(w[k]);
            }
            writeln!(obj, "v {} {} {}", w[0], w[1], w[2]).unwrap();
            writeln!(obj, "vt {} {}", v.uv[0], v.uv[1]).unwrap();
        }
        for r in p.runs.iter().filter(|r| r.lod == 1) {
            writeln!(obj, "usemtl {}", p.materials[r.material]).unwrap();
            for t in r.indices.chunks_exact(3) {
                let i = |k: usize| base + t[k];
                writeln!(obj, "f {0}/{0} {1}/{1} {2}/{2}", i(0), i(1), i(2)).unwrap();
            }
        }
        base += p.vertices.len() as u32;
    }
    println!("world bbox {mn:?} .. {mx:?}");
    if let Some(o) = a.get(2) {
        std::fs::write(o, obj).unwrap();
    }
}
