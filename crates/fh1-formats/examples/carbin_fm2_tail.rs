//! `carbin_fm2_tail <car.carbin> <out.csv>`: per vertex of the LOD1 triangles: part, world position, triangle-averaged normal and the 44 undecoded tail bytes (hex).
use fh1_formats::carbin_fm2;
use std::fmt::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = carbin_fm2::parse(&std::fs::read(&a[1]).unwrap()).unwrap();
    let mut out = String::new();
    for p in &c.parts {
        let Some(ce) = p.centre else { continue };
        let mut n = vec![[0.0f32; 3]; p.vertices.len()];
        for r in p.runs.iter().filter(|r| r.lod == 1) {
            for t in r.indices.chunks_exact(3) {
                let v = |k: usize| p.vertices[t[k] as usize].position;
                let (a, b, c) = (v(0), v(1), v(2));
                let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                let f = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                for k in 0..3 {
                    for j in 0..3 {
                        n[t[k] as usize][j] += f[j];
                    }
                }
            }
        }
        for (i, v) in p.vertices.iter().enumerate() {
            let l = (n[i][0].powi(2) + n[i][1].powi(2) + n[i][2].powi(2)).sqrt().max(1e-9);
            let hex: String = v.tail.iter().map(|b| format!("{b:02x}")).collect();
            writeln!(out, "{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{}", p.name, i, v.position[0] + ce[0], v.position[1] + ce[1], v.position[2] + ce[2], n[i][0] / l, n[i][1] / l, n[i][2] / l, hex).unwrap();
        }
    }
    std::fs::write(&a[2], out).unwrap();
}
