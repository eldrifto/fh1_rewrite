//! texmatch: find which Xenia guest-texture dump corresponds to a disc BC4 texture.
//!
//! usage: texmatch <disc.dds> <dump_dir> [endian]
//!
//! Scans every dump named `<base>_f<format>_<w>x<h>.bin` whose dimensions match the disc
//! texture, untiles it with the game's own tiling (`xds::untile`) and ranks the dumps by
//! Pearson correlation of the red component against the disc BC4.  A correlation near 1
//! means the game stored the texture unchanged (no load-time conversion); anything else
//! means the importer must reproduce a conversion.
//!
//! Also fits `guest = disc^g` by least squares on the log of mid-tone samples, so the
//! exponent of any applied curve is printed when the match is found.

use std::fs;

use fh1_formats::xds::{self, Format};

fn disc_values(dds: &[u8], w: u32, h: u32) -> Vec<f32> {
    let dxgi = u32::from_le_bytes(dds[128..132].try_into().unwrap());
    // BC4 (the road grains), BC1/BC3 (the colour layers) -- all three appear on the disc.
    let (fmt, block) = match dxgi {
        80 => (texpresso::Format::Bc4, 8usize),
        71 => (texpresso::Format::Bc1, 8),
        77 => (texpresso::Format::Bc3, 16),
        other => panic!("unsupported disc DXGI {other}"),
    };
    let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
    let payload = dds.get(148..148 + blocks * block).expect("compressed payload");
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    fmt.decompress(payload, w as usize, h as usize, &mut rgba);
    (0..(w * h) as usize).map(|i| rgba[i * 4] as f32 / 255.0).collect()
}

fn corr(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let ma = a.iter().sum::<f32>() / n;
    let mb = b.iter().sum::<f32>() / n;
    let cov = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum::<f32>();
    let sa = a.iter().map(|x| (x - ma).powi(2)).sum::<f32>().sqrt();
    let sb = b.iter().map(|y| (y - mb).powi(2)).sum::<f32>().sqrt();
    if sa < 1e-9 || sb < 1e-9 { 0.0 } else { cov / (sa * sb) }
}

fn gamma_fit(d: &[f32], g: &[f32]) -> Option<f32> {
    // least squares on ln(guest) = g * ln(disc), restricted to the mid tones.
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    let mut sxx = 0.0f64;
    let mut sxy = 0.0f64;
    let mut n = 0.0f64;
    for (d, g) in d.iter().zip(g) {
        if *d > 0.02 && *d < 0.98 && *g > 0.02 && *g < 0.98 {
            let x = (*d as f64).ln();
            let y = (*g as f64).ln();
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
            n += 1.0;
        }
    }
    if n < 100.0 {
        return None;
    }
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    Some(((n * sxy - sx * sy) / denom) as f32)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: texmatch <disc.dds> <dump_dir> [endian]");
        std::process::exit(1);
    }
    let dds = fs::read(&args[0]).expect("disc dds");
    let dir = &args[1];
    let endian: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let w = u32::from_le_bytes(dds[16..20].try_into().unwrap());
    let h = u32::from_le_bytes(dds[12..16].try_into().unwrap());
    println!("disc {}: {}x{}", args[0], w, h);

    // The dump sidecar gives the exact untiling parameters (pitch, tiled, endian).
    // Format: `<name> base <hex> fmt <n> w <n> h <n> pitch <n> tiled <0|1> endian <n> ...`
    let mut meta: std::collections::HashMap<String, (u32, bool, u32)> =
        std::collections::HashMap::new();
    if let Ok(text) = fs::read_to_string(format!("{dir}\\dump_index.txt")) {
        for line in text.lines() {
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() < 12 {
                continue;
            }
            let mut pitch = 0u32;
            let mut tiled = true;
            let mut end = 0u32;
            let mut i = 1;
            while i + 1 < t.len() {
                match t[i] {
                    "pitch" => pitch = t[i + 1].parse().unwrap_or(0),
                    "tiled" => tiled = t[i + 1] != "0",
                    "endian" => end = t[i + 1].parse().unwrap_or(0),
                    _ => {}
                }
                i += 2;
            }
            meta.insert(t[0].to_string(), (pitch, tiled, end));
        }
    }
    println!("index entries: {}", meta.len());
    let disc = disc_values(&dds, w, h);
    // Non-zero disc texels are the informative ones.
    let keep: Vec<usize> = (0..disc.len()).filter(|&i| disc[i] > 0.02).collect();
    let disc_k: Vec<f32> = keep.iter().map(|&i| disc[i]).collect();

    let suffix = format!("_{}x{}.bin", w, h);
    let mut best: Vec<(f32, String, f32, f32)> = Vec::new();
    let mut scanned = 0usize;
    for entry in fs::read_dir(dir).expect("dump dir").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(&suffix) {
            continue;
        }
        // `28A3F000_f59_512x512.bin`
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() < 3 {
            continue;
        }
        let gfmt: u32 = match parts[1].trim_start_matches('f').parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let bytes = match fs::read(entry.path()) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let (pitch, tiled, end) = meta.get(name.trim_end_matches(".bin")).copied().unwrap_or((w, true, endian));
        let pitch = if pitch == 0 { w } else { pitch };
        let img = match xds::untile(&bytes, Format::from_raw(gfmt), w, h, pitch, tiled, end) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let rgba = match xds::to_rgba8(&img) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let guest: Vec<f32> = (0..(w * h) as usize).map(|i| rgba[i * 4] as f32 / 255.0).collect();
        let guest_k: Vec<f32> = keep.iter().map(|&i| guest[i]).collect();
        let c = corr(&disc_k, &guest_k);
        let mean = guest.iter().sum::<f32>() / guest.len() as f32;
        let g = gamma_fit(&disc_k, &guest_k).unwrap_or(f32::NAN);
        best.push((c, format!("{name} fmt{gfmt}"), mean, g));
        scanned += 1;
    }
    best.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("scanned {scanned} dumps of size {w}x{h}");
    println!("{:>3} {:>22} {:>8} {:>8} {:>10}", "rank", "dump", "corr", "mean", "gamma_fit");
    for (i, (c, name, mean, g)) in best.iter().take(10).enumerate() {
        println!("{:>3} {:>22} {:>8.4} {:>8.4} {:>10.3}", i + 1, name, c, mean, g);
    }

    // For the best candidate, print the actual transfer curve disc -> guest, so the load-time
    // conversion (if any) is visible as a table rather than inferred from one number.
    if let Some((c, name, _, g)) = best.first() {
        let fname = name.split(' ').next().unwrap_or("");
        let bytes = match fs::read(format!("{dir}\\{fname}")) {
            Ok(b) => b,
            Err(_) => return,
        };
        let gfmt: u32 = fname.split('_').nth(1).unwrap_or("f0").trim_start_matches('f').parse().unwrap_or(0);
        let (pm, tm, em) = meta.get(fname.trim_end_matches(".bin")).copied().unwrap_or((w, true, endian));
        let pm = if pm == 0 { w } else { pm };
        println!("untile params: pitch {pm} tiled {tm} endian {em}");
        let img = match xds::untile(&bytes, Format::from_raw(gfmt), w, h, pm, tm, em) {
            Ok(i) => i,
            Err(_) => return,
        };
        let rgba = match xds::to_rgba8(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        let guest: Vec<f32> = (0..(w * h) as usize).map(|i| rgba[i * 4] as f32 / 255.0).collect();
        let guest_k: Vec<f32> = keep.iter().map(|&i| guest[i]).collect();
        println!("\nbest candidate {fname} (corr {c:.4}, gamma fit {g:.3})");
        let powered: Vec<f32> = disc_k.iter().map(|d| d.powf(*g)).collect();
        println!("corr(disc^g, guest) = {:.4}", corr(&powered, &guest_k));
        let lin: Vec<f32> = disc_k.iter().map(|d| 0.5 + 0.5 * d).collect();
        println!("corr(0.5+0.5*disc, guest) = {:.4}", corr(&lin, &guest_k));
        println!("{:>10} {:>6} {:>8} {:>8} {:>8}", "disc_bin", "n", "g_mean", "g_min", "g_max");
        let mut bins = [(0.0f64, 0.0f64, 1.0f32, 0.0f32, 0usize); 10];
        for (i, &k) in keep.iter().enumerate() {
            let d = disc[k];
            let b = ((d * 10.0) as usize).min(9);
            let e = &mut bins[b];
            e.0 += d as f64;
            e.1 += guest[k] as f64;
            e.2 = e.2.min(guest[k]);
            e.3 = e.3.max(guest[k]);
            e.4 += 1;
        }
        for (i, b) in bins.iter().enumerate() {
            if b.4 == 0 {
                continue;
            }
            println!(
                "{:>4.1}-{:>4.1} {:>6} {:>8.4} {:>8.4} {:>8.4}",
                i as f32 / 10.0,
                (i + 1) as f32 / 10.0,
                b.4,
                b.1 / b.4 as f64,
                b.2,
                b.3
            );
        }

        // Write both images as PPM so the match can be checked as a picture, not only as numbers.
        let write_ppm = |path: &str, rgba: &[u8]| {
            let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
            for i in 0..(w * h) as usize {
                out.extend_from_slice(&rgba[i * 4..i * 4 + 3]);
            }
            let _ = fs::write(path, out);
        };
        let mut disc_rgba = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            let v = (disc[i] * 255.0).round() as u8;
            disc_rgba[i * 4] = v;
            disc_rgba[i * 4 + 1] = v;
            disc_rgba[i * 4 + 2] = v;
            disc_rgba[i * 4 + 3] = 255;
        }
        write_ppm("data/work/match_disc.ppm", &disc_rgba);
        write_ppm("data/work/match_guest.ppm", &rgba);
        println!("wrote data/work/match_disc.ppm and data/work/match_guest.ppm");

        // Spatial registration: if the content is the same image, some shift will give a much
        // higher correlation than 0 (a wrong pitch or a block offset looks exactly like a shift).
        let mut best_shift = (0i32, 0i32, -1.0f32);
        for dy in (-16..=16).step_by(4) {
            for dx in (-16..=16).step_by(4) {
                let mut a = Vec::new();
                let mut b = Vec::new();
                for y in 0..h as i32 {
                    for x in 0..w as i32 {
                        let (sx, sy) = (x + dx, y + dy);
                        if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                            continue;
                        }
                        let i = (y as u32 * w + x as u32) as usize;
                        let j = (sy as u32 * w + sx as u32) as usize;
                        a.push(disc[i]);
                        b.push(guest[j]);
                    }
                }
                let c = corr(&a, &b);
                if c > best_shift.2 {
                    best_shift = (dx, dy, c);
                }
            }
        }
        println!("best shift dx {} dy {} -> corr {:.4}", best_shift.0, best_shift.1, best_shift.2);
    }
}
