//! `spectator_atlas <Spectators.zip> <out.ppm>`: the sprite atlas (`sprites.xds`) as a PPM over magenta, and its size.
use fh1_formats::{crowd, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let e = ar.entries.iter().find(|e| e.name.eq_ignore_ascii_case("sprites.xds")).unwrap().clone();
    let (w, h, rgba) = crowd::decode_dxt1_alpha(&ar.read(&e).unwrap()).unwrap();
    println!("{w} x {h}");
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in rgba.chunks_exact(4) {
        let t = p[3] as f32 / 255.0;
        out.extend([0, 1, 2].map(|k| (p[k] as f32 * t + if k != 1 { 255.0 * (1.0 - t) } else { 0.0 }) as u8));
    }
    std::fs::write(&a[2], out).unwrap();
}
