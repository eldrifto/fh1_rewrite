//! Probe: decode the base mip of an `.xds` to a PNG.
//! `cargo run -p fh1-formats --example xds_png -- in.xds out.png [out.rgba]`
use fh1_formats::xds;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (h, img) = xds::decode_base(&std::fs::read(&a[1]).unwrap()).unwrap();
    println!("{}x{} {:?}", h.width, h.height, img.format);
    let rgba = xds::to_rgba8(&img).unwrap();
    if let Some(raw) = a.get(3) {
        std::fs::write(raw, &rgba).unwrap();
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&a[2]).unwrap()), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
}
