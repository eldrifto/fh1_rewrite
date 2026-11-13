//! sky_bake: apply the game's sky formula to an imported track's dome textures, at a chosen `w`.
//!
//! The formula is read from the game's own pixel shader (MODLOG 337):
//!
//!     out.rgb = tex.rgb * (1 + alpha * m * (1 - w))
//!
//! `alpha` is the dome texture's alpha channel, `m` is `c188.x` (sky.json, per draw) and `w` is `c104.w`,
//! which is runtime-set and absent from the model.  `w = 1` collapses the formula to `out = tex`, i.e. the
//! shipped path, so sweeping `w` sweeps between shipped and full bake.
//!
//! **The gain is applied in LINEAR light, not on the display value.**  The dome textures are sRGB: the console
//! fetches them and the hardware converts to linear, the shader does its arithmetic there, and the sRGB
//! framebuffer converts back.  Multiplying the display value instead (MODLOG 338's first bake) applies a gain
//! that is too strong by the transfer curve -- `f` on the display value is `f^2.2` in light.
//!
//! Only level 0 is re-encoded; the stored mip levels are copied byte for byte from the source, so a sweep does
//! not change the mip chain between runs.
//!
//! usage:
//!   sky_bake <track_dir> --save <dir>     copy the dome textures to <dir> as the pristine source
//!   sky_bake <track_dir> <w> --from <dir> restore from <dir>, bake at <w>, write the live textures
//!   sky_bake <track_dir> <w>              bake the live textures in place (NOT repeatable: feed it a --save copy)

use std::fs;
use std::path::{Path, PathBuf};

use texpresso::{Algorithm, Format, Params};

const HEADER: usize = 128;

fn le32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap())
}

/// The BC format of a DDS written by this project (DXGI format word at 128), or None if it is not one we re-encode.
fn format_of(d: &[u8]) -> Option<Format> {
    match le32(d, HEADER) {
        71 => Some(Format::Bc1),
        77 => Some(Format::Bc3),
        _ => None,
    }
}

/// DDS byte offset of level 0's blocks: the 128-byte legacy header plus, for a DX10 header, its 20-byte extension.
fn data_offset(d: &[u8]) -> usize {
    if d.get(84..88) == Some(b"DX10") { HEADER + 20 } else { HEADER }
}

/// The draws' (texture id, m) pairs out of `<track>/scenery/sky.json`.
fn draws(track: &Path) -> Vec<(u32, f32)> {
    let Ok(b) = fs::read(track.join("scenery/sky.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) else { return Vec::new() };
    let mut out = Vec::new();
    for d in v["draws"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let m = d["constants"]["1"][0].as_f64().unwrap_or(0.0) as f32;
        let Some(first) = d["textures"].as_array().and_then(|t| t.first()).and_then(|t| t.as_u64()) else { continue };
        out.push((first as u32, m));
    }
    out
}

fn tex_path(track: &Path, id: u32) -> PathBuf {
    track.join("scenery/sky/textures").join(format!("{id:08x}.dds"))
}

/// An 8-bit sRGB value as linear light (the IEC 61966-2-1 curve, the one the GPU's sRGB fetch applies).
fn to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear light back to an 8-bit sRGB value, rounded.
fn to_srgb(lin: f32) -> u8 {
    let c = lin.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: sky_bake <track_dir> --save <dir> | <track_dir> <w> [--from <dir>]";
    if args.len() < 2 {
        eprintln!("{usage}");
        std::process::exit(2);
    }
    let track = PathBuf::from(&args[0]);
    let mut w: Option<f32> = None;
    let mut save: Option<PathBuf> = None;
    let mut from: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--save" => {
                save = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--from" => {
                from = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            v => {
                w = v.parse().ok();
                i += 1;
            }
        }
    }
    // The whole sky of one track is the same handful of files; a texture drawn twice (the cap and a sector
    // share nothing, but be safe) is baked once at that draw's m.
    let mut seen: Vec<u32> = Vec::new();
    let mut n = 0;
    for (id, m) in draws(&track) {
        if seen.contains(&id) {
            continue;
        }
        let live = tex_path(&track, id);
        let Some(mut bytes) = from.as_ref().map(|d| fs::read(d.join(live.file_name().unwrap())).ok()).unwrap_or_else(|| fs::read(&live).ok()) else {
            eprintln!("skip {id:08x}: not readable");
            continue;
        };
        if let Some(dir) = &save {
            let _ = fs::create_dir_all(dir);
            let _ = fs::write(dir.join(live.file_name().unwrap()), &bytes);
            seen.push(id);
            n += 1;
            continue;
        }
        let Some(w) = w else {
            eprintln!("{usage}");
            std::process::exit(2);
        };
        let Some(fmt) = format_of(&bytes) else { continue };
        let (h, wd) = (le32(&bytes, 12) as usize, le32(&bytes, 16) as usize);
        let off = data_offset(&bytes);
        let len = fmt.compressed_size(wd, h);
        let Some(src) = bytes.get(off..off + len).map(|s| s.to_vec()) else { continue };
        let mut rgba = vec![0u8; wd * h * 4];
        fmt.decompress(&src, wd, h, &mut rgba);
        let k = 1.0 - w;
        let mut lo = 1.0f32;
        let mut hi = 1.0f32;
        for px in rgba.chunks_exact_mut(4) {
            let f = 1.0 + (px[3] as f32 / 255.0) * m * k;
            lo = lo.min(f);
            hi = hi.max(f);
            for c in 0..3 {
                let lin = to_linear(px[c]);
                px[c] = to_srgb(lin * f);
            }
        }
        let params = Params { algorithm: Algorithm::RangeFit, ..Default::default() };
        let mut out = vec![0u8; len];
        fmt.compress(&rgba, wd, h, params, &mut out);
        bytes[off..off + len].copy_from_slice(&out);
        if let Ok(_) = fs::write(&live, &bytes) {
            println!("  {id:08x}  m {m:.2}  gain {lo:.2}-{hi:.2}  ({wd}x{h})");
            seen.push(id);
            n += 1;
        }
    }
    if save.is_some() {
        println!("saved {n} dome textures");
    } else {
        println!("baked {n} dome textures at w = {}", w.unwrap_or(f32::NAN));
    }
}
