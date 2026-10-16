//! Probe: every material's uv points drawn over a car atlas. `fm2_uv_overlay <car.zip> <out.png> [nodamage|lights] [material,material,...]`
use fh1_formats::{carbin_fm2, xds, zip::Archive};

const COLS: [[u8; 3]; 10] = [[255, 0, 0], [0, 255, 0], [0, 128, 255], [255, 255, 0], [255, 0, 255], [0, 255, 255], [255, 128, 0], [160, 80, 255], [255, 255, 255], [128, 255, 128]];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let atlas = a.get(3).map(String::as_str).unwrap_or("nodamage");
    let only: Option<Vec<&str>> = a.get(4).map(|s| s.split(',').collect());
    let find = |ar: &Archive<_>, n: &str| ar.entries.iter().find(|x| x.name.to_ascii_lowercase().ends_with(n)).cloned().unwrap();
    let e = find(&ar, ".carbin");
    let car = carbin_fm2::parse(&ar.read(&e).unwrap()).unwrap();
    let e = find(&ar, &format!("{atlas}.xds"));
    let (_, img) = xds::decode_base(&ar.read(&e).unwrap()).unwrap();
    let mut px = xds::to_rgba8(&img).unwrap();
    for p in px.chunks_exact_mut(4) {
        p[0] /= 3;
        p[1] /= 3;
        p[2] /= 3;
        p[3] = 255;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let mut legend: Vec<String> = Vec::new();
    for part in &car.parts {
        for r in part.runs.iter().filter(|r| r.lod == 1) {
            let m = part.materials.get(r.material).cloned().unwrap_or_default();
            if only.as_ref().is_some_and(|o| !o.contains(&m.as_str())) {
                continue;
            }
            if !legend.contains(&m) {
                legend.push(m.clone());
            }
            let c = COLS[legend.iter().position(|x| *x == m).unwrap() % COLS.len()];
            for &i in &r.indices {
                let uv = part.vertices[i as usize].uv;
                let (x, y) = ((uv[0] * w as f32) as i64, (uv[1] * h as f32) as i64);
                if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                    let o = (y as usize * w + x as usize) * 4;
                    px[o..o + 3].copy_from_slice(&c);
                }
            }
        }
    }
    for (i, m) in legend.iter().enumerate() {
        println!("{:?} = {m}", COLS[i % COLS.len()]);
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&a[2]).unwrap()), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
}
