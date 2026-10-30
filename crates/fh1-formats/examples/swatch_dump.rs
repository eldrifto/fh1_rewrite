//! Probe: FM6 `.swatchbin` -> PNG. `swatch_dump <folder or file> <out dir> [mip]` decodes mip 0 (or `mip`) of every
//! swatchbin found and writes `<name>.png`; prints what could not be decoded. Output goes to a git-ignored folder.
use fh1_formats::swatch;

fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if p.is_dir() {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            walk(&e.path(), out);
        }
    } else if p.extension().and_then(|s| s.to_str()) == Some("swatchbin") {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mip: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut files = Vec::new();
    walk(std::path::Path::new(&a[1]), &mut files);
    std::fs::create_dir_all(&a[2]).unwrap();
    let (mut ok, mut bad) = (0, 0);
    for f in &files {
        let d = std::fs::read(f).unwrap();
        let name = f.file_stem().unwrap().to_string_lossy().chars().take(40).collect::<String>();
        let res = swatch::parse(&d).and_then(|s| {
            let m = mip.min(s.mips - 1);
            let (w, h) = s.mip_pixels(m);
            s.decode_mip(m).map(|px| (s.format, s.mips, w, h, px))
        });
        match res {
            Ok((fmt, mips, w, h, px)) => {
                let file = std::fs::File::create(std::path::Path::new(&a[2]).join(format!("{name}.png"))).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&px).unwrap();
                println!("ok   {name}: {fmt:?} {w}x{h} ({mips} mips)");
                ok += 1;
            }
            Err(e) => {
                println!("FAIL {name}: {e}");
                bad += 1;
            }
        }
    }
    println!("{ok} decoded, {bad} not");
}
