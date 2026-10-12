//! Scratch probe: mean/max per-channel difference of two same-size PNGs.
//! `cargo run -p fh1-formats --example png_diff -- a.png b.png`
fn load(p: &str) -> (u32, u32, Vec<u8>) {
    let d = std::fs::read(p).unwrap();
    let dec = png::Decoder::new(std::io::Cursor::new(d));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    let (w, h) = (info.width, info.height);
    // Normalise to RGBA8.
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        other => panic!("color type {other:?}"),
    };
    (w, h, rgba)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (wa, ha, pa) = load(&a[1]);
    let (wb, hb, pb) = load(&a[2]);
    assert_eq!((wa, ha), (wb, hb));
    let n = pa.len() / 4;
    let (mut sum, mut mx, mut changed) = (0f64, 0i32, 0usize);
    let mut chan = [0f64; 3];
    for i in 0..n {
        let mut dmax = 0i32;
        for c in 0..3 {
            let d = (pa[i * 4 + c] as i32 - pb[i * 4 + c] as i32).abs();
            chan[c] += d as f64;
            dmax = dmax.max(d);
        }
        sum += dmax as f64;
        if dmax > 8 {
            changed += 1;
        }
        mx = mx.max(dmax);
    }
    println!("{wa}x{ha}: mean max-channel diff {:.2}, max {mx}, pixels >8: {changed} ({:.1}%)", sum / n as f64, 100.0 * changed as f64 / n as f64);
    println!("per-channel mean R {:.2} G {:.2} B {:.2}", chan[0] / n as f64, chan[1] / n as f64, chan[2] / n as f64);
}
