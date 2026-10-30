//! Probe: writes the six faces of a `.xpr` cube map side by side as a PPM and prints their mean colour.
//! `xpr_cube <staticCarCubemap.xpr> <out.ppm>`
use fh1_formats::{xds, xpr};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let res = xpr::parse(&d).unwrap();
    let r = res.iter().find(|r| &r.kind == b"TXCM").expect("no cube");
    let levels = xpr::decode(&d, r).unwrap();
    let faces = &levels[0];
    println!("{} {}x{} format {:?}, {} faces", r.name, r.header.width, r.header.height, r.header.format, faces.len());
    let (w, h) = (r.header.width as usize, r.header.height as usize);
    let mut sheet = vec![0u8; w * 6 * h * 3];
    for (i, f) in faces.iter().enumerate() {
        let rgba = xds::to_rgba8(f).unwrap();
        let mut sum = [0f64; 3];
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    let v = rgba[(y * w + x) * 4 + c];
                    sheet[(y * w * 6 + i * w + x) * 3 + c] = v;
                    sum[c] += v as f64;
                }
            }
        }
        let n = (w * h) as f64;
        println!("  face {i}: mean {:.0} {:.0} {:.0}", sum[0] / n, sum[1] / n, sum[2] / n);
    }
    let mut out = format!("P6\n{} {}\n255\n", w * 6, h).into_bytes();
    out.extend(sheet);
    std::fs::write(&a[2], out).unwrap();
}
