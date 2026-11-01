//! ddsppm: write a DDS (BC4 or RGBA8) as a P6 PPM so its values can be combined and analysed.
//!
//! usage: ddsppm <in.dds> <out.ppm>

use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: ddsppm <in.dds> <out.ppm>");
        std::process::exit(1);
    }
    let dds = fs::read(&args[0]).expect("dds");
    let h = u32::from_le_bytes(dds[12..16].try_into().unwrap());
    let w = u32::from_le_bytes(dds[16..20].try_into().unwrap());
    let dxgi = u32::from_le_bytes(dds[128..132].try_into().unwrap());
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    match dxgi {
        80 => {
            let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
            let payload = dds.get(148..148 + blocks * 8).expect("BC4 payload");
            texpresso::Format::Bc4.decompress(payload, w as usize, h as usize, &mut rgba);
        }
        28 => {
            let start = 148;
            rgba.copy_from_slice(&dds[start..start + (w * h * 4) as usize]);
        }
        77 => {
            let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
            let payload = dds.get(148..148 + blocks * 16).expect("BC3 payload");
            texpresso::Format::Bc3.decompress(payload, w as usize, h as usize, &mut rgba);
        }
        71 => {
            let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
            let payload = dds.get(148..148 + blocks * 8).expect("BC1 payload");
            texpresso::Format::Bc1.decompress(payload, w as usize, h as usize, &mut rgba);
        }
        other => {
            eprintln!("unsupported DXGI {other}");
            std::process::exit(1);
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for i in 0..(w * h) as usize {
        out.extend_from_slice(&rgba[i * 4..i * 4 + 3]);
    }
    fs::write(&args[1], out).expect("write ppm");
    println!("{} -> {} ({}x{}, dxgi {})", args[0], args[1], w, h, dxgi);
}
