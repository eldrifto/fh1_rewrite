//! Prints an .xds texture's header and a few decoded pixels: `cargo run -p fh1-formats --example xds_info -- <Textures.zip> <entry substring>`
use fh1_formats::{xds, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let pat = a[2].to_ascii_lowercase();
    for e in ar.entries.clone().into_iter().filter(|e| e.name.to_ascii_lowercase().contains(&pat)) {
        let d = ar.read(&e).unwrap();
        let (h, img) = xds::decode_base(&d).unwrap();
        let rgba = xds::to_rgba8(&img).unwrap();
        println!("{}: {}x{} format {:?} fetch {:08x?}", e.name, h.width, h.height, h.format, h.fetch);
        let (w, hh) = (h.width as usize, h.height as usize);
        for (x, y) in [(5, hh / 2), (w / 3, hh / 2), (w / 2, hh / 2), (w - 5, hh / 2)] {
            let o = (y * w + x) * 4;
            println!("  ({x},{y}) rgba {:?}", &rgba[o..o + 4]);
        }
    }
}
