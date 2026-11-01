//! texraw: byte-level comparison between disc BC4 textures and Xenia's guest texture dumps.
//!
//! usage: texraw <disc_dir> <dump_dir> <format_raw> <w> <h>
//!
//! The disc DDS holds the *untiled* BC4 blocks; the guest dump holds the *tiled* guest bytes.
//! Re-tiling the disc payload with the same Xenos tiling (`xds::tiled_offset_2d`) must reproduce
//! the guest bytes exactly when the game stores the texture unchanged -- that is the direct test
//! for a load-time conversion, with no decoding in between.
//!
//! Also tries the three guest endian modes, since the fetch constant's endianness decides whether
//! 16/32-bit components are swapped on the way in.

use std::fs;
use std::path::PathBuf;

use fh1_formats::xds::{self, Format};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!("usage: texraw <disc_dir> <dump_dir> <format_raw> <w> <h>");
        std::process::exit(1);
    }
    let disc_dir = PathBuf::from(&args[0]);
    let dump_dir = PathBuf::from(&args[1]);
    let gfmt: u32 = args[2].parse().expect("format");
    let w: u32 = args[3].parse().expect("width");
    let h: u32 = args[4].parse().expect("height");

    let (bw, bpb) = Format::from_raw(gfmt).block().expect("guest format block");
    let blocks_w = w.div_ceil(bw);
    let blocks_h = h.div_ceil(bw);
    let block_bytes = (blocks_w * blocks_h * bpb) as usize;
    let log_bpb = bpb.trailing_zeros();

    // Load every 512x512 BC4 disc texture and re-tile it into guest order.
    let mut discs: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in fs::read_dir(&disc_dir).expect("disc dir").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("dds") {
            continue;
        }
        let dds = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if dds.len() < 148 {
            continue;
        }
        let dw = u32::from_le_bytes(dds[16..20].try_into().unwrap());
        let dh = u32::from_le_bytes(dds[12..16].try_into().unwrap());
        if (dw, dh) != (w, h) {
            continue;
        }
        let dxgi = u32::from_le_bytes(dds[128..132].try_into().unwrap());
        if dxgi != 80 {
            continue;
        }
        let payload = match dds.get(148..148 + block_bytes) {
            Some(p) => p,
            None => continue,
        };
        let mut tiled = vec![0u8; block_bytes];
        for y in 0..blocks_h {
            for x in 0..blocks_w {
                let src = ((y * blocks_w + x) * bpb) as usize;
                let dst = (xds::tiled_offset_2d(x, y, blocks_w, log_bpb) * bpb) as usize;
                tiled[dst..dst + bpb as usize]
                    .copy_from_slice(&payload[src..src + bpb as usize]);
            }
        }
        discs.push((path.file_name().unwrap().to_string_lossy().to_string(), tiled));
    }
    println!("re-tiled {} disc textures of {w}x{h} fmt {gfmt}", discs.len());

    let mut checked = 0usize;
    let mut best: Vec<(f32, String, String, u32)> = Vec::new();
    for entry in fs::read_dir(&dump_dir).expect("dump dir").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() < 3 || parts[1] != format!("f{gfmt}") || !name.ends_with(&format!("_{w}x{h}.bin")) {
            continue;
        }
        let guest = match fs::read(entry.path()) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if guest.len() < block_bytes {
            continue;
        }
        checked += 1;
        for (dname, tiled) in &discs {
            for endian in 0..4u32 {
                let mut g = guest[..block_bytes].to_vec();
                match endian {
                    1 => g.chunks_exact_mut(2).for_each(|c| c.swap(0, 1)),
                    2 => g.chunks_exact_mut(4).for_each(|c| c.reverse()),
                    3 => g.chunks_exact_mut(4).for_each(|c| c.swap(0, 2)),
                    _ => {}
                }
                let same = tiled.iter().zip(&g).filter(|(a, b)| a == b).count();
                let frac = same as f32 / block_bytes as f32;
                best.push((frac, name.clone(), dname.clone(), endian));
            }
        }
    }
    best.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("checked {checked} dumps against {} discs x 4 endian modes", discs.len());
    println!("{:>8} {:>26} {:>22} {:>7}", "same", "dump", "disc", "endian");
    for (f, d, s, e) in best.iter().take(12) {
        println!("{:>8.4} {:>26} {:>22} {:>7}", f, d, s, e);
    }
}
