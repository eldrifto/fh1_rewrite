//! Probe: sizes of FM6 `.swatchbin` textures inside zips. `swatch_survey <zip or folder of zips> [max per zip]` prints one
//! line per texture: width, height, mips, format flags, bytes per element, data length, and the length the current
//! padding rule (every mip padded to whole 8x8-element tiles) predicts. Used to find the rule for small textures.
use fh1_formats::{grub, zip::Archive};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let max: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let p = std::path::Path::new(&a[1]);
    let mut zips: Vec<std::path::PathBuf> = if p.is_dir() {
        std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).filter(|q| q.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip"))).collect()
    } else {
        vec![p.to_path_buf()]
    };
    zips.sort();
    println!("name,w,h,mips,flags,elem_bytes,len,predicted");
    for z in zips {
        let Ok(mut ar) = Archive::open(&z) else { continue };
        let entries: Vec<_> = ar.entries.iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".swatchbin")).take(max).cloned().collect();
        for e in entries {
            let Ok(d) = ar.read(&e) else { continue };
            let Ok(g) = grub::parse(&d) else { continue };
            let Some(c) = g.chunks.iter().find(|c| c.tag == "TXCB") else { continue };
            let (Ok(meta), Ok(data)) = (g.meta_of(c), g.data_of(c)) else { continue };
            if meta.len() < 43 {
                continue;
            }
            let r = &meta[24..];
            let (w, h, mips) = (u16::from_le_bytes([r[0], r[1]]) as usize, u16::from_le_bytes([r[2], r[3]]) as usize, r[12] as usize);
            let flags = [r[15], r[16], r[17], r[18]];
            let (eb, block) = match (flags[0], flags[1]) {
                (0x0e, 0) | (0x6e, 0) => (8, 4),
                (0xae, 0) | (0x2e, 1) => (16, 4),
                (0xae, 1) | (0xad, 1) => (4, 1),
                _ => (0, 0),
            };
            let pred: usize = if eb == 0 {
                0
            } else {
                (0..mips)
                    .map(|i| {
                        let a = (w >> i).max(1).div_ceil(block);
                        let b = (h >> i).max(1).div_ceil(block);
                        a.div_ceil(8) * 8 * b.div_ceil(8) * 8 * eb
                    })
                    .sum()
            };
            let name = e.name.rsplit(['/', '\\']).next().unwrap_or(&e.name).to_string();
            println!("{name},{w},{h},{mips},{:02x}{:02x}{:02x}{:02x},{eb},{},{pred}", flags[0], flags[1], flags[2], flags[3], data.len());
        }
    }
}
