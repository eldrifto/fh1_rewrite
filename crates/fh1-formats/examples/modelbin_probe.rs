//! Probe: FM6 `.modelbin` meshes. `modelbin_probe <folder> [out.obj]` decodes the LOD0 meshes of every modelbin
//! under the folder and checks them: decoded box against the stored box, and normals against the triangle
//! normals. With `out.obj` it also writes every LOD0 mesh into one Wavefront OBJ (write it to a git-ignored folder).
use fh1_formats::modelbin;
use std::io::Write;

fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if p.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).collect();
        v.sort();
        for e in v {
            walk(&e, out);
        }
    } else if p.extension().and_then(|s| s.to_str()) == Some("modelbin") {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    walk(std::path::Path::new(&a[1]), &mut files);
    let mut obj = a.get(2).map(|p| std::io::BufWriter::new(std::fs::File::create(p).unwrap()));
    let mut vbase = 1usize;
    let (mut ok, mut bad, mut meshes_total, mut tris_total) = (0, 0, 0, 0);
    let mut worst_box = 0f32;
    let mut cos_sum = 0f64;
    let mut cos_n = 0usize;
    for f in &files {
        let d = std::fs::read(f).unwrap();
        let name = f.strip_prefix(&a[1]).unwrap_or(f).display().to_string();
        let meshes = match modelbin::parse(&d) {
            Ok(m) => m,
            Err(e) => {
                println!("FAIL {name}: {e}");
                bad += 1;
                continue;
            }
        };
        let lod0: Vec<_> = meshes.iter().filter(|m| m.lod == 0 && m.material != 0xFFFF).collect();
        let mut tris = 0;
        for m in &lod0 {
            let p = &m.positions;
            let mut mn = [f32::MAX; 3];
            let mut mx = [f32::MIN; 3];
            for v in p {
                for k in 0..3 {
                    mn[k] = mn[k].min(v[k]);
                    mx[k] = mx[k].max(v[k]);
                }
            }
            if let Some(b) = m.bbox {
                for k in 0..3 {
                    worst_box = worst_box.max((mn[k] - b[k]).abs()).max((mx[k] - b[3 + k]).abs());
                }
            }
            for t in m.indices.chunks_exact(3) {
                let (a0, b0, c0) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                let u = [b0[0] - a0[0], b0[1] - a0[1], b0[2] - a0[2]];
                let w = [c0[0] - a0[0], c0[1] - a0[1], c0[2] - a0[2]];
                let fnr = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let fl = (fnr[0] * fnr[0] + fnr[1] * fnr[1] + fnr[2] * fnr[2]).sqrt();
                if fl > 1e-12 && m.normals.len() == p.len() {
                    let n = &m.normals;
                    let s = [0, 1, 2].map(|k| n[t[0] as usize][k] + n[t[1] as usize][k] + n[t[2] as usize][k]);
                    let sl = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt().max(1e-12);
                    cos_sum += ((fnr[0] * s[0] + fnr[1] * s[1] + fnr[2] * s[2]) / (fl * sl)) as f64;
                    cos_n += 1;
                }
            }
            tris += m.indices.len() / 3;
            if let Some(o) = obj.as_mut() {
                writeln!(o, "o {}_{}", name.replace(['\\', '/'], "_"), m.material).unwrap();
                for v in p {
                    writeln!(o, "v {} {} {}", v[0], v[1], v[2]).unwrap();
                }
                for n in &m.normals {
                    writeln!(o, "vn {} {} {}", n[0], n[1], n[2]).unwrap();
                }
                let has_n = m.normals.len() == p.len();
                for t in m.indices.chunks_exact(3) {
                    let [i, j, k] = [t[0], t[1], t[2]].map(|x| x as usize + vbase);
                    if has_n {
                        writeln!(o, "f {i}//{i} {j}//{j} {k}//{k}").unwrap();
                    } else {
                        writeln!(o, "f {i} {j} {k}").unwrap();
                    }
                }
                vbase += p.len();
            }
        }
        println!("ok   {name}: {} LOD0 meshes, {tris} triangles", lod0.len());
        meshes_total += lod0.len();
        tris_total += tris;
        ok += 1;
    }
    println!("{ok} files decoded, {bad} failed; {meshes_total} LOD0 meshes, {tris_total} triangles");
    println!("largest difference between decoded and stored box: {worst_box:.4} m");
    println!("normals vs triangle normals: mean cosine {:.3} over {cos_n} triangles", cos_sum / cos_n.max(1) as f64);
}
