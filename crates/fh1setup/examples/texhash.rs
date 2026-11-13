//! texhash: match a texture fetch in a constants dump to the importer's DDS file, by hash.
//!
//! The instrumented Xenia writes, per draw, `texfetch ... addr A WxH fmt F bytes N hash H prefix P`, where the hash is
//! FNV-1a over the guest bytes the game stored (tiled, level 0). This tool reads the dump, and for every DDS in the
//! textures directory with the same dimensions and a compatible format, re-tiles level 0 with the same Xenos rule
//! (`xds::tiled_offset_2d`) and computes the same hash. A match is a proof, not a guess.
//!
//! usage: texhash <dump> <textures_dir> [--max-draws N] [--guest-dir <XE_TEXDUMP dir>]
//!
//! With `--guest-dir`, the fingerprint is computed from the guest bytes Xenia dumped (`<base>_f<fmt>_<w>x<h>.bin`,
//! whose index carries Xenia's own decode), instead of from the dump's hash -- the dump's hash cannot see the guest
//! window (MODLOG 381), while the dumped file is the texture itself.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use fh1_formats::xds::{self, Format};

fn fnv(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for chunk in bytes.chunks_exact(4) {
        let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        h = (h ^ w).wrapping_mul(16777619);
    }
    h
}

/// (block edge in texels, bytes per block) for a Xenos format code, as the importer's `xds::Format::block`.
fn xenos_block(fmt: u32) -> (u32, u32) {
    match fmt {
        59 | 58 | 18 | 51 => (4, 8),
        20 | 53 | 19 | 52 => (4, 16),
        6 | 50 | 2 => (1, 4),
        _ => (4, 16),
    }
}

/// The DDS files the project writes use DX10 headers; level 0 starts at 148.
fn dds_dims(path: &Path) -> Option<(u32, u32, u32, Vec<u8>)> {
    let b = fs::read(path).ok()?;
    if b.len() < 148 {
        return None;
    }
    let h = u32::from_le_bytes(b[12..16].try_into().ok()?);
    let w = u32::from_le_bytes(b[16..20].try_into().ok()?);
    let dxgi = u32::from_le_bytes(b[128..132].try_into().ok()?);
    Some((w, h, dxgi, b[148..].to_vec()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: texhash <dump> <textures_dir> [--max-draws N]");
        std::process::exit(1);
    }
    let dump_path = &args[0];
    let tex_dir = PathBuf::from(&args[1]);
    let guest_dir: Option<PathBuf> = args
        .iter()
        .position(|a| a == "--guest-dir")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    // base -> (fmt, w, h), from Xenia's own index when a guest dir is given
    let mut index: HashMap<u32, (u32, u32, u32, u32)> = HashMap::new();
    if let Some(gd) = &guest_dir {
        if let Ok(txt) = fs::read_to_string(gd.join("dump_index.txt")) {
            for line in txt.lines() {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 8 {
                    continue;
                }
                let base = u32::from_str_radix(f[0].split('_').next().unwrap_or(""), 16).unwrap_or(0);
                let kv = |k: &str| -> Option<u32> {
                    f.iter().position(|x| *x == k).and_then(|i| f.get(i + 1)).and_then(|v| v.parse().ok())
                };
                let fmt: u32 = kv("fmt").unwrap_or(0);
                let w: u32 = kv("w").unwrap_or(0);
                let h: u32 = kv("h").unwrap_or(0);
                let pitch: u32 = kv("pitch").unwrap_or(w);
                if base != 0 {
                    index.entry(base).or_insert((fmt, w, h, pitch));
                }
            }
        }
        println!("guest index: {} textures", index.len());
    }
    let max_draws: usize = args
        .iter()
        .position(|a| a == "--max-draws")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);

    // Every texture the importer wrote, by dimensions: (w, h) -> [(file, dxgi, payload)]
    let mut by_dim: HashMap<(u32, u32), Vec<(String, u32, Vec<u8>)>> = HashMap::new();
    if let Ok(rd) = fs::read_dir(&tex_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("dds") {
                continue;
            }
            if let Some((w, h, dxgi, payload)) = dds_dims(&p) {
                by_dim.entry((w, h)).or_default().push((p.file_name().unwrap().to_string_lossy().to_string(), dxgi, payload));
            }
        }
    }
    println!("textures: {} files in {} dimension groups", by_dim.values().map(|v| v.len()).sum::<usize>(), by_dim.len());

    let text = fs::read_to_string(dump_path).expect("dump");
    let mut draw = 0usize;
    let mut matched = 0usize;
    let mut unmatched = 0usize;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("draw ") {
            draw = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
            if draw > max_draws {
                break;
            }
            continue;
        }
        let Some(rest) = t.strip_prefix("texfetch ") else { continue };
        // texfetch <i> <d0..d5> addr <A> <WxH> fmt <F> bytes <N> hash <H> prefix <P>
        let f: Vec<&str> = rest.split_whitespace().collect();
        if f.len() < 18 {
            continue;
        }
        let fi = f[0];
        let d1 = u32::from_str_radix(f[2], 16).unwrap_or(0);
        let base_addr = ((d1 >> 12) & 0xFFFFF) << 12;
        let addr = f[8];
        let dim = f[9];
        // The dump's dimensions come from a decode that is not dimension-aware (MODLOG 382); when a guest index is
        // available, take the values Xenia itself decoded for that base page.
        let (fmt, w, h, pitch) = match index.get(&base_addr) {
            Some(&(ifmt, iw, ih, ip)) => (ifmt, iw, ih, ip),
            None => {
                let fmt: u32 = match f[11].parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let Some((ws, hs)) = f[9].split_once('x') else { continue };
                let (w, h): (u32, u32) = match (ws.parse(), hs.parse()) {
                    (Ok(a), Ok(b)) => (a, b),
                    _ => continue,
                };
                (fmt, w + 1, h + 1, w + 1)
            }
        };
        let _ = dim;
        let _ = f[13];
        let (edge, bpb) = xenos_block(fmt);
        let bw = w.div_ceil(edge);
        let bh = h.div_ceil(edge);
        // Storage is laid out in 32x32-block tiles over the PITCH-aligned width (xds::untile's rule), not over the
        // texture width -- for these textures pitch is 4/8/16 texels-pairs against widths of 32/64/128 blocks.
        let pitch_blocks = pitch.max(w).div_ceil(edge);
        let stored = (pitch_blocks * bh * bpb) as usize;
        // Fingerprint source: the dumped guest bytes when a guest dir is given (the dump's own hash cannot see the
        // guest window, MODLOG 381), otherwise the dump's hash.
        let guest_bytes: Option<Vec<u8>> = match &guest_dir {
            Some(gd) => {
                let name = format!("{base_addr:08X}_f{fmt}_{w}x{h}.bin");
                fs::read(gd.join(&name)).ok()
            }
            None => None,
        };
        if guest_dir.is_some() && guest_bytes.is_none() {
            unmatched += 1;
            continue;
        }
        let stored = guest_bytes.as_ref().map(|g| g.len()).unwrap_or(stored).max(1);
        let log_bpb = bpb.trailing_zeros();
        let mut found: Option<String> = None;
        for (name, dxgi, payload) in by_dim.get(&(w, h)).map(|v| v.as_slice()).unwrap_or(&[]) {
            let compatible = match fmt {
                20 | 53 => *dxgi == 77 || *dxgi == 78,
                59 | 58 => *dxgi == 80 || *dxgi == 81,
                18 | 51 => *dxgi == 71 || *dxgi == 72,
                6 | 50 => *dxgi == 28,
                _ => true,
            };
            if !compatible || payload.len() < stored {
                continue;
            }
            let mut tiled = vec![0u8; stored];
            for y in 0..bh {
                for x in 0..bw {
                    let src = ((y * bw + x) * bpb) as usize;
                    let dst = (xds::tiled_offset_2d(x, y, pitch_blocks, log_bpb) * bpb) as usize;
                    if src + bpb as usize <= payload.len() && dst + bpb as usize <= tiled.len() {
                        tiled[dst..dst + bpb as usize].copy_from_slice(&payload[src..src + bpb as usize]);
                    }
                }
            }
            // Decoded comparison: the project's own `xds::untile` (the importer's reader, endian handled inside)
            // against the DDS's linear level 0 -- no hand-rolled tiling involved.
            if let Some(g) = &guest_bytes {
                let fmt_enum = Format::from_raw(fmt);
                for endian in 0..4u32 {
                    if let Ok(img) = xds::untile(g, fmt_enum, w, h, pitch, true, endian) {
                        let n = img.data.len().min(payload.len());
                        if n > 0 && img.data[..n] == payload[..n] {
                            found = Some(format!("{name} (DECODED match, fmt {fmt} -> dxgi {dxgi}, guest endian {endian})"));
                            break;
                        }
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            let tiled_hash = fnv(&tiled);
            match &guest_bytes {
                Some(g) => {
                    if g.len() < stored {
                        continue;
                    }
                    // The guest stores the texture in its own endianness; try the four modes exactly as texraw does
                    // (MODLOG 181: the same rule that made the 512x512 BC4 pair match byte-for-byte).
                    for endian in 0..4u32 {
                        let mut cand = g[..stored].to_vec();
                        match endian {
                            1 => cand.chunks_exact_mut(2).for_each(|c| c.swap(0, 1)),
                            2 => cand.chunks_exact_mut(4).for_each(|c| c.reverse()),
                            3 => cand.chunks_exact_mut(4).for_each(|c| c.swap(0, 2)),
                            _ => {}
                        }
                        if fnv(&cand) == tiled_hash {
                            found = Some(format!("{name} ({bw}x{bh} blocks, fmt {fmt} -> dxgi {dxgi}, guest endian {endian})"));
                            break;
                        }
                    }
                }
                None => {
                    if tiled_hash == u32::from_str_radix(f[15], 16).unwrap_or(0) {
                        found = Some(format!("{name} ({bw}x{bh} blocks, fmt {fmt} -> dxgi {dxgi})"));
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        match found {
            Some(name) => {
                matched += 1;
                println!("draw {draw} tf{fi} {w}x{h} fmt {fmt} addr {addr} -> **{name}**");
            }
            None => {
                unmatched += 1;
                if unmatched <= 12 {
                    println!("draw {draw} tf{fi} {w}x{h} fmt {fmt} addr {addr} -> no DDS matches (hash {})", f[15]);
                }
            }
        }
    }
    println!("\nmatched {matched}, unmatched {unmatched}");
}
