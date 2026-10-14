//! Forza Motorsport 2 circuit scenery (`import-fm2`): the FM2 `.rmb.bin` models (fh1_formats::fm2) written as the same
//! `scenery/` tiles + `materials.json` + `textures/` the other games produce (module docs of `scenery.rs`).
//!
//! APPROXIMATION (what is not decoded yet, docs in `fh1_formats::fm2`): every draw is drawn with ONE diffuse texture on a plain
//! diffuse shader (the install's `h_diff_1`): the first colour texture its sampler list binds (single-channel BC4 textures are
//! lightmaps / blend masks and are skipped; PVS runtime textures come from the `.stx.bin`), so terrain blends show their first
//! layer; a draw with only a lightmap (roads: their colour is a shader constant) gets a flat grey. A draw's winding is not
//! decoded: it is flipped when most of its up / down facing triangles face down (`FM2_NO_FLIP=1` keeps the file's order).
//! Debug: `FM2_DEBUG=1` (texture flag histogram, large draws), `FM2_AT=x,z` (the draws under a point, model space). The vertex
//! texture coordinates are 16-bit normalised values times `FM2_UV_SCALE` (default 32, UNVERIFIED tiling factor).
//!
//! LIGHTMAPS: a draw that binds a lightmap atlas tile (PVS flags 8 / 9 / 12) gets the tile cropped out of its `.stx.bin`
//! page (`convert_lightmaps`, a 32x32 greyscale Dxt5A crop) bound on the material's Light_Map sampler (slot 7) and the
//! draw's lightmap uv (`Draw::uv_unorm`, 0..1 over the tile) on uv1. The uv / tile mapping is VERIFIED from the data.
//! `FM2_LIGHTMAPS=0` disables. This is the primary path: the remaster (`remaster.rs`, the default renderer) reads the tile
//! as the lighting (`FLAG_LM_LIGHT`: `albedo * (lightmap * gain + bias)`, drawn unlit — FM2's lightmap already contains the
//! baked sun), so run `fh1setup <FH1 disc> --only remaster` after importing. Without that table the faithful path draws
//! the same material with FH1's `h_diff_1_lm`, which ADDS the lightmap to its own sun/ambient and looks over-bright (c6.z
//! = `FM2_LM_FLOOR`). Blend draws (terrain) use `h_blnd2_ao_lm` with the lightmap uv on UV2 (the remaster reads it from set 2); the faithful shader samples the mask and the lightmap from the
//! uv, which FM2's two separate pairs do not fit, so they have no lightmap yet.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use fh1_formats::fm2;
use fh1_formats::zip::Archive;

use crate::scenery::{convert_textures, smooth_normals, write_tile, Batch, TrackSrc, ATTR_COLOR, ATTR_UV0, ATTR_UV1, ATTR_UV2, TILE};

/// File id of the generated flat grey texture.
const ASPHALT: u32 = 0xFFFF_0001;
/// File id of the generated white texture (the blend shader's ambient-occlusion slot).
const WHITE: u32 = 0xFFFF_0002;

/// A draw waiting for its texture decision: the candidate (file id, flags) per sampler, in order.
struct Pending {
    /// A separate mask uv (`Draw::separate_mask`), unscaled; None = the mask shares `raw`.
    mask_raw: Option<Vec<[f32; 2]>>,
    /// Pixel-shader constants of the draw by register (`Draw::constants`).
    consts: std::collections::BTreeMap<u32, [f32; 4]>,
        model: usize,
    tile: (i32, i32),
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    /// The unscaled texture coordinates: the blend mask's (the draw's unwrapped layout, VERIFIED on the mask atlas).
    raw: Vec<[f32; 2]>,
    /// The lightmap atlas tile this draw binds (file id, page `.stx.bin` entry, `[su, sv, ou, ov]` in the page).
    lightmap: Option<(u32, u32, [f32; 4])>,
    /// The lightmap texture coordinates (unorm 0..1 over the tile).
    lm_uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
    candidates: Vec<(u32, u32, usize)>,
}

pub fn build_track(src: &TrackSrc, out: &Path, ar: &mut Archive<std::fs::File>, pvs_bytes: &[u8]) -> Result<()> {
    let pvs = fm2::parse_pvs(pvs_bytes).context("FM2 pvs")?;
    // 32 reproduces the banners' lettering and the kerbs' stripes on Silverstone (the per-draw guess `tiling_scale` made them
    // worse; FM2_UV_SCALE=auto selects it).
    let fixed_scale: Option<f32> = match std::env::var("FM2_UV_SCALE").as_deref() {
        Ok("auto") => None,
        Ok(v) => Some(v.parse().unwrap_or(32.0)),
        Err(_) => Some(32.0),
    };
    let mut by_name = HashMap::new();
    for e in &ar.entries {
        by_name.entry(e.name.to_ascii_lowercase().replace(char::from(92), "/")).or_insert_with(|| e.clone());
    }
    let (mut models, mut failed, mut draws) = (0usize, 0usize, 0usize);
    let mut pending: Vec<Pending> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut name_hist: BTreeMap<String, usize> = BTreeMap::new();
    for e in ar.entries.clone() {
        let name = e.name.to_ascii_lowercase();
        let Some(number) = name.strip_suffix(".rmb.bin").and_then(|s| s.rsplit('.').next()).and_then(|s| s.parse::<usize>().ok()) else { continue };
        let raw_model = ar.read(&e)?;
        if std::env::var_os("FM2_NAMES").is_some() {
            let mut i = 0;
            while i + 7 < raw_model.len() {
                if &raw_model[i..i + 7] == b"shader_" {
                    let end = raw_model[i..].iter().position(|&c| c == 0).unwrap_or(0);
                    let name = String::from_utf8_lossy(&raw_model[i..i + end]).to_string();
                    let tail = name.rsplit("__").next().unwrap_or(&name).trim_end_matches('_').to_owned();
                    let tail: String = tail.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_').to_owned();
                    *name_hist.entry(tail).or_default() += 1;
                    i += end.max(1);
                } else {
                    i += 1;
                }
            }
        }
        let model = match fm2::parse_model(&raw_model) {
            Ok(m) => m,
            Err(err) => {
                failed += 1;
                println!("[fm2] {}: {err}", e.name);
                continue;
            }
        };
        models += 1;
        for why in &model.skipped {
            *skipped.entry(why.split(|c: char| c.is_ascii_digit()).next().unwrap_or("").to_owned()).or_default() += 1;
        }
        let list = pvs.models.get(number).cloned().unwrap_or_default();
        for d in &model.draws {
            if std::env::var("FM2_PAIRS_MODEL").ok().and_then(|v| v.parse::<usize>().ok()) == Some(number) {
                let mut line = format!("[fm2] pairs model {number} stride {} tris {}:", d.stride, d.indices.len() / 3);
                let mut o = 12;
                while o + 4 <= d.stride {
                    let sgn = d.uv_at(o);
                    let un = d.uv_unorm(o);
                    let span = |v: &Vec<[f32; 2]>, k: usize| v.iter().map(|p| p[k]).fold(f32::MIN, f32::max) - v.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
                    line += &format!(" @{o}: signed {:.3}x{:.3} unorm {:.3}x{:.3};", span(&sgn, 0), span(&sgn, 1), span(&un, 0), span(&un, 1));
                    o += 4;
                }
                println!("{line}");
            }
            let (uv0, _) = d.uv_offsets();
            let raw = d.uv_at(uv0);
            let scale = fixed_scale.unwrap_or_else(|| tiling_scale(&d.positions, &raw, &d.indices));
            let uv: Vec<[f32; 2]> = raw.iter().map(|p| [p[0] * scale, p[1] * scale]).collect();
            let raw_uv = raw;
            if std::env::var_os("FM2_DEBUG").is_some() && (d.items.is_empty() || list.is_empty()) && draws < 40 {
                println!("[fm2] model {number}: items {:?} list {:?} stride {}", d.items, list, d.stride);
            }
            let candidates: Vec<(u32, u32, usize)> = d
                .items
                .iter()
                .filter_map(|&it| list.get(it as usize).and_then(|&t| pvs.textures.get(t as usize).map(|x| (x.file_id, x.flags, t as usize))))
                .collect();
            let has_tile = candidates.iter().any(|t| matches!(t.1, 8 | 9 | 12));
            let sep = if has_tile { d.separate_mask(uv0) } else { None };
            let lightmap_at = if let Some((lm, _)) = sep { Some(lm) } else { has_tile.then(|| d.lightmap_offset(uv0)).flatten() };
            let mask_raw = sep.map(|(_, mk)| d.uv_at(mk));
            let lm_uv = lightmap_at.map(|o| d.uv_unorm(o)).unwrap_or_default();
            if std::env::var_os("FM2_UNRES").is_some() {
                let beyond: Vec<u32> = d.items.iter().copied().filter(|&it| it as usize >= list.len()).collect();
                if !beyond.is_empty() && d.positions.len() <= 8 {
                    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for v in &d.positions {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v[k]);
                            hi[k] = hi[k].max(v[k]);
                        }
                    }
                    println!("[fm2] unresolved model {number} list {} items {:?} at x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0}", list.len(), d.items, lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
                }
            }
            let c = d.positions.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
            let n = d.positions.len().max(1) as f32;
            // The draw's lightmap atlas tile (PVS flags 8 / 9 / 12): its page's `.stx.bin` entry and its rectangle.
            let lightmap = candidates.iter().find(|t| matches!(t.1, 8 | 9 | 12)).map(|t| {
                let p = &pvs.textures[t.2];
                (t.0, p.object, p.tile)
            });
            pending.push(Pending {
                consts: d.constants.clone(),
                model: number,
                tile: (((c[0] / n) / TILE).floor() as i32, ((c[2] / n) / TILE).floor() as i32),
                positions: d.positions.clone(),
                uv,
                raw: raw_uv,
                mask_raw,
                lightmap,
                lm_uv,
                indices: d.indices.clone(),
                candidates,
            });
            draws += 1;
        }
    }
    println!("[fm2] {models} models ({failed} failed), {draws} draws, skipped draws {skipped:?}");
    if !name_hist.is_empty() {
        let mut v: Vec<_> = name_hist.iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(*x.1));
        for (k, n) in v.iter().take(120) {
            println!("[fm2] material {k}: {n}");
        }
    }
    if std::env::var_os("FM2_DEBUG").is_some() {
        println!("[fm2] pvs: {} textures, {} model lists", pvs.textures.len(), pvs.models.len());
    }

    // Textures: every candidate with a file on the disc (.bix pair = flags & 4, else a .bin).
    let dir = out.join(&src.name);
    std::fs::create_dir_all(dir.join("textures"))?;
    std::fs::create_dir_all(dir.join("tiles"))?;
    let mut wanted = BTreeMap::new();
    for t in pending.iter().flat_map(|p| &p.candidates) {
        if t.1 & 1 == 0 && t.1 & 8 == 0 {
            wanted.insert(t.0, t.1 & 4 != 0);
        }
    }
    let mut textures = convert_textures(ar, &by_name, &wanted, &dir.join("textures"))?;
    // The track's `.stx.bin`: runtime textures (flags 1) and the lightmap atlas pages the flags 8 / 9 / 12 tiles are cut from.
    let stx = crate::scenery::read_stx(ar, &by_name);
    if let Some((d, ents)) = &stx {
        for t in pending.iter().flat_map(|p| &p.candidates).filter(|t| t.1 == 1) {
            if textures.contains_key(&t.0) {
                continue;
            }
            let Some(&ct) = ents.get(t.2) else { continue };
            match crate::textures::caff_texture_to_dds(d, ct) {
                Ok(c) => {
                    std::fs::write(dir.join("textures").join(format!("{:08x}.dds", t.0)), &c.dds)?;
                    textures.insert(t.0, (c.alpha, c.word));
                }
                Err(e) => println!("[fm2] stx texture {} (_0x{:08X}): {e}", t.2, t.0),
            }
        }
    }
    // Lightmaps (`FM2_LIGHTMAPS=0` disables): crop every draw's atlas tile out of its `.stx.bin` page into a texture of
    // its own. On by default: the remaster material tables need the tile bound on the material.
    let lightmaps_on = std::env::var("FM2_LIGHTMAPS").map_or(true, |v| v != "0");
    if lightmaps_on {
        let mut tiles: BTreeMap<u32, (u32, [f32; 4])> = BTreeMap::new();
        for p in &pending {
            if let Some((id, page, tile)) = p.lightmap {
                tiles.insert(id, (page, tile));
            }
        }
        let lm = convert_lightmaps(&stx, &tiles, &dir.join("textures"));
        println!("[fm2] lightmap tile brightness: {:?}", DEBUG_TILES.lock().unwrap());
        println!("[fm2] lightmaps: {} of {} tiles cropped from {} pages", lm.len(), tiles.len(), stx.as_ref().map_or(0, |(_, e)| e.len()));
        textures.extend(lm);
    }
    // Foliage sheets are DXT1 with black as the colour key (the game's pixel shader discards the black texels): without
    // alpha they draw as black boxes. Rewrite those textures with the black texels transparent.
    let mut keyed = 0usize;
    let mut keyed_ids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    if std::env::var_os("FM2_NO_KEY").is_none() {
        let used: std::collections::BTreeSet<u32> = pending.iter().flat_map(|p| p.candidates.iter().filter(|t| t.1 & 8 == 0 && t.1 & 1 == 0).map(|t| t.0)).collect();
        for id in used {
            if textures.get(&id).is_some_and(|t| t.0) {
                continue;
            }
            let path = dir.join("textures").join(format!("{id:08x}.dds"));
            if let Some((w, h, rgba)) = std::fs::read(&path).ok().and_then(|b| black_keyed(&b)) {
                if let Ok(c) = crate::textures::rgba_to_dds(rgba, w, h) {
                    if std::fs::write(&path, &c.dds).is_ok() {
                        let word = textures[&id].1;
                        textures.insert(id, (true, word));
                        keyed_ids.insert(id);
                        keyed += 1;
                    }
                }
            }
        }
        // Other alpha textures: partly transparent sheets are cut-outs; unused alpha channels (all 0) stay opaque.
        for (&id, &(alpha, _)) in textures.iter() {
            if alpha && !keyed_ids.contains(&id) {
                if std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok().is_some_and(|b| cutout_texture(&b)) {
                    keyed_ids.insert(id);
                }
            }
        }
        println!("[fm2] {keyed} black-keyed foliage textures given alpha, {} cut-out textures in all", keyed_ids.len());
        if std::env::var_os("FM2_DEBUG").is_some() {
            for (&id, &(alpha, _)) in textures.iter() {
                if alpha && !keyed_ids.contains(&id) {
                    let dds = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).unwrap_or_default();
                    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
                    let fmt = match u(128) { 74 => Some(texpresso::Format::Bc2), 77 => Some(texpresso::Format::Bc3), _ => None };
                    if let Some(fmt) = fmt {
                        let (h, w) = (u(12) as usize, u(16) as usize);
                        if let Some(data) = dds.get(148..148 + fmt.compressed_size(w, h)) {
                            let mut rgba = vec![0u8; w * h * 4];
                            fmt.decompress(data, w, h, &mut rgba);
                            let low: Vec<&[u8]> = rgba.chunks_exact(4).filter(|p| p[3] < 128).collect();
                            let dark = low.iter().filter(|p| p[0].max(p[1]).max(p[2]) <= 24).count();
                            let mean_rgb = low.iter().map(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) as f32 / 3.0).sum::<f32>() / low.len().max(1) as f32;
                            println!("[fm2] alpha texture {id:#x} {w}x{h} fmt {} low {:.1}% dark-of-low {:.0}% mean rgb of low {mean_rgb:.0}", u(128), low.len() as f32 * 100.0 / (w * h) as f32, dark as f32 * 100.0 / low.len().max(1) as f32);
                        }
                    }
                }
            }
        }
        if std::env::var_os("FM2_DEBUG").is_some() {
            for (&id, _) in textures.iter() {
                let Ok(b) = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))) else { continue };
                let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
                let fmt = match u(128) { 71 => texpresso::Format::Bc1, 74 => texpresso::Format::Bc2, 77 => texpresso::Format::Bc3, _ => continue };
                let (h, w) = (u(12) as usize, u(16) as usize);
                let size = fmt.compressed_size(w, h);
                let Some(data) = b.get(148..148 + size) else { continue };
                let mut rgba = vec![0u8; w * h * 4];
                fmt.decompress(data, w, h, &mut rgba);
                let mean = rgba.chunks_exact(4).map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).sum::<u32>() as f32 / (3.0 * (w * h) as f32);
                if mean < 12.0 {
                    println!("[fm2] dark texture {id:#x} {w}x{h} fmt {} mean {mean:.1}", u(128));
                }
            }
        }
    }
    // BC4 (DXGI 80 / 81) textures are single-channel masks.
    let mask = |id: u32| {
        std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok().is_some_and(|b| matches!(b.get(128..132).map(|w| u32::from_le_bytes(w.try_into().unwrap())), Some(80 | 81)))
    };

    if std::env::var_os("FM2_DEBUG").is_some() {
        let mut hist: BTreeMap<String, usize> = BTreeMap::new();
        for p in &pending {
            let k = p.candidates.iter().map(|t| format!("{}{}", t.1, if textures.contains_key(&t.0) { "+" } else { "-" })).collect::<Vec<_>>().join(",");
            *hist.entry(k).or_default() += 1;
        }
        let mut v: Vec<_> = hist.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (k, n) in v.iter().take(25) {
            println!("[fm2] draws {n}: {k}");
        }
    }
    let mut textures = textures;
    if let Ok(c) = crate::textures::rgba_to_dds([118u8, 118, 124, 255].repeat(16), 4, 4) {
        std::fs::write(dir.join("textures").join(format!("{ASPHALT:08x}.dds")), &c.dds)?;
        textures.insert(ASPHALT, (false, 0x3F00));
    }
    // Without lightmaps everything is lit at full strength (FM2's lightmaps average about half): `FM2_SHADE` (default 1.0, the remaster lights lightmap-less draws itself)
    // scales the diffuse shader's light term and the blend shader's occlusion texture.
    let shade: f32 = std::env::var("FM2_SHADE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    // `h_diff_1_lm`'s lightmap floor c6.z (`FM2_LM_FLOOR`, default 1): the shader adds `max(c213.y, c6.z) * lightmap`
    // to the ambient term, and c213.y (FH1's night threshold) is 0 by day, so without this the lightmap does nothing.
    let lightmap_floor: f32 = std::env::var("FM2_LM_FLOOR").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let ao_byte = (255.0 * shade.clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8;
    if let Ok(c) = crate::textures::rgba_to_dds([ao_byte, ao_byte, ao_byte, 255].repeat(16), 4, 4) {
        std::fs::write(dir.join("textures").join(format!("{WHITE:08x}.dds")), &c.dds)?;
        textures.insert(WHITE, (false, 0x3F00));
    }
    let mut materials: Vec<serde_json::Value> = Vec::new();
    let mut by_tex: HashMap<String, u32> = HashMap::new();
    let mut blends = 0usize;
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();
    let mut tiles: BTreeMap<(i32, i32), BTreeMap<(u32, u32, u32), Batch>> = BTreeMap::new();
    let (mut textured, mut bare) = (0usize, 0usize);
    let one_sided = std::env::var("FM2_NO_FLIP").is_ok();
    let mut flipped = 0usize;
    // Roads: draws that bind masks and no colour texture. Their colour is a pixel-shader constant (`c_constant1`, the asphalt) modulated by
    // a fine grain mask; baked here into one small texture per (grain, colour). UNVERIFIED reading of the constants (see the handoff).
    let road_gain: f32 = std::env::var("FM2_ROAD_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let mut road_ids: Vec<Option<(u32, u32)>> = Vec::with_capacity(pending.len());
    let mut baked: HashMap<(u32, [u8; 3]), u32> = HashMap::new();
    let mut baked_new: Vec<u32> = Vec::new();
    for p in &pending {
        let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0);
        let has_colour = p.candidates.iter().filter(usable).any(|t| !mask(t.0));
        let masks: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| mask(t.0)).map(|t| t.0).collect();
        if has_colour || masks.is_empty() || std::env::var_os("FM2_NO_ROAD").is_some() {
            road_ids.push(None);
            continue;
        }
        let dims = |id: u32| {
            let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok()?;
            let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
            Some((u(16) as usize, u(12) as usize, b))
        };
        // The grain: the first small square mask (the wear-streak mask is a wide one).
        let grain = masks.iter().copied().find(|&id| dims(id).is_some_and(|(w, h, _)| w == h && w <= 512));
        let c1 = p.consts.get(&1).map(|v| [v[0], v[1], v[2]]).unwrap_or([0.04; 3]);
        let enc = |v: f32| (255.0 * (v * road_gain).clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8;
        let key = (grain.unwrap_or(0), [enc(c1[0]), enc(c1[1]), enc(c1[2])]);
        let id = *baked.entry(key).or_insert_with(|| {
            let id = 0xFFFF_2000 + baked_new.len() as u32;
            let (w, h) = (256usize, 256usize);
            let mut grain_r = vec![128u8; w * h];
            if let Some((gw, gh, b)) = grain.and_then(dims) {
                let fmt = texpresso::Format::Bc4;
                if let Some(data) = b.get(148..148 + fmt.compressed_size(gw, gh)) {
                    let mut rgba = vec![0u8; gw * gh * 4];
                    fmt.decompress(data, gw, gh, &mut rgba);
                    for y in 0..h {
                        for x in 0..w {
                            grain_r[y * w + x] = rgba[((y * gh / h) * gw + x * gw / w) * 4];
                        }
                    }
                }
            }
            let mean = (grain_r.iter().map(|&v| v as f32).sum::<f32>() / grain_r.len() as f32).max(1.0);
            let mut px = Vec::with_capacity(w * h * 4);
            for &g in &grain_r {
                let f = (1.0 + 0.6 * (g as f32 / mean - 1.0)).clamp(0.5, 1.6);
                for k in 0..3 {
                    px.push((255.0 * (c1[k] * road_gain * f).clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8);
                }
                px.push(255);
            }
            if let Ok(c) = crate::textures::rgba_to_dds(px, w as u32, h as u32) {
                let _ = std::fs::write(dir.join("textures").join(format!("{id:08x}.dds")), &c.dds);
            }
            baked_new.push(id);
            id
        });
        road_ids.push(Some((id, 0)));
    }
    for id in baked_new {
        textures.insert(id, (false, 0x3F00));
    }
    // Colour + wear mask (road strips with a separate mask uv): the mask's dark layer is the colour texture darkened (rubber).
    let wear_dark: f32 = std::env::var("FM2_WEAR_DARK").ok().and_then(|v| v.parse().ok()).unwrap_or(0.55);
    let mut dark_of: HashMap<u32, u32> = HashMap::new();
    let mut dark_ids: Vec<Option<u32>> = Vec::with_capacity(pending.len());
    let mut dark_new: Vec<u32> = Vec::new();
    for p in &pending {
        let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0);
        let cols: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| !mask(t.0)).map(|t| t.0).collect();
        let nmask = p.candidates.iter().filter(usable).filter(|t| mask(t.0)).count();
        if p.mask_raw.is_none() || cols.len() != 1 || nmask != 1 || std::env::var_os("FM2_NO_WEAR").is_some() {
            dark_ids.push(None);
            continue;
        }
        let a = cols[0];
        let id = *dark_of.entry(a).or_insert_with(|| {
            let id = 0xFFFF_3000 + dark_new.len() as u32;
            let path = dir.join("textures").join(format!("{a:08x}.dds"));
            let made = (|| {
                let dds = std::fs::read(&path).ok()?;
                let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
                let fmt = match u(128) {
                    71 => texpresso::Format::Bc1,
                    74 => texpresso::Format::Bc2,
                    77 => texpresso::Format::Bc3,
                    _ => return None,
                };
                let (h, w) = (u(12) as usize, u(16) as usize);
                let mut rgba = vec![0u8; w * h * 4];
                fmt.decompress(dds.get(148..148 + fmt.compressed_size(w, h))?, w, h, &mut rgba);
                for px in rgba.chunks_exact_mut(4) {
                    for c in &mut px[..3] {
                        *c = (255.0 * (*c as f32 / 255.0).powf(2.2).mul_add(wear_dark, 0.0).powf(1.0 / 2.2)).round() as u8;
                    }
                }
                crate::textures::rgba_to_dds(rgba, w as u32, h as u32).ok()
            })();
            if let Some(c) = made {
                let _ = std::fs::write(dir.join("textures").join(format!("{id:08x}.dds")), &c.dds);
            }
            dark_new.push(id);
            id
        });
        dark_ids.push(Some(id));
    }
    for id in dark_new {
        textures.insert(id, (false, 0x3F00));
    }
    for (pi, p) in pending.into_iter().enumerate() {
        if std::env::var_os("FM2_DEBUG").is_some() {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &p.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            if hi[0] - lo[0] > 120.0 || hi[2] - lo[2] > 120.0 {
                let desc: Vec<String> = p.candidates.iter().map(|t| format!("{:#x}/{}{}{}", t.0, t.1, if textures.contains_key(&t.0) { "+" } else { "-" }, if mask(t.0) { "m" } else { "" })).collect();
                println!("[fm2] big draw x {:.0}..{:.0} z {:.0}..{:.0} y {:.0}..{:.0} tris {} tex {desc:?}", lo[0], hi[0], lo[2], hi[2], lo[1], hi[1], p.indices.len() / 3);
            }
        }
        if let Some((ax, az, r)) = std::env::var("FM2_AT").ok().and_then(|v| {
            let f: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (f.len() >= 2).then(|| (f[0], f[1], f.get(2).copied().unwrap_or(0.0)))
        }) {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &p.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            if ax >= lo[0] - r && ax <= hi[0] + r && az >= lo[2] - r && az <= hi[2] + r && hi[0] - lo[0] < 200.0 {
                let lum: Vec<String> = p.candidates.iter().map(|c| texture_mean(&dir.join("textures").join(format!("{:08x}.dds", c.0))).map_or("?".to_owned(), |m| format!("{m:.0}"))).collect();
                let span = |k: usize| p.raw.iter().map(|v| v[k]).fold(f32::MIN, f32::max) - p.raw.iter().map(|v| v[k]).fold(f32::MAX, f32::min);
                let area: f32 = p.indices.chunks_exact(3).map(|t| {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| p.positions[i as usize]);
                    let (e, f) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                    let n = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
                    0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
                }).sum();
                let uvarea: f32 = p.indices.chunks_exact(3).map(|t| {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| p.raw[i as usize]);
                    0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs()
                }).sum();
                println!("[fm2]   raw uv span {:.2} x {:.2}, world area {area:.0} m2, uv area {uvarea:.4} => {:.1} m2 per uv unit", span(0), span(1), area / uvarea.max(1e-9));
                println!(
                    "[fm2] near {ax},{az}: lum {lum:?} model {} bbox x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0} tris {} lm {} lm_uv {} tex {:?}",
                    p.model, lo[0], hi[0], lo[1], hi[1], lo[2], hi[2], p.indices.len() / 3, p.lightmap.is_some(), p.lm_uv.len(), p.candidates
                );
            }
        }
        if let Some(cam) = std::env::var("FM2_WEDGE").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 6) {
            // x, y, z of the camera and x, y, z of the point it looks toward: draws in a +-14 degree cone, 15..90 m away
            let (o, f) = ([cam[0], cam[1], cam[2]], [cam[3] - cam[0], cam[4] - cam[1], cam[5] - cam[2]]);
            let fl = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            let c = p.positions.iter().fold([0.0f32; 3], |a, v| [a[0] + v[0], a[1] + v[1], a[2] + v[2]]);
            let n = p.positions.len().max(1) as f32;
            let c = [c[0] / n - o[0], c[1] / n - o[1], c[2] / n - o[2]];
            let cl = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            let cos = (c[0] * f[0] + c[1] * f[1] + c[2] * f[2]) / (cl * fl).max(1e-6);
            if cl > 15.0 && cl < 200.0 && cos > 0.985 && p.indices.len() / 3 <= 400 {
                let lum: Vec<String> = p.candidates.iter().map(|c| texture_mean(&dir.join("textures").join(format!("{:08x}.dds", c.0))).map_or("?".to_owned(), |m| format!("{m:.0}"))).collect();
                println!("[fm2] wedge d {cl:.0} cos {cos:.3} model {} tris {} tex {:?} lum {lum:?} at {:.0},{:.0},{:.0}", p.model, p.indices.len() / 3, p.candidates, c[0] + o[0], c[1] + o[1], c[2] + o[2]);
            }
        }
        let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0);
        let colours: Vec<(u32, u32)> = p.candidates.iter().filter(usable).filter(|t| !mask(t.0)).map(|t| (t.0, t.1)).collect();
        let masks: Vec<(u32, u32)> = p.candidates.iter().filter(usable).filter(|t| mask(t.0)).map(|t| (t.0, t.1)).collect();
        let tex = |(id, flags): (u32, u32)| json!({"id": id, "flags": flags, "file": format!("textures/{id:08x}.dds"), "word": textures[&id].1});
        // Two colour textures and one single-channel mask: FH1's two-way terrain blend (h_blnd2_ao: A, B, splat mask in slots 0, 1,
        // 2 and ambient occlusion in 5; texcoord 0 tiles the colours, texcoord 1 is the mask's).
        if std::env::var_os("FM2_ROADS").is_some() && colours.is_empty() && masks.len() >= 2 {
            let ids: Vec<String> = masks.iter().map(|m| format!("{:#x}", m.0)).collect();
            let c: Vec<String> = p.consts.iter().map(|(r, v)| format!("c{r}=({:.3},{:.3},{:.3},{:.3})", v[0], v[1], v[2], v[3])).collect();
            println!("[fm2] road model {} tris {} masks {ids:?} {}", p.model, p.indices.len() / 3, c.join(" "));
        }
        if let Ok(want) = std::env::var("FM2_PATTERN_AT") {
            let pat: String = p.candidates.iter().map(|c| if matches!(c.1, 8 | 9 | 12) { 'L' } else if !textures.contains_key(&c.0) { '?' } else if mask(c.0) { 'M' } else { 'C' }).collect();
            if pat == want {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for v in &p.positions {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
                let ids: Vec<String> = p.candidates.iter().map(|c| format!("{:#x}/{}", c.0, c.1)).collect();
                println!("[fm2] {want} model {} tris {} tex {ids:?} bbox x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0} uv0 span {:.2}x{:.2}", p.model, p.indices.len() / 3, lo[0], hi[0], lo[1], hi[1], lo[2], hi[2],
                    p.raw.iter().map(|v| v[0]).fold(f32::MIN, f32::max) - p.raw.iter().map(|v| v[0]).fold(f32::MAX, f32::min),
                    p.raw.iter().map(|v| v[1]).fold(f32::MIN, f32::max) - p.raw.iter().map(|v| v[1]).fold(f32::MAX, f32::min));
            }
        }
        if std::env::var_os("FM2_PATTERNS").is_some() {
            let pat: String = p.candidates.iter().map(|c| if matches!(c.1, 8 | 9 | 12) { 'L' } else if !textures.contains_key(&c.0) { '?' } else if mask(c.0) { 'M' } else { 'C' }).collect();
            *patterns.entry(pat).or_default() += 1;
        }
        let mut colours = colours;
        if let (Some(d), 1) = (dark_ids[pi], colours.len()) {
            colours.push((d, 0));
        }
        let blend = masks.len() == 1 && colours.len() >= 2 && std::env::var_os("FM2_NO_BLEND").is_none();
        // A decoded lightmap atlas tile that isn't used by the blend path (its shader shares the mask's uv, so the two
        // cannot be told apart yet). FM2_LIGHTMAPS=1 only (opt-in: the mapping is new).
        let lm_used = (lightmaps_on && p.lm_uv.len() == p.positions.len())
            .then_some(p.lightmap)
            .flatten()
            .filter(|(id, _, _)| textures.contains_key(id));
        let (key, material_json, attrs) = if blend {
            let (a, b2, m) = (colours[0], colours[1], masks[0]);
            let mut ao = tex((WHITE, 0));
            ao["flags"] = json!(0);
            let mut ps = vec![[1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 0.0, 0.0], [0.2, 8.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
            if let Some((lm_id, _, _)) = lm_used {
                // With a lightmap: `h_blnd2_ao_lm` (Light_Map in slot 7). The remaster reads the lightmap's uv from set 2 for FM2
                // (the mask keeps set 1: the two are different pairs in FM2's vertices); the faithful path shares the mask's.
                ps.push([0.0, 0.0, lightmap_floor, 0.0]);
                (
                    format!("blend_lm {} {} {} {}", a.0, b2.0, m.0, lm_id),
                    json!({
                        "shader": "shaders/track/h_blnd2_ao_lm.fx", "technique": 0, "vs": [], "ps": ps,
                        "textures": [tex(a), tex(b2), tex(m), null, null, ao, null, tex((lm_id, 0))],
                    }),
                    ATTR_UV0 | ATTR_UV1 | ATTR_UV2 | ATTR_COLOR,
                )
            } else {
                (
                    format!("blend {} {} {}", a.0, b2.0, m.0),
                    json!({
                        "shader": "shaders/track/h_blnd2_ao.fx", "technique": 0, "vs": [], "ps": ps,
                        "textures": [tex(a), tex(b2), tex(m), null, null, ao],
                    }),
                    ATTR_UV0 | ATTR_UV1 | ATTR_COLOR,
                )
            }
        } else if let Some((lm_id, _, _)) = lm_used {
            // The baked lightmap (`h_diff_1_lm`): the diffuse texture on uv0 (tiled), the cropped atlas tile on the
            // Light_Map sampler (slot 7) sampled with uv1. Draws with no colour texture (the road surface: its colour is a
            // shader constant) keep the flat grey, which the lightmap now modulates.
            // c6.z is the shader's lightmap floor (`max(c213.y, c6.z)`, 0 by day in FH1 → the lightmap does nothing; the
            // FM4 remaster calls it the `_lm` floor). Raise it so the baked lightmap contributes.
            let (id, flags) = colours.first().copied().or(road_ids[pi]).unwrap_or((ASPHALT, 0));
            // A diffuse with transparent texels (fences, foliage, signs) needs the cut-out shader, or its black RGB shows.
            let base = if keyed_ids.contains(&id) { "h_diff_opac_1" } else { "h_diff_1" };
            // The remaster scales the albedo by ps0.z: the lightmap path lights the material itself, so 1.
            let mut ps = vec![[1.0, 1.0, 1.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]];
            ps.push([0.0, 0.0, lightmap_floor, 0.0]);
            (
                format!("diff_lm {base} {id} {lm_id}"),
                json!({
                    "shader": format!("shaders/track/{base}_lm.fx"), "technique": 0, "vs": [], "ps": ps,
                    "textures": [tex((id, flags)), null, null, null, null, null, null, tex((lm_id, 0))],
                }),
                ATTR_UV0 | ATTR_UV1,
            )
        } else {
            // Draws with only a lightmap (the road surface: its colour is a shader constant) get a flat asphalt grey.
            let (id, flags) = colours.first().copied().or(road_ids[pi]).unwrap_or_else(|| {
                bare += 1;
                (ASPHALT, 0)
            });
            let base = if keyed_ids.contains(&id) { "h_diff_opac_1" } else { "h_diff_1" };
            (
                format!("diff {base} {id}"),
                json!({
                    "shader": format!("shaders/track/{base}.fx"), "technique": 0, "vs": [], "ps": [[1.0, 1.0, shade, 0.0]],
                    "textures": [tex((id, flags))],
                }),
                ATTR_UV0,
            )
        };
        textured += 1;
        blends += blend as usize;
        let material = *by_tex.entry(key).or_insert_with(|| {
            materials.push(material_json);
            materials.len() as u32 - 1
        });
        let normals = smooth_normals(&p.positions, &p.indices);
        let b = tiles.entry(p.tile).or_default().entry((material, 0, attrs)).or_default();
        let base = b.positions.len() as u32;
        b.positions.extend_from_slice(&p.positions);
        b.normals.extend(normals);
        b.uvs[0].extend_from_slice(&p.uv);
        if blend {
            b.uvs[1].extend_from_slice(p.mask_raw.as_ref().unwrap_or(&p.raw));
            if attrs & ATTR_UV2 != 0 {
                b.uvs[2].extend_from_slice(&p.lm_uv);
            }
            // The blend vertex shader reads a vertex colour (not used by the pixel shader's terrain path): white.
            b.colours.extend(std::iter::repeat([255u8; 4]).take(p.positions.len()));
        } else if lm_used.is_some() {
            b.uvs[1].extend_from_slice(&p.lm_uv);
        }
        // The winding differs between draws (not decoded): faces that point mostly sideways or up/down are judged by the
        // up-facing majority (terrain, roads and roofs face up), so a draw written the other way round is flipped.
        let up = p.indices.chunks_exact(3).fold((0i64, 0i64), |(u, d), t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| p.positions[i as usize]);
            let (e, f) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l < 1e-9 || n[1].abs() < 0.7 * l {
                (u, d)
            } else if n[1] > 0.0 {
                (u + 1, d)
            } else {
                (u, d + 1)
            }
        });
        if one_sided || up.0 >= up.1 {
            b.indices.extend(p.indices.iter().map(|i| i + base));
        } else {
            flipped += 1;
            b.indices.extend(p.indices.chunks_exact(3).flat_map(|t| [t[0] + base, t[2] + base, t[1] + base]));
        }
    }
    if !patterns.is_empty() {
        let mut v: Vec<_> = patterns.iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(*x.1));
        for (k, n) in v.iter().take(30) {
            println!("[fm2] pattern {k}: {n}");
        }
    }
    let mut index = Vec::new();
    for (&(x, z), t) in &tiles {
        let file = format!("tiles/{x}_{z}.bin");
        let (lo, hi, nv, ni) = write_tile(&dir.join(&file), t)?;
        index.push(json!({"x": x, "z": z, "file": file, "min": lo, "max": hi, "vertices": nv, "triangles": ni / 3}));
    }
    let tex_index: Vec<_> = textures.iter().map(|(id, (alpha, _))| json!({"id": id, "file": format!("textures/{id:08x}.dds"), "alpha": alpha})).collect();
    std::fs::create_dir_all(dir.join("props/templates"))?;
    std::fs::create_dir_all(dir.join("props/tiles"))?;
    let index_json = json!({
        "tile_size": TILE, "tiles": index, "far": null, "textures": tex_index, "zones": null,
        "props": {"tile_size": TILE, "tiles": [], "templates": [], "lods": [], "collision": {"smash": [], "solid": [], "bounds": {}}, "glows": null},
        "single_channel": "replicate",
    });
    std::fs::write(dir.join("index.json"), serde_json::to_vec_pretty(&index_json)?)?;
    std::fs::write(dir.join("materials.json"), serde_json::to_vec(&materials)?)?;
    println!(
        "[fm2] {}: {flipped} draws rewound, {textured} draws ({blends} terrain blends, {bare} lightmap-only: flat grey), {} materials, {} textures, {} tiles",
        src.name,
        materials.len(),
        textures.len(),
        tiles.len()
    );
    Ok(())
}

/// Crops each lightmap atlas tile out of its `.stx.bin` page (`fh1_formats::caff::parse_stx` entry) into a texture of its
/// own, in `dir`. Every FM2 tile sampled has a **negative** `su`: the tile is stored mirrored in the page, i.e. its u=0
/// column is the page's right edge of the tile rectangle. The crop samples the page per output texel (so the mirror is
/// handled without a separate flip). The mesh's lightmap uv (`Draw::uv_unorm`) runs 0..1 over the result.
/// Returns file id -> (has alpha, format word).
static DEBUG_TILES: std::sync::Mutex<BTreeMap<&str, usize>> = std::sync::Mutex::new(BTreeMap::new());

fn convert_lightmaps(
    stx: &Option<(Vec<u8>, Vec<fh1_formats::caff::CaffTexture>)>,
    tiles: &BTreeMap<u32, (u32, [f32; 4])>,
    dir: &Path,
) -> BTreeMap<u32, (bool, u32)> {
    let mut out = BTreeMap::new();
    let Some((d, ents)) = stx else { return out };
    // Decoded page (stx entry) -> (width, height, RGBA8), shared by its tiles.
    let mut pages: HashMap<u32, Option<(u32, u32, Vec<u8>)>> = HashMap::new();
    for (&id, &(page, tile)) in tiles {
        let entry = pages.entry(page).or_insert_with(|| {
            let ct = *ents.get(page as usize)?;
            let (_, faces) = fh1_formats::caff::decode_faces_of(d, ct).ok()?;
            let img = faces.into_iter().next()?;
            let (w, h) = (img.width, img.height);
            let mut rgba = fh1_formats::xds::to_rgba8(&img).ok()?;
            // Dxt5A decodes into R only: a lightmap is greyscale, so replicate and make it opaque (the engine only does
            // that for BC4 DDS, and this tile is re-encoded to BC3).
            for p in rgba.chunks_exact_mut(4) {
                p[1] = p[0];
                p[2] = p[0];
                p[3] = 255;
            }
            Some((w, h, rgba))
        });
        let Some((pw, ph, rgba)) = entry.as_ref() else { continue };
        let [su, sv, ou, ov] = tile;
        let (w, h) = (((su.abs() * *pw as f32).round() as usize).max(1), ((sv.abs() * *ph as f32).round() as usize).max(1));
        if w > *pw as usize || h > *ph as usize {
            continue;
        }
        // Page texel index of tile coordinate `t` (0..1) along an axis: page = offset + scale * t.
        let texel = |t: f32, o: f32, s: f32, n: u32| ((o + s * t) * n as f32 - 0.5).round().clamp(0.0, n as f32 - 1.0) as usize;
        let mut px = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let sy = texel((y as f32 + 0.5) / h as f32, ov, sv, *ph);
            for x in 0..w {
                let sx = texel((x as f32 + 0.5) / w as f32, ou, su, *pw);
                px.extend_from_slice(&rgba[(sy * *pw as usize + sx) * 4..][..4]);
            }
        }
        if std::env::var_os("FM2_DEBUG").is_some() {
            let mean = px.chunks_exact(4).map(|p| p[0] as u32).sum::<u32>() as f32 / (px.len() / 4).max(1) as f32;
            let k = if mean < 4.0 { "black" } else if mean < 40.0 { "dark" } else { "lit" };
            *DEBUG_TILES.lock().unwrap().entry(k).or_insert(0usize) += 1;
        }
        // Keep the page's fetch word so the engine picks gamma the same way (BC4 → linear).
        let word = ents.get(page as usize).map_or(0x0100, |c| c.raw_format);
        match crate::textures::rgba_to_dds(px, w as u32, h as u32) {
            Ok(mut c) => {
                c.word = word;
                if std::fs::write(dir.join(format!("{id:08x}.dds")), &c.dds).is_ok() {
                    out.insert(id, (c.alpha, c.word));
                }
            }
            Err(e) => println!("[fm2] lightmap {id:08x} ({w}x{h}): {e}"),
        }
    }
    out
}

/// HEURISTIC stand-in for the vertex shader's texture-coordinate scale (a power of two, not decoded): the one that makes a
/// texture repeat about every `TILE_M` metres on this draw (median metres per raw uv unit), 1 for a draw whose raw uvs already
/// span about that much (billboards, signs).
fn tiling_scale(pos: &[[f32; 3]], uv: &[[f32; 2]], idx: &[u32]) -> f32 {
    const TILE_M: f32 = 8.0;
    let mut m: Vec<f32> = Vec::new();
    for t in idx.chunks_exact(3).take(4000) {
        for (a, b) in [(0, 1), (1, 2)] {
            let (a, b) = (t[a] as usize, t[b] as usize);
            let dp = ((pos[a][0] - pos[b][0]).powi(2) + (pos[a][1] - pos[b][1]).powi(2) + (pos[a][2] - pos[b][2]).powi(2)).sqrt();
            let du = ((uv[a][0] - uv[b][0]).powi(2) + (uv[a][1] - uv[b][1]).powi(2)).sqrt();
            if dp > 1e-3 && du > 1e-5 {
                m.push(dp / du);
            }
        }
    }
    if m.len() < 4 {
        return 1.0;
    }
    m.sort_by(f32::total_cmp);
    let metres_per_unit = m[m.len() / 2];
    2f32.powf((metres_per_unit / TILE_M).log2().round()).clamp(1.0, 64.0)
}

/// A BC1 DDS (`rgba_to_dds` / game blocks, DX10 header) whose texels are mostly pure black (a foliage sheet keyed on black):
/// its level 0 as RGBA8 with the black texels' alpha cleared. None for any other texture.
fn black_keyed(dds: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
    if dds.get(0..4) != Some(b"DDS ") || u(84) != 0x3031_5844 || u(128) != 71 {
        return None;
    }
    let (h, w) = (u(12) as usize, u(16) as usize);
    let size = texpresso::Format::Bc1.compressed_size(w, h);
    let mut rgba = vec![0u8; w * h * 4];
    texpresso::Format::Bc1.decompress(dds.get(148..148 + size)?, w, h, &mut rgba);
    let dark = |p: &[u8]| p[0] <= 6 && p[1] <= 6 && p[2] <= 6;
    let black = rgba.chunks_exact(4).filter(|p| dark(p)).count();
    if black * 100 < w * h * 35 {
        return None;
    }
    for p in rgba.chunks_exact_mut(4) {
        if dark(p) {
            p[3] = 0;
        }
    }
    Some((w as u32, h as u32, rgba))
}

/// A BC2 / BC3 DDS (DX10 header) that is partly transparent (a cut-out sheet), as opposed to alpha that carries something else.
fn cutout_texture(dds: &[u8]) -> bool {
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
    let fmt = match (dds.get(0..4), u(84), u(128)) {
        (Some(b"DDS "), 0x3031_5844, 74) => texpresso::Format::Bc2,
        (Some(b"DDS "), 0x3031_5844, 77) => texpresso::Format::Bc3,
        _ => return false,
    };
    let (h, w) = (u(12) as usize, u(16) as usize);
    let Some(data) = dds.get(148..148 + fmt.compressed_size(w, h)) else { return false };
    let mut rgba = vec![0u8; w * h * 4];
    fmt.decompress(data, w, h, &mut rgba);
    let low = rgba.chunks_exact(4).filter(|p| p[3] < 128).count();
    // 3..97 % transparent: a sprite sheet. Fully transparent textures are an unused alpha channel (X8R8G8B8 decoded with
    // alpha 0), and a few percent is noise.
    low * 100 >= w * h * 3 && low * 100 <= w * h * 97
}

/// Mean RGB (0..255) of a BC1 / BC2 / BC3 DDS (DX10 header), for debugging.
fn texture_mean(path: &Path) -> Option<f32> {
    let dds = std::fs::read(path).ok()?;
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
    let fmt = match u(128) {
        71 => texpresso::Format::Bc1,
        74 => texpresso::Format::Bc2,
        77 => texpresso::Format::Bc3,
        _ => return None,
    };
    let (h, w) = (u(12) as usize, u(16) as usize);
    let mut rgba = vec![0u8; w * h * 4];
    fmt.decompress(dds.get(148..148 + fmt.compressed_size(w, h))?, w, h, &mut rgba);
    Some(rgba.chunks_exact(4).map(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) as f32 / 3.0).sum::<f32>() / (w * h) as f32)
}
