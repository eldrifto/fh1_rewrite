//! texcmp: compare a disc DDS (BC4) with a Xenia guest texture dump (untiled with the game's own tiling).
//!
//! usage: texcmp <disc.dds> <dump.bin> <width> <height> <guest_format_raw> [endian]
//! Prints the value distribution of both and the guest/disc ratio, to see whether the game
//! stored the texture raw (ratio ~1) or applied a load-time conversion.

use std::fs;

use fh1_formats::xds::{self, Format};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!("usage: texcmp <disc.dds> <dump.bin> <w> <h> <guest_format_raw> [endian]");
        std::process::exit(1);
    }
    let dds = fs::read(&args[0]).expect("disc dds");
    let dump = fs::read(&args[1]).expect("dump");
    let w: u32 = args[2].parse().expect("width");
    let h: u32 = args[3].parse().expect("height");
    let gfmt: u32 = args[4].parse().expect("guest format");
    let endian: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);

    // Disc: DX10 header, BC4 payload at 148.
    let dxgi = u32::from_le_bytes(dds[128..132].try_into().unwrap());
    assert_eq!(dxgi, 80, "expected BC4 (80) in the disc DDS, got {dxgi}");
    let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
    let payload = dds.get(148..148 + blocks * 8).expect("BC4 payload");
    let mut disc = vec![0u8; (w * h * 4) as usize];
    texpresso::Format::Bc4.decompress(payload, w as usize, h as usize, &mut disc);
    let disc_v: Vec<f32> = (0..(w * h) as usize).map(|i| disc[i * 4] as f32 / 255.0).collect();

    // Guest: game tiling -> RGBA.
    let img = xds::untile(&dump, Format::from_raw(gfmt), w, h, w, true, endian).expect("untile guest");
    let guest = xds::to_rgba8(&img).expect("guest rgba");
    let guest_v: Vec<f32> = (0..(w * h) as usize).map(|i| guest[i * 4] as f32 / 255.0).collect();

    let stat = |v: &[f32], name: &str| {
        let mut s = v.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f32| s[(p * (s.len() - 1) as f32) as usize];
        let mean = s.iter().sum::<f32>() / s.len() as f32;
        println!(
            "{name:>6}: mean {mean:.4}  p05 {:.4} p25 {:.4} p50 {:.4} p75 {:.4} p95 {:.4}",
            q(0.05), q(0.25), q(0.50), q(0.75), q(0.95)
        );
    };
    stat(&disc_v, "disc");
    stat(&guest_v, "guest");

    let pairs: Vec<(f32, f32)> = disc_v.iter().copied().zip(guest_v.iter().copied()).filter(|(d, _)| *d > 0.02).collect();
    if !pairs.is_empty() {
        let ratio: Vec<f32> = pairs.iter().map(|(d, g)| g / d).collect();
        let mut r = ratio.clone();
        r.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("guest/disc ratio (where disc > 0.02): median {:.3}  p10 {:.3}  p90 {:.3}  n {}",
                 r[r.len() / 2], r[r.len() / 10], r[r.len() * 9 / 10], r.len());
        // Pearson correlation between the disc and guest values (identity match if ~1).
        let n = pairs.len() as f32;
        let (md_, mg_) = (
            pairs.iter().map(|p| p.0).sum::<f32>() / n,
            pairs.iter().map(|p| p.1).sum::<f32>() / n,
        );
        let cov = pairs
            .iter()
            .map(|(d, g)| (d - md_) * (g - mg_))
            .sum::<f32>()
            / n;
        let sd = (pairs.iter().map(|(d, _)| (d - md_).powi(2)).sum::<f32>() / n).sqrt();
        let sg = (pairs.iter().map(|(_, g)| (g - mg_).powi(2)).sum::<f32>() / n).sqrt();
        println!("correlation: {:.4}", cov / (sd * sg).max(1e-9));
        // gamma curve fit check: ratio of means
        let md = pairs.iter().map(|p| p.0).sum::<f32>() / pairs.len() as f32;
        let mg = pairs.iter().map(|p| p.1).sum::<f32>() / pairs.len() as f32;
        println!("means: disc {md:.4} guest {mg:.4}  guest^2.2/disc {:.3}", (mg.powf(2.2)) / md);
    }
}
