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
    /// The sampler slot of each candidate (`candidates` is `Draw::items` minus the unresolvable ones).
    cand_slots: Vec<usize>,
    /// Texture coordinates resolved from the draw's own shaders (`fm2_uv`), by sampler slot.
    resolved: BTreeMap<usize, Vec<[f32; 2]>>,
    /// The pixel shader's output alpha is a constant: the colour textures' alpha channel is not opacity.
    alpha_const: bool,
    /// The draw's pixel shader and its sampler bindings, for evaluating it on the CPU.
    ps_run: Option<crate::fm2_uv::PsRun>,
    /// Sampler slot -> draw constant register holding the atlas sub-rectangle the shader maps the coordinate into.
    atlas: BTreeMap<usize, u32>,
}

/// Pixel-shader microcode hashes (`ps_sig`) of the road shader family `FM2_BC4_SRGB` applies to (Nurburgring models 118, 122, 85, 2837).
const NB_ROAD_SIGS: &[u64] = &[0x4aa0_eec0_57a0_743d];

/// The BC4-is-sRGB family rules (STATUS.md) are ON by default as of 2026-10-19; `FM2_BC4_SRGB=0` turns them off and restores the earlier importer output.
fn bc4_srgb_on() -> bool {
    std::env::var("FM2_BC4_SRGB").map_or(true, |v| v != "0")
}

/// Per-family gamma table (STATUS.md, end 21 to 40): pixel-shader hash (`ps_sig`) -> the samplers whose texture the console decodes as sRGB for that shader,
/// as fitted against Xenia's sunlit road with the CPU interpreter. Bit n = sampler `tfN`; `BC4` = every single-channel BC4 sampler of the draw. Draws of these
/// shaders get the shader's own colour (neutral light, placeholder textures white) instead of the constants-based road colour, and a draw with a colour
/// texture gets that texture scaled to the shader's brightness. `FM2_BC4_SRGB=0` turns the whole rule off; `FM2_BC4_SRGB_ALL=1` applies the BC4 rule to every road draw.
const BC4: u32 = 0x8000_0000;
const FAMILY_GAMMA: &[(u64, u32)] = &[
    (0x4aa0_eec0_57a0_743d, BC4),        // Nurburgring road (its BC1 tf3 is a weight, stays linear)
    (0x1902_6bd2_5939_21ac, BC4),        // Tsukuba road (model 50)
    (0x5cd0_e87d_36cd_8908, BC4),        // Silverstone road
    (0x21d6_c861_8fa0_5dd2, BC4),        // Test Track road (model 321)
    (0xa851_b0ab_5755_db49, 1 << 1),     // New York h_diff_1 (BC3 colour tf1)
    (0x1eb3_44c3_a908_6634, 1 << 4),     // Laguna Seca palette road family (BC1 colour tf4)
    (0xbef0_b122_d3da_f8b8, 0),          // Sunset Infield road family (BC1 colour stays linear)
    (0xf1fe_1fd4_779c_1604, 0),          // Sebring asphalt family (models 153 / 158; texture 161 paints the left foreground; no gamma fits Xenia at the typical lightmap level)
];

/// The gamma sampler mask of a draw's shader (see `FAMILY_GAMMA`), or None when the family is not in the table.
fn family_gamma(p: &Pending) -> Option<u32> {
    if !bc4_srgb_on() {
        return None;
    }
    let sig = ps_sig(p);
    FAMILY_GAMMA.iter().find(|(h, _)| *h == sig).map(|(_, m)| *m).or_else(|| std::env::var_os("FM2_BC4_SRGB_ALL").is_some().then_some(BC4))
}

/// FNV-1a hash of a draw's pixel-shader microcode (big-endian bytes of its dwords); 0 when the draw has none: a shader family signature.
fn ps_sig(p: &Pending) -> u64 {
    p.ps_run.as_ref().map_or(0, |ps| ps.words.iter().flat_map(|w| w.to_be_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)))
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
        let shader_uv = if std::env::var("FM2_SHADER_UV").is_ok_and(|v| v == "0") { Vec::new() } else { crate::fm2_uv::resolve(&raw_model, &model, &|it| match list.get(it as usize).and_then(|&t| pvs.textures.get(t as usize)) { None => crate::fm2_uv::ItemKind::System, Some(t) if matches!(t.flags, 8 | 9 | 12) => crate::fm2_uv::ItemKind::Tile, Some(_) => crate::fm2_uv::ItemKind::Other }) };
        for (di, d) in model.draws.iter().enumerate() {
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
            let cand_slots: Vec<usize> = d.items.iter().enumerate().filter(|(_, &it)| list.get(it as usize).is_some_and(|&t| pvs.textures.get(t as usize).is_some())).map(|(j, _)| j).collect();
            let has_tile = candidates.iter().any(|t| matches!(t.1, 8 | 9 | 12 | 1));
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
                cand_slots,
                resolved: shader_uv.get(di).map(|s| s.slots.clone()).unwrap_or_default(),
                alpha_const: shader_uv.get(di).is_some_and(|s| s.alpha_const),
                ps_run: shader_uv.get(di).and_then(|s| s.ps.clone()),
                atlas: shader_uv.get(di).map(|s| s.atlas.clone()).unwrap_or_default(),
            });
            draws += 1;
        }
    }
    println!("[fm2] {models} models ({failed} failed), {draws} draws, skipped draws {skipped:?}");
    if std::env::var_os("FM2_UV_STATS").is_some() {
        let n = |i: usize| crate::fm2_uv::STATS[i].load(std::sync::atomic::Ordering::Relaxed);
        println!("[fm2] shader uv: {} draws unpaired, {} with no direct sampler, {} direct samplers on a complex vertex output, {} matrices without values, {} samplers resolved", n(0), n(1), n(2), n(3), n(4));
    }
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
    if let Err(e) = crate::fm2_sky::import(ar, &by_name, &pvs, &dir) {
        println!("[fm2] sky: {e}");
    }
    std::fs::create_dir_all(dir.join("textures"))?;
    std::fs::create_dir_all(dir.join("tiles"))?;
    let mut wanted = BTreeMap::new();
    for t in pending.iter().flat_map(|p| &p.candidates) {
        if t.1 & 1 == 0 && t.1 & 8 == 0 {
            wanted.insert(t.0, t.1 & 4 != 0);
        }
    }
    let mut textures = convert_textures(ar, &by_name, &wanted, &dir.join("textures"))?;
    // Atlas lookups (`frac(uv) * c.xy + c.zw`): the draw's colour is a sub-rectangle of a shared atlas that repeats. Cut the rectangle out into its
    // own texture so the plain unit coordinate (sampled with wrap) reproduces it; one cut per (texture, rectangle).
    if std::env::var_os("FM2_NO_ATLAS_CROP").is_none() {
        let mut cuts: HashMap<(u32, [u32; 4]), Option<u32>> = HashMap::new();
        let mut next = 0u32;
        let mut cropped = 0usize;
        for p in pending.iter_mut() {
            for (&slot, &creg) in p.atlas.clone().iter() {
                let (Some(k), Some(c)) = (p.cand_slots.iter().position(|&s| s == slot), p.consts.get(&creg).copied()) else { continue };
                let id = p.candidates[k].0;
                if !textures.contains_key(&id) || c[0] <= 0.0 || c[1] <= 0.0 {
                    continue;
                }
                let key = (id, c.map(f32::to_bits));
                let new = *cuts.entry(key).or_insert_with(|| {
                    let c2 = crop_atlas(&dir.join("textures").join(format!("{id:08x}.dds")), c)?;
                    let nid = 0xFFFF_5000 + next;
                    next += 1;
                    std::fs::write(dir.join("textures").join(format!("{nid:08x}.dds")), &c2.dds).ok()?;
                    textures.insert(nid, (c2.alpha, c2.word));
                    Some(nid)
                });
                if let Some(nid) = new {
                    p.candidates[k].0 = nid;
                    cropped += 1;
                }
            }
        }
        println!("[fm2] {cropped} atlas draws cut to {} sub-textures", cuts.values().flatten().count());
    }
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
    let mut opaque_alpha = 0usize;
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
                // A texture only ever drawn by shaders whose output alpha is a constant carries other data in its alpha (FM2 atlases).
                let opaque_use = || {
                    let mut uses = pending.iter().filter(|p| p.candidates.iter().any(|c| c.0 == id)).peekable();
                    uses.peek().is_some() && uses.all(|p| p.alpha_const)
                };
                if std::env::var_os("FM2_NO_ALPHA_CONST").is_some() || !opaque_use() {
                    if std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok().is_some_and(|b| cutout_texture(&b)) {
                        keyed_ids.insert(id);
                    }
                } else {
                    opaque_alpha += 1;
                }
            }
        }
        println!("[fm2] {keyed} black-keyed foliage textures given alpha, {} cut-out textures in all, {opaque_alpha} alpha atlases kept opaque", keyed_ids.len());
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
    // BC4 (DXGI 80 / 81) textures are single-channel masks, except the small runtime ones (PVS flags 1, <= 64x64): those are a model's own
    // lightmap carried as a plain `.stx.bin` texture instead of an atlas tile (`small_lm`), drawn as a lightmap, not as a mask or a colour.
    let is_bc4 = |id: u32| {
        std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok().is_some_and(|b| matches!(b.get(128..132).map(|w| u32::from_le_bytes(w.try_into().unwrap())), Some(80 | 81)))
    };
    let small_lm: std::collections::HashSet<u32> = pending
        .iter()
        .flat_map(|p| p.candidates.iter())
        .filter(|t| t.1 == 1 && std::env::var_os("FM2_NO_SMALL_LM").is_none())
        .map(|t| t.0)
        .filter(|&id| {
            let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).unwrap_or_default();
            let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
            is_bc4(id) && u(12) * u(16) <= 64 * 64
        })
        .collect();
    // Same form as the cropped atlas tiles: greyscale replicated into RGB (the remaster samples `.rgb`; BC4 alone would tint it red).
    for &id in &small_lm {
        let path = dir.join("textures").join(format!("{id:08x}.dds"));
        let Ok(b) = std::fs::read(&path) else { continue };
        let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
        let (w, h) = (u(16), u(12));
        let fmt = texpresso::Format::Bc4;
        if let Some(data) = b.get(148..148 + fmt.compressed_size(w, h)) {
            let mut rgba = vec![0u8; w * h * 4];
            fmt.decompress(data, w, h, &mut rgba);
            for p in rgba.chunks_exact_mut(4) {
                (p[1], p[2], p[3]) = (p[0], p[0], 255);
            }
            if let Ok(c) = crate::textures::rgba_to_dds(rgba, w as u32, h as u32) {
                let _ = std::fs::write(&path, &c.dds);
            }
        }
    }
    let mask = |id: u32| !small_lm.contains(&id) && is_bc4(id);
    // Tangent-space normal maps (mean (127, 127, 255)) ride along in the draws' item lists: they are not surface colours.
    let normal_ids: std::collections::HashSet<u32> = if std::env::var_os("FM2_NO_NORMAL_FILTER").is_some() {
        Default::default()
    } else {
        let used: std::collections::BTreeSet<u32> = pending.iter().flat_map(|p| p.candidates.iter().map(|t| t.0)).collect();
        used.into_iter()
            .filter(|&id| {
                let Ok(b) = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))) else { return false };
                let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
                let (h, w) = (u(12), u(16));
                let mut rgba = vec![0u8; w * h * 4];
                match u(128) {
                    71 | 77 => {
                        let fmt = if u(128) == 71 { texpresso::Format::Bc1 } else { texpresso::Format::Bc3 };
                        let Some(data) = b.get(148..148 + fmt.compressed_size(w, h)) else { return false };
                        fmt.decompress(data, w, h, &mut rgba);
                    }
                    // Uncompressed RGBA8.
                    28 => {
                        let Some(data) = b.get(148..148 + w * h * 4) else { return false };
                        rgba.copy_from_slice(data);
                    }
                    _ => return false,
                }
                let n = (w * h) as f32;
                let m = |k: usize| rgba.chunks_exact(4).map(|p| p[k] as f32).sum::<f32>() / n;
                (m(0) - 127.0).abs() < 14.0 && (m(1) - 127.0).abs() < 14.0 && m(2) > 235.0
            })
            .collect()
    };
    println!("[fm2] {} normal maps kept out of the colour lists", normal_ids.len());
    // Runtime textures (PVS flags 1) of at most 32x32 that are not masks are slots the game fills at run time or leaves unused (a 1x1 black
    // one fills an optional layer): they are no surface colour. Test Track's road draws listed one as their colour, so the road blended two
    // copies of black (the missing dark copy then drew white). `FM2_KEEP_PLACEHOLDER=1` keeps them as colours.
    let placeholder_ids: std::collections::HashSet<u32> = if std::env::var_os("FM2_KEEP_PLACEHOLDER").is_some() {
        Default::default()
    } else {
        pending
            .iter()
            .flat_map(|p| p.candidates.iter())
            .filter(|t| t.1 == 1)
            .map(|t| t.0)
            .filter(|&id| {
                let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).unwrap_or_default();
                let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
                !b.is_empty() && u(12) * u(16) <= 32 * 32 && !matches!(u(128), 80 | 81)
            })
            .collect()
    };
    println!("[fm2] {} runtime placeholder textures kept out of the colour lists", placeholder_ids.len());

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
    // The files' own winding is consistent (draws with a clear up / down majority are up-facing on every track), so it is kept. The old
    // up-facing-majority flip turned gantries (their undersides face down) inside out: mirrored sign text, missing boards. `FM2_FLIP=1` brings it back.
    let one_sided = std::env::var("FM2_FLIP").is_err();
    let mut flipped = 0usize;
    let mut shader_uv_draws = 0usize;
    let mut colour_fallback = 0usize;
    let mut fallback_why: BTreeMap<String, usize> = BTreeMap::new();
    // `FM2_BC4_SRGB=1` (off by default, UNVERIFIED in the engine): the road pixel shader of Nurburgring model 118 reads one BC1 texture as a blend
    // weight (luma) and two grey BC4 textures as the real road colours: albedo = lerp(tf5, tf4, luma(tf3)). The CPU interpreter matches the Xenia
    // pixel only when the BC4 greys are decoded as sRGB (STATUS.md). Such draws (one BC1/BC3 colour, then two small square BC4 textures of one size)
    // get a baked colour texture of that blend instead of the weight texture, times the shader's light gain `FM2_ROAD_GAIN`; the greys use their
    // mean (their own tiling uv is not the draw's). `FM2_BC4_SRGB_W=1` also decodes the weight texture.
    if bc4_srgb_on() {
        let srgb_dec = |b: u8| {
            let c = b as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let decode_w = std::env::var_os("FM2_BC4_SRGB_W").is_some();
        let gain: f32 = std::env::var("FM2_ROAD_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
        let read_dds = |id: u32, fmt: texpresso::Format| {
            let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok()?;
            let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
            let (h, w) = (u(12), u(16));
            let mut rgba = vec![0u8; w * h * 4];
            fmt.decompress(b.get(148..148 + fmt.compressed_size(w, h))?, w, h, &mut rgba);
            Some((w, h, rgba))
        };
        let mut made: HashMap<(u32, u32, u32), u32> = HashMap::new();
        let mut made_new: Vec<u32> = Vec::new();
        let mut swapped = 0usize;
        for p in pending.iter_mut() {
            let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
            let cols: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| !mask(t.0)).map(|t| t.0).collect();
            let masks: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| mask(t.0)).map(|t| t.0).collect();
            if cols.len() != 1 || masks.len() < 2 {
                continue;
            }
            if std::env::var_os("FM2_BC4_SRGB_SIG").is_some() {
                println!("[fm2] BC4-sRGB candidate: model {} ps_sig {:016x} colour {:#x} masks {:x?}", p.model, ps_sig(p), cols[0], masks);
            }
            // Only the Nurburgring road shader family (verified in the CPU interpreter, STATUS.md); other families with the same texture mix
            // (Sebring) are not known to need it.
            if !NB_ROAD_SIGS.contains(&ps_sig(p)) {
                continue;
            }
            let (Some(a), Some(b)) = (read_dds(masks[0], texpresso::Format::Bc4), read_dds(masks[1], texpresso::Format::Bc4)) else { continue };
            if a.0 != a.1 || a.0 > 256 || (a.0, a.1) != (b.0, b.1) {
                continue;
            }
            let mean = |px: &[u8]| px.chunks_exact(4).map(|p| srgb_dec(p[0])).sum::<f32>() / (px.len() / 4) as f32;
            let (m4, m5) = (mean(&a.2), mean(&b.2));
            let col = cols[0];
            let key = (col, masks[0], masks[1]);
            let id = if let Some(&id) = made.get(&key) {
                id
            } else {
                let fmt = match std::fs::read(dir.join("textures").join(format!("{col:08x}.dds"))).ok().and_then(|b| b.get(128..132).map(|w| u32::from_le_bytes(w.try_into().unwrap()))) {
                    Some(71) => texpresso::Format::Bc1,
                    Some(74) => texpresso::Format::Bc2,
                    Some(77) => texpresso::Format::Bc3,
                    _ => continue,
                };
                let Some((w, h, mut rgba)) = read_dds(col, fmt) else { continue };
                for px in rgba.chunks_exact_mut(4) {
                    let rgb = [px[0], px[1], px[2]].map(|v| if decode_w { srgb_dec(v) } else { v as f32 / 255.0 });
                    let lum = 0.114 * rgb[0] + 0.299 * rgb[1] + 0.587 * rgb[2];
                    let lin = (gain * (m5 + (m4 - m5) * lum)).clamp(0.0, 1.0);
                    let enc = (255.0 * lin.powf(1.0 / 2.2)).round() as u8;
                    (px[0], px[1], px[2]) = (enc, enc, enc);
                }
                let id = 0xFFFF_4000 + made_new.len() as u32;
                let Ok(c) = crate::textures::rgba_to_dds(rgba, w as u32, h as u32) else { continue };
                if std::fs::write(dir.join("textures").join(format!("{id:08x}.dds")), &c.dds).is_err() {
                    continue;
                }
                println!("[fm2] BC4-sRGB road: colour {col:#x} + greys {:#x}/{:#x} (means {m4:.3}/{m5:.3}) -> {id:#x}", masks[0], masks[1]);
                made.insert(key, id);
                made_new.push(id);
                id
            };
            for t in p.candidates.iter_mut() {
                if t.0 == col {
                    t.0 = id;
                }
            }
            swapped += 1;
        }
        for id in made_new {
            textures.insert(id, (false, 0x3F00));
        }
        println!("[fm2] BC4-sRGB: {swapped} draws use a baked blend");
    }
    // Runtime placeholder slots read white in the interpreter (their neutral value), then family draws that carry a COLOUR texture (Laguna Seca's palette
    // road, New York's `h_diff_1`, Sunset's road family) get that texture scaled to the brightness the draw's own pixel shader produces with neutral
    // light (`shader_albedo`, the family's sampler gamma mask): per channel scale = shader colour / mean sRGB-decoded texture colour. The texture keeps its
    // pattern; only its level (and tint) follows the shader. Draws without a colour texture go through the baked-road path below.
    WHITE_TEX.with(|w| {
        let mut w = w.borrow_mut();
        w.clear();
        for id in &placeholder_ids {
            w.insert(dir.join("textures").join(format!("{id:08x}.dds")));
        }
    });
    // Off by default (`FM2_FAMILY_COLOUR=1` turns it on): measured over the twelve tracks it did not improve the match (STATUS.md, 2026-10-22).
    if bc4_srgb_on() && std::env::var_os("FM2_FAMILY_COLOUR").is_some() {
        let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let mut scaled: HashMap<(u32, [u32; 3]), u32> = HashMap::new();
        let mut scaled_new: Vec<u32> = Vec::new();
        let mut tex_cache: HashMap<std::path::PathBuf, Option<(usize, usize, Vec<u8>)>> = HashMap::new();
        let mut shader_cache: HashMap<u64, Option<[f32; 3]>> = HashMap::new();
        let mut changed = 0usize;
        for p in pending.iter_mut() {
            if family_gamma(p).is_none() || NB_ROAD_SIGS.contains(&ps_sig(p)) {
                continue;
            }
            let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
            let Some(cid) = p.candidates.iter().filter(usable).find(|t| !mask(t.0)).map(|t| t.0) else { continue };
            if cid >= 0xFFFF_0000 {
                continue;
            }
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            ps_sig(p).hash(&mut h);
            for (r, v) in &p.consts {
                r.hash(&mut h);
                v.map(f32::to_bits).hash(&mut h);
            }
            p.candidates.iter().map(|c| c.0).collect::<Vec<_>>().hash(&mut h);
            let Some(a) = *shader_cache.entry(h.finish()).or_insert_with(|| shader_albedo(p, &dir.join("textures"), &mut tex_cache)) else { continue };
            let path = dir.join("textures").join(format!("{cid:08x}.dds"));
            let Some((w, hh, rgba)) = tex_cache.entry(path.clone()).or_insert_with(|| decode_rgba(&path)).clone() else { continue };
            let n = (w * hh).max(1) as f32;
            let mean: [f32; 3] = std::array::from_fn(|k| rgba.chunks_exact(4).map(|q| lin(q[k] as f32 / 255.0)).sum::<f32>() / n);
            let sc: [f32; 3] = std::array::from_fn(|k| (a[k] / mean[k].max(1e-4)).clamp(0.02, 8.0));
            if std::env::var_os("FM2_ROAD_DBG").is_some() {
                println!("[fm2] family colour model {} sig {:016x} colour {cid:#x} shader {a:.3?} texture mean {mean:.3?} scale {sc:.3?}", p.model, ps_sig(p));
            }
            let key = (cid, sc.map(|v| (v * 50.0).round() as u32));
            let id = *scaled.entry(key).or_insert_with(|| {
                let mut px = Vec::with_capacity(w * hh * 4);
                for q in rgba.chunks_exact(4) {
                    for k in 0..3 {
                        let v = (lin(q[k] as f32 / 255.0) * sc[k]).clamp(0.0, 1.0);
                        px.push((255.0 * if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }).round() as u8);
                    }
                    px.push(if q[3] == 250 { 255 } else { q[3] });
                }
                let id = 0xFFFF_5000 + scaled_new.len() as u32;
                if let Ok(c) = crate::textures::rgba_to_dds(px, w as u32, hh as u32) {
                    let _ = std::fs::write(dir.join("textures").join(format!("{id:08x}.dds")), &c.dds);
                }
                scaled_new.push(id);
                id
            });
            for t in p.candidates.iter_mut() {
                if t.0 == cid {
                    t.0 = id;
                }
            }
            changed += 1;
        }
        for id in scaled_new {
            textures.insert(id, (true, 0x3F00));
        }
        println!("[fm2] family albedo: {changed} colour draws scaled to their shader's brightness");
    }
    // Roads: draws that bind masks and no colour texture. Their colour is a pixel-shader constant (`c_constant1`, the asphalt) modulated by
    // a fine grain mask; baked here into one small texture per (grain, colour). UNVERIFIED reading of the constants (see the handoff).
    let road_tex_gain: f32 = std::env::var("FM2_ROAD_TEX_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let road_gain: f32 = std::env::var("FM2_ROAD_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let mut road_ids: Vec<Option<(u32, u32)>> = Vec::with_capacity(pending.len());
    // `FM2_SPEC_MAT=1`: the draw's own specular term (peak rise per unit level from the interpreter, material exponent from the model), carried in the material.
    let spec_mat = std::env::var_os("FM2_SPEC_MAT").is_some();
    let mut road_spec: Vec<Option<(f32, f32)>> = Vec::with_capacity(pending.len());
    // The same for every draw (not only the baked roads), cached by shader code, constants and textures: K per unit level and the exponent.
    let draw_spec: Vec<Option<(f32, f32)>> = if spec_mat {
        use std::hash::{Hash, Hasher};
        let mut memo: HashMap<u64, Option<(f32, f32)>> = HashMap::new();
        let mut local_cache: HashMap<std::path::PathBuf, Option<(usize, usize, Vec<u8>)>> = HashMap::new();
        pending
            .iter()
            .map(|p| {
                let (_, e) = p.ps_run.as_ref()?.spec_exp?;
                let mut h = std::collections::hash_map::DefaultHasher::new();
                p.ps_run.as_ref()?.words.hash(&mut h);
                for (r, v) in &p.consts {
                    r.hash(&mut h);
                    v.map(f32::to_bits).hash(&mut h);
                }
                p.candidates.iter().map(|c| c.0).collect::<Vec<_>>().hash(&mut h);
                *memo.entry(h.finish()).or_insert_with(|| shader_spec_k(p, &dir.join("textures"), &mut local_cache).map(|k| (k, e)))
            })
            .collect()
    } else {
        vec![None; pending.len()]
    };
    let mut road_spec: Vec<Option<(f32, f32)>> = Vec::with_capacity(pending.len());
    let mut baked: HashMap<(u32, [u8; 3], u32), u32> = HashMap::new();
    let mut baked_new: Vec<u32> = Vec::new();
    let mut road_tex_cache: HashMap<std::path::PathBuf, Option<(usize, usize, Vec<u8>)>> = HashMap::new();
    for p in &pending {
        let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
        let has_colour = p.candidates.iter().filter(usable).any(|t| !mask(t.0));
        let masks: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| mask(t.0)).map(|t| t.0).collect();
        if has_colour || masks.is_empty() || std::env::var_os("FM2_NO_ROAD").is_some() {
            road_ids.push(None);
            road_spec.push(None);
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
        // The shader scales the grain texture by c3 (0.7 on Tsukuba / Silverstone, 1.55 on Suzuka, whose asphalt is much lighter): relative to 0.7.
        let tex_gain = if std::env::var_os("FM2_NO_C3_GAIN").is_some() { 1.0 } else { p.consts.get(&3).map_or(1.0, |v| (v[0] / 0.7).clamp(1.0, 3.0)) };
        // The colour the draw's own pixel shader produces with neutral lighting (`shader_albedo`): replaces the constant heuristics. Opt-in (`FM2_SHADER_ROAD=1`): measured against Xenia it helped Suzuka but made Tsukuba, Silverstone and Mugello too bright.
        let albedo = if std::env::var_os("FM2_SHADER_ROAD").is_some() || family_gamma(p).is_some() { shader_albedo(p, &dir.join("textures"), &mut road_tex_cache).filter(|a| a.iter().cloned().fold(0.0, f32::max) > 0.03) } else { None };
        if std::env::var_os("FM2_ROAD_DBG").is_some() {
            println!("[fm2] road model {} shader albedo {albedo:.3?} (old: c1 {c1:.3?} x gain {road_gain}) sig {:016x} tris {} consts {:.2?} tex {:x?} spec_exp {:?}", p.model, ps_sig(p), p.indices.len() / 3, p.consts, p.candidates.iter().map(|c| c.0).collect::<Vec<_>>(), p.ps_run.as_ref().and_then(|r| r.spec_exp));
        }
        let enc01 = |v: f32| (255.0 * v.clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8;
        let key = match albedo {
            Some(a) => (grain.unwrap_or(0), [enc01(a[0]), enc01(a[1]), enc01(a[2])], 9999),
            None => (grain.unwrap_or(0), [enc(c1[0]), enc(c1[1]), enc(c1[2])], (tex_gain * 100.0).round() as u32),
        };
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
                if let Some(a) = albedo {
                    let f = (1.0 + 0.6 * (g as f32 / mean - 1.0)).clamp(0.5, 1.6);
                    for k in 0..3 {
                        px.push(enc01(a[k] * f));
                    }
                    px.push(255);
                    continue;
                }
                // A black road constant (`c_constant1` < 0.05, Tsukuba / Suzuka / Silverstone): the shader lerps toward the constant only where the
                // stone mask says so, otherwise the road is the grain texture itself, a mid-grey asphalt (`FM2_NO_ROAD_TEX=1` keeps the constant).
                if grain.is_some() && c1.iter().all(|v| *v < 0.1) && std::env::var_os("FM2_NO_ROAD_TEX").is_none() {
                    // The grain is a linear-value mask (gamma word 0x01); the baked texture is sRGB, so encode it.
                    // The shader sums four grain textures at different scales, which averages the speckle out: keep one grain's mean, soften its contrast.
                    let k: f32 = std::env::var("FM2_ROAD_TEX_CONTRAST").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
                    let g = mean + k * (g as f32 - mean);
                    let v = ((g / 255.0) * road_tex_gain * tex_gain).clamp(0.0, 1.0).powf(std::env::var("FM2_ROAD_ENC").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0 / 2.2));
                    let b = (255.0 * v).round() as u8;
                    px.extend_from_slice(&[b, b, b, 255]);
                    continue;
                }
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
        let spec_val = if spec_mat { p.ps_run.as_ref().and_then(|r| r.spec_exp).and_then(|(_, e)| shader_spec_k(p, &dir.join("textures"), &mut road_tex_cache).map(|k| (k, e))) } else { None };
        if std::env::var_os("FM2_ROAD_DBG").is_some() && spec_mat {
            println!("[fm2] road model {} spec (K, exp) {spec_val:?}", p.model);
        }
        road_spec.push(spec_val);
    }
    for id in baked_new {
        textures.insert(id, (false, 0x3F00));
    }
    // Tree sprites (`MLMC` / `MC` / `MMC`: a big opacity mask first, then a small colour map): the colour is a smooth 128x128 gradient
    // stretched over the sprite and the BC4 mask (a 512x512 sheet of leaf / branch silhouettes) is its opacity. Composed into one RGBA texture
    // per (mask, colour), drawn as a cut-out card at uv x1. The second mask (a 256x256 shading map) is ignored.
    let mut sprite_ids: Vec<Option<u32>> = Vec::with_capacity(pending.len());
    {
        let mut made: HashMap<(u32, u32), u32> = HashMap::new();
        let mut made_new: Vec<u32> = Vec::new();
        for p in &pending {
            let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
            let ordered: Vec<(u32, bool)> = p.candidates.iter().filter(usable).map(|t| (t.0, mask(t.0))).collect();
            let size = |id: u32| {
                let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).unwrap_or_default();
                let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
                (u(16), u(12))
            };
            let cols: Vec<u32> = ordered.iter().filter(|c| !c.1).map(|c| c.0).collect();
            let masks_o: Vec<u32> = ordered.iter().filter(|c| c.1).map(|c| c.0).collect();
            let mut first_mask = ordered.first().filter(|c| c.1).map(|c| c.0).filter(|&m| size(m).0 * size(m).1 >= 256 * 256);
            // `CM` (colour, then a 64x64..128x128 mask): the same sprite with a small sheet. 32x32 BC4 textures are lightmap-sized, not sprites.
            if first_mask.is_none() && masks_o.len() == 1 && (64 * 64..256 * 256).contains(&(size(masks_o[0]).0 * size(masks_o[0]).1)) && cols.len() == 1 {
                first_mask = Some(masks_o[0]);
            }
            let ok = cols.len() == 1 && first_mask.is_some() && size(cols[0]).0 * size(cols[0]).1 <= 128 * 128;
            if !ok || std::env::var_os("FM2_NO_SPRITE").is_some() {
                sprite_ids.push(None);
                continue;
            }
            let (m, c) = (first_mask.unwrap_or(0), cols[0]);
            let id = *made.entry((m, c)).or_insert_with(|| {
                let id = 0xFFFF_4000 + made_new.len() as u32;
                let read = |id: u32, fmt: texpresso::Format| {
                    let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).ok()?;
                    let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
                    let (h, w) = (u(12), u(16));
                    let mut rgba = vec![0u8; w * h * 4];
                    fmt.decompress(b.get(148..148 + fmt.compressed_size(w, h))?, w, h, &mut rgba);
                    Some((w, h, rgba, u(128)))
                };
                let made = (|| {
                    let (mw, mh, mrgba, _) = read(m, texpresso::Format::Bc4)?;
                    let (cw, ch, crgba, cf) = {
                        let b = std::fs::read(dir.join("textures").join(format!("{c:08x}.dds"))).ok()?;
                        let cf = u32::from_le_bytes(b.get(128..132)?.try_into().ok()?);
                        let fmt = match cf {
                            71 => texpresso::Format::Bc1,
                            74 => texpresso::Format::Bc2,
                            77 => texpresso::Format::Bc3,
                            _ => return None,
                        };
                        let (w, h, r, _) = read(c, fmt)?;
                        (w, h, r, cf)
                    };
                    let _ = cf;
                    let mut out = vec![0u8; mw * mh * 4];
                    for y in 0..mh {
                        for x in 0..mw {
                            // Bilinear sample of the colour map over the whole sprite.
                            let (fx, fy) = ((x as f32 + 0.5) / mw as f32 * cw as f32 - 0.5, (y as f32 + 0.5) / mh as f32 * ch as f32 - 0.5);
                            let (x0, y0) = (fx.floor().max(0.0) as usize, fy.floor().max(0.0) as usize);
                            let (x1, y1) = ((x0 + 1).min(cw - 1), (y0 + 1).min(ch - 1));
                            let (tx, ty) = ((fx - x0 as f32).clamp(0.0, 1.0), (fy - y0 as f32).clamp(0.0, 1.0));
                            for k in 0..3 {
                                let g = |xx: usize, yy: usize| crgba[(yy * cw + xx) * 4 + k] as f32;
                                let v = (g(x0, y0) * (1.0 - tx) + g(x1, y0) * tx) * (1.0 - ty) + (g(x0, y1) * (1.0 - tx) + g(x1, y1) * tx) * ty;
                                out[(y * mw + x) * 4 + k] = v.round() as u8;
                            }
                            out[(y * mw + x) * 4 + 3] = mrgba[(y * mw + x) * 4];
                        }
                    }
                    crate::textures::rgba_to_dds(out, mw as u32, mh as u32).ok()
                })();
                if let Some(c) = made {
                    let _ = std::fs::write(dir.join("textures").join(format!("{id:08x}.dds")), &c.dds);
                }
                made_new.push(id);
                id
            });
            sprite_ids.push(Some(id));
        }
        for id in made_new {
            textures.insert(id, (true, 0x3F00));
            keyed_ids.insert(id);
        }
    }
    // Painted strips with no colour texture (the start-line chequer): a long thin chequer mask (BC4, 8:1), sometimes with a second long thin opacity mask. The shader
    // builds the paint from the chequer mask; here the chequer, read as greyscale, is the colour of a plain opaque card on its own uv (the
    // opacity mask is nearly white inside the strip). `FM2_NO_CHECKER=1` keeps the old (dark) look.
    let mut checker_of: Vec<Option<(u32, u32)>> = Vec::with_capacity(pending.len());
    {
        let mut made: HashMap<u32, u32> = HashMap::new();
        let mut made_new: Vec<u32> = Vec::new();
        for p in &pending {
            let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
            let dims = |id: u32| {
                let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).unwrap_or_default();
                let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
                (u(16), u(12))
            };
            let no_colour = !p.candidates.iter().filter(usable).any(|c| !mask(c.0) && c.1 != 1);
            let mut strips: Vec<u32> = p.candidates.iter().filter(usable).filter(|c| mask(c.0)).map(|c| c.0).filter(|&m| { let (w, h) = dims(m); h > 0 && w >= 8 * h && w >= 512 }).collect();
            strips.dedup();
            strips.sort_unstable();
            strips.dedup();
            if !no_colour || strips.is_empty() || strips.len() > 2 || std::env::var_os("FM2_NO_CHECKER").is_some() {
                checker_of.push(None);
                continue;
            }
            // The chequer is the strip with the larger spread of values.
            let spread = |id: u32| -> f32 {
                let Ok(b) = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))) else { return 0.0 };
                let (w, h) = dims(id);
                let Some(data) = b.get(148..148 + texpresso::Format::Bc4.compressed_size(w, h)) else { return 0.0 };
                let mut rgba = vec![0u8; w * h * 4];
                texpresso::Format::Bc4.decompress(data, w, h, &mut rgba);
                let n = (w * h) as f32;
                let m = rgba.chunks_exact(4).map(|q| q[0] as f32).sum::<f32>() / n;
                (rgba.chunks_exact(4).map(|q| (q[0] as f32 - m).powi(2)).sum::<f32>() / n).sqrt()
            };
            let src = strips.iter().copied().max_by(|&a, &b| spread(a).total_cmp(&spread(b))).unwrap_or(strips[0]);
            // A single strip counts only when it really is a chequer (a wide spread of values), not a soft opacity ramp.
            if spread(src) < 45.0 {
                checker_of.push(None);
                continue;
            }
            let id = *made.entry(src).or_insert_with(|| {
                let id = 0xFFFF_5000 + made_new.len() as u32;
                let made = (|| {
                    let b = std::fs::read(dir.join("textures").join(format!("{src:08x}.dds"))).ok()?;
                    let (w, h) = dims(src);
                    let mut rgba = vec![0u8; w * h * 4];
                    texpresso::Format::Bc4.decompress(b.get(148..148 + texpresso::Format::Bc4.compressed_size(w, h))?, w, h, &mut rgba);
                    // The chequer's light squares peak below 255 (the shader scales them by a constant): stretch to white.
                    let peak = rgba.chunks_exact(4).map(|q| q[0]).max().unwrap_or(255).max(1) as f32;
                    for q in rgba.chunks_exact_mut(4) {
                        let v = (q[0] as f32 * 255.0 / peak).min(255.0) as u8;
                        (q[0], q[1], q[2], q[3]) = (v, v, v, 255);
                    }
                    crate::textures::rgba_to_dds(rgba, w as u32, h as u32).ok()
                })();
                if let Some(c) = made {
                    let _ = std::fs::write(dir.join("textures").join(format!("{id:08x}.dds")), &c.dds);
                }
                made_new.push(id);
                id
            });
            checker_of.push(Some((id, src)));
        }
        for id in made_new {
            textures.insert(id, (false, 0x3F00));
        }
    }
    // Colour + wear mask (road strips with a separate mask uv): the mask's dark layer is the colour texture darkened (rubber).
    let wear_dark: f32 = std::env::var("FM2_WEAR_DARK").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    let mut dark_of: HashMap<u32, u32> = HashMap::new();
    let mut dark_ids: Vec<Option<(u32, u32)>> = Vec::with_capacity(pending.len());
    // The base colour a wear blend darkens (the tiled one of a two-colour road draw; None = the draw's first colour).
    let mut dark_base: Vec<Option<u32>> = Vec::with_capacity(pending.len());
    let mut dark_invert: Vec<bool> = Vec::with_capacity(pending.len());
    let mut dark_new: Vec<u32> = Vec::new();
    for (pi_dark, p) in pending.iter().enumerate() {
        let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
        let cols: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| !mask(t.0)).map(|t| t.0).collect();
        let mask_ids: Vec<u32> = p.candidates.iter().filter(usable).filter(|t| mask(t.0)).map(|t| t.0).collect();
        // The base: the draw's one colour texture, or the baked road. The wear mask: the largest mask (>= 512x512; a small square
        // 512x512 shared by many models is the asphalt grain, a 2048x512 / 512x1024 one the wear).
        let area = |id: u32| {
            let b = std::fs::read(dir.join("textures").join(format!("{id:08x}.dds"))).unwrap_or_default();
            let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
            u(12) * u(16)
        };
        let wear_mask = mask_ids.iter().copied().max_by_key(|&m| area(m)).filter(|&m| area(m) >= 512 * 512);
        // The uv extent of a colour's resolved coordinate (the wider axis): the tiled layer of a road spans many units, its one-pass overlay one.
        let extent = |id: u32| {
            let u = p.candidates.iter().enumerate().filter(|(_, c)| c.0 == id).find_map(|(k, _)| p.cand_slots.get(k).and_then(|s| p.resolved.get(s)))?;
            let (lo, hi) = u.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |a, t| ([a.0[0].min(t[0]), a.0[1].min(t[1])], [a.1[0].max(t[0]), a.1[1].max(t[1])]));
            (!u.is_empty()).then_some((hi[0] - lo[0]).max(hi[1] - lo[1]))
        };
        // Road shoulders list the tiled asphalt plus a one-pass overlay colour, with the streak mask and the grain: the streak darkens the asphalt
        // (`FM2_NO_ROAD_WEAR=1` keeps the overlay blend).
        let road_pair = cols.len() == 2 && mask_ids.len() == 2 && std::env::var_os("FM2_NO_ROAD_WEAR").is_none();
        let tiled = if road_pair { cols.iter().copied().find(|&c| extent(c).is_some_and(|v| v >= 2.0) && cols.iter().filter(|&&o| o != c).all(|&o| extent(o).is_some_and(|v| v < 1.5))) } else { None };
        let base = if cols.len() == 1 { Some(cols[0]) } else if cols.is_empty() { road_ids[pi_dark].map(|r| r.0) } else { tiled };
        if sprite_ids[pi_dark].is_some() || checker_of[pi_dark].is_some() {
            dark_ids.push(None);
            dark_base.push(None);
            dark_invert.push(false);
            continue;
        }
        // A cut-out base (the start-line chequer: grey squares opaque, the others transparent) is a decal: the wear blend has no alpha, so it
        // would draw the whole card. `FM2_NO_DECAL_CUTOUT=1` keeps the blend.
        let base = base.filter(|b| !(keyed_ids.contains(b) && std::env::var_os("FM2_NO_DECAL_CUTOUT").is_none() && !p.alpha_const));
        let (Some(a), Some(wm)) = (base, wear_mask) else {
            dark_ids.push(None);
            dark_base.push(None);
            dark_invert.push(false);
            continue;
        };
        if std::env::var_os("FM2_NO_WEAR").is_some() || (cols.len() == 1 && mask_ids.len() == 1 && p.mask_raw.is_none() && p.candidates.len() < 3) {
            dark_ids.push(None);
            dark_base.push(None);
            dark_invert.push(false);
            continue;
        }
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
        // A "wear" mask that is mostly bright (mean > 140: Suzuka's verge, a grass-blade detail image) is not a streak mask: the shader blends the
        // other way round, so the pair swaps. Streak masks are dark (Road Atlanta 38). `FM2_NO_WEAR_INVERT=1` keeps the old order.
        let bright = (|| {
            let b = std::fs::read(dir.join("textures").join(format!("{wm:08x}.dds"))).ok()?;
            let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
            let (h, w) = (u(12), u(16));
            let mut rgba = vec![0u8; w * h * 4];
            texpresso::Format::Bc4.decompress(b.get(148..148 + texpresso::Format::Bc4.compressed_size(w, h))?, w, h, &mut rgba);
            Some(rgba.chunks_exact(4).map(|q| q[0] as f32).sum::<f32>() / (w * h) as f32 > 140.0)
        })().unwrap_or(false) && std::env::var_os("FM2_NO_WEAR_INVERT").is_none();
        // Only for a real colour texture (not a baked road): Suzuka's verge (a tiled strip under a tiled grass image) and Laguna Seca's road (grey
        // asphalt darkened only in the rubber lanes). Measured against Xenia, Laguna Seca's road went from 0.27x to 1.29x; no other map changed.
        dark_invert.push(bright && !cols.is_empty());
        dark_ids.push(Some((id, wm)));
        dark_base.push(Some(a));
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
        let usable = |t: &&(u32, u32, usize)| t.1 & 8 == 0 && (t.1 == 1 || t.1 & 1 == 0) && textures.contains_key(&t.0) && !small_lm.contains(&t.0) && !normal_ids.contains(&t.0) && !placeholder_ids.contains(&t.0);
        // A texture the draw's pixel shader samples as a cube map is an environment map: neither a colour nor a mask.
        let cube = |id: u32| p.candidates.iter().enumerate().any(|(k, c)| c.0 == id && p.cand_slots.get(k).and_then(|s| p.resolved.get(s)).is_some_and(|u| u.is_empty()));
        let colours: Vec<(u32, u32)> = p.candidates.iter().filter(usable).filter(|t| !mask(t.0) && !cube(t.0)).map(|t| (t.0, t.1)).collect();
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
                let ids: Vec<String> = p
                    .candidates
                    .iter()
                    .map(|c| {
                        let d = std::fs::read(dir.join("textures").join(format!("{:08x}.dds", c.0))).unwrap_or_default();
                        let u = |o: usize| u32::from_le_bytes(d.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap());
                        format!("{:#x}/{}/{}x{}", c.0, c.1, u(16), u(12))
                    })
                    .collect();
                println!("[fm2] {want} model {} tris {} tex {ids:?} bbox x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0} uv0 span {:.2}x{:.2}", p.model, p.indices.len() / 3, lo[0], hi[0], lo[1], hi[1], lo[2], hi[2],
                    p.raw.iter().map(|v| v[0]).fold(f32::MIN, f32::max) - p.raw.iter().map(|v| v[0]).fold(f32::MAX, f32::min),
                    p.raw.iter().map(|v| v[1]).fold(f32::MIN, f32::max) - p.raw.iter().map(|v| v[1]).fold(f32::MAX, f32::min));
            }
        }
        if std::env::var_os("FM2_PATTERNS").is_some() {
            let pat: String = p.candidates.iter().map(|c| if matches!(c.1, 8 | 9 | 12) { 'L' } else if !textures.contains_key(&c.0) { '?' } else if mask(c.0) { 'M' } else { 'C' }).collect();
            *patterns.entry(pat).or_default() += 1;
        }
        let (mut colours, mut masks) = (colours, masks);
        // Textures bound to samplers the shader reads a surface uv through come first: the item list can also carry textures of samplers the
        // colour path does not use (the shadow-map slots hold placeholders).
        {
            let resolved_colour = |id: u32| p.candidates.iter().enumerate().filter(|(_, c)| c.0 == id).any(|(k, _)| p.cand_slots.get(k).and_then(|s| p.resolved.get(s)).is_some_and(|u| !u.is_empty()));
            colours.sort_by_key(|c| !resolved_colour(c.0));
        }
        // Terrain blends list their layers' colours with a greyscale detail / AO image among them: with three or more colours, the grey ones are
        // not a layer, and taking one as the blend's first colour greyed the terrain (Nurburgring's verges). `FM2_KEEP_GREY=1` keeps them.
        if colours.len() >= 3 && masks.len() == 1 && std::env::var_os("FM2_KEEP_GREY").is_none() {
            let colour_ones: Vec<(u32, u32)> = colours.iter().copied().filter(|c| !is_grey_texture(&dir.join("textures").join(format!("{:08x}.dds", c.0)))).collect();
            if colour_ones.len() >= 2 {
                colours = colour_ones;
            }
        }
        let sprite = sprite_ids[pi];
        if let Some(sid) = sprite {
            colours = vec![(sid, 4)];
            masks = Vec::new();
        }
        if let Some((cid, _)) = checker_of[pi] {
            colours = vec![(cid, 0)];
            masks = Vec::new();
        }
        if let Some((d, wm)) = dark_ids[pi] {
            // Wear blend: the colour (or the baked road) and its darkened copy, by the wear mask.
            let base = colours.iter().copied().find(|c| dark_base[pi] == Some(c.0)).or(colours.first().copied()).or(road_ids[pi]).unwrap_or((ASPHALT, 0));
            colours = if dark_invert[pi] { vec![(d, 0), base] } else { vec![base, (d, 0)] };
            masks = vec![(wm, 4)];
        }
        // Three "colours" with no mask, one of them a greyscale DXT texture spanning the draw once and the other two tiled (grass verges: a hand-painted
        // mix mask stored as a DXT1 image): that texture is the blend mask. `FM2_NO_GREY_MASK=1` keeps it as a colour (drawn white).
        if masks.is_empty() && colours.len() == 3 && std::env::var_os("FM2_NO_GREY_MASK").is_none() {
            // The wider of the two coordinate spans: a layer tiled along the road only (Road Atlanta's verge) barely spans the other axis.
            let span = |id: u32| {
                let u = p.candidates.iter().enumerate().filter(|(_, c)| c.0 == id).find_map(|(k, _)| p.cand_slots.get(k).and_then(|s| p.resolved.get(s)))?;
                let (lo, hi) = u.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |a, t| ([a.0[0].min(t[0]), a.0[1].min(t[1])], [a.1[0].max(t[0]), a.1[1].max(t[1])]));
                (!u.is_empty()).then_some((hi[0] - lo[0]).max(hi[1] - lo[1]))
            };
            let cand = (0..3).find(|&i| span(colours[i].0).is_some_and(|v| v < 1.5) && (0..3).filter(|&j| j != i).all(|j| span(colours[j].0).is_some_and(|v| v >= 2.0)));
            if let Some(i) = cand.filter(|&i| is_grey_texture(&dir.join("textures").join(format!("{:08x}.dds", colours[i].0)))) {
                let m = colours.remove(i);
                masks = vec![m];
                // The shader lerps the item-order colours the other way round (mask white = the first listed layer): swap to the blend's A / B.
                colours.swap(0, 1);
            }
        }
        // Two colours and two masks where one colour is tiled and the other spans the draw once (road shoulders: asphalt + a strip of edge
        // concrete, blended by the strip's own mask uv): a blend of the tiled colour (A) and the strip (B) by the one-pass mask. The second mask
        // (the asphalt grain) is dropped. `FM2_NO_STRIP_BLEND=1` keeps the old first-colour-only look.
        if masks.len() == 2 && colours.len() == 2 && dark_ids[pi].is_none() && road_ids[pi].is_none() && std::env::var_os("FM2_NO_STRIP_BLEND").is_none() {
            let span = |id: u32| {
                let u = p.candidates.iter().enumerate().filter(|(_, c)| c.0 == id).find_map(|(k, _)| p.cand_slots.get(k).and_then(|s| p.resolved.get(s)))?;
                let (lo, hi) = u.iter().fold((f32::MAX, f32::MIN), |a, t| (a.0.min(t[1]), a.1.max(t[1])));
                (u.len() > 0).then_some(hi - lo)
            };
            let (s0, s1) = (span(colours[0].0), span(colours[1].0));
            let one_pass = |s: Option<f32>| s.is_some_and(|v| v < 2.0);
            let tiled = |s: Option<f32>| s.is_some_and(|v| v >= 4.0);
            let strip_mask = masks.iter().copied().find(|m| span(m.0).is_some_and(|v| v < 2.0));
            if let Some(sm) = strip_mask {
                if tiled(s0) && one_pass(s1) {
                    masks = vec![sm];
                } else if tiled(s1) && one_pass(s0) {
                    colours.swap(0, 1);
                    masks = vec![sm];
                }
            }
        }
        let blend = masks.len() == 1 && colours.len() >= 2 && std::env::var_os("FM2_NO_BLEND").is_none();
        // A decoded lightmap atlas tile that isn't used by the blend path (its shader shares the mask's uv, so the two
        // cannot be told apart yet). FM2_LIGHTMAPS=1 only (opt-in: the mapping is new).
        let small = p.candidates.iter().find(|t| small_lm.contains(&t.0)).map(|t| (t.0, 0u32, [1.0f32, 1.0, 0.0, 0.0]));
        let lm_used = (lightmaps_on && p.lm_uv.len() == p.positions.len())
            .then(|| p.lightmap.or(small))
            .flatten()
            .filter(|(id, _, _)| textures.contains_key(id))
            // The chequer card is lit like the road around it, not by the (dark) tile its draw binds.
            .filter(|_| checker_of[pi].is_none());
        let (key, material_json, attrs) = if blend {
            let (a, b2, m) = (colours[0], colours[1], masks[0]);
            let mut ao = tex((WHITE, 0));
            ao["flags"] = json!(0);
            let mut ps = vec![[1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 0.0, 0.0], [0.2, 8.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
            if let Some((lm_id, _, _)) = lm_used {
                // With a lightmap: `h_blnd2_ao_lm` (Light_Map in slot 7). The remaster reads the lightmap's uv from set 2 for FM2
                // (the mask keeps set 1: the two are different pairs in FM2's vertices); the faithful path shares the mask's.
                ps.push([0.0, 0.0, lightmap_floor, 0.0]);
                // A baked road draw's own specular term (see the diffuse branch): ps[8] = (K per unit level, material exponent).
                let spec = draw_spec[pi];
                let spec_key = spec.map_or(String::new(), |(k, e)| format!(" spec {k:.4} {e:.3}"));
                if let Some((k, e)) = spec {
                    ps.push([0.0; 4]);
                    ps.push([k, e, 0.0, 0.0]);
                }
                (
                    format!("blend_lm {} {} {} {}{spec_key}", a.0, b2.0, m.0, lm_id),
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
            // The road's own specular term: ps[8] = (K per unit level, material exponent); remaster.rs hands it to the shader.
            let spec = draw_spec[pi];
            let spec_key = spec.map_or(String::new(), |(k, e)| format!(" spec {k:.4} {e:.3}"));
            if let Some((k, e)) = spec {
                ps.push([0.0; 4]);
                ps.push([k, e, 0.0, 0.0]);
            }
            (
                format!("diff_lm {base} {id} {lm_id}{spec_key}"),
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
        if let Some(f) = std::env::var("FM2_NEAR").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 3) {
            // x, z, radius: draws with any vertex within the radius (horizontal) of (x, z), with their lightmap tile and shader hash
            let near = p.positions.iter().any(|q| (q[0] - f[0]).powi(2) + (q[2] - f[1]).powi(2) < f[2] * f[2]);
            if near {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for q in &p.positions {
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                println!("[fm2] near: model {} ps_sig {:016x} tris {} cands {:?} lightmap {:?} box x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0}", p.model, ps_sig(&p), p.indices.len() / 3, p.candidates, p.lightmap.map(|l| l.0), lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
            }
        }
        if let Some(sig) = std::env::var("FM2_FIND_SIG").ok().and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
            if ps_sig(&p) == sig {
                println!("[fm2] find-sig: model {} tris {} cands {:?}", p.model, p.indices.len() / 3, p.candidates);
            }
        }
        if let Some(id) = std::env::var("FM2_FIND_TEX").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
            if p.candidates.iter().any(|c| c.0 == id) {
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for u in &p.lm_uv {
                    for k in 0..2 {
                        lo[k] = lo[k].min(u[k]);
                        hi[k] = hi[k].max(u[k]);
                    }
                }
                let c = p.positions.iter().fold([0.0f32; 3], |a, q| [a[0] + q[0], a[1] + q[1], a[2] + q[2]]);
                let n = p.positions.len().max(1) as f32;
                println!("[fm2] find-tex {id:#x}: model {} ps_sig {:016x} tris {} cands {:?} consts {:?} lm_uv {lo:.3?}..{hi:.3?} lightmap {:?} centre {:.0},{:.0},{:.0}", p.model, ps_sig(&p), p.indices.len() / 3, p.candidates, p.consts, p.lightmap, c[0] / n, c[1] / n, c[2] / n);
            }
        }
        if let Some(f) = std::env::var("FM2_UNDER").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 3) {
            // x, z, y: draws with a triangle that contains (x, z) at about height y, with the material they ended up with
            let hit = p.indices.chunks_exact(3).any(|t| {
                let q = [t[0], t[1], t[2]].map(|i| p.positions[i as usize]);
                let d = (q[1][2] - q[2][2]) * (q[0][0] - q[2][0]) + (q[2][0] - q[1][0]) * (q[0][2] - q[2][2]);
                if d.abs() < 1e-9 { return false; }
                let a = ((q[1][2] - q[2][2]) * (f[0] - q[2][0]) + (q[2][0] - q[1][0]) * (f[1] - q[2][2])) / d;
                let b = ((q[2][2] - q[0][2]) * (f[0] - q[2][0]) + (q[0][0] - q[2][0]) * (f[1] - q[2][2])) / d;
                let c = 1.0 - a - b;
                a >= 0.0 && b >= 0.0 && c >= 0.0 && (a * q[0][1] + b * q[1][1] + c * q[2][1] - f[2]).abs() < std::env::var("FM2_UNDER_TOL").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.4)
            });
            if hit {
                println!("[fm2] under: model {} ps_sig {:016x} tris {} cands {:?} material {} {} blend {blend} colours {colours:?} masks {masks:?}", p.model, ps_sig(&p), p.indices.len() / 3, p.candidates, material, materials[material as usize]["shader"]);
            }
        }
        let normals = smooth_normals(&p.positions, &p.indices);
        let b = tiles.entry(p.tile).or_default().entry((material, 0, attrs)).or_default();
        let base = b.positions.len() as u32;
        b.positions.extend_from_slice(&p.positions);
        b.normals.extend(normals);
        // Sprite cards map their sheet once (x1), not tiled.
        // The texture coordinates the draw's shaders feed the colour / mask samplers (`fm2_uv`), when they resolved; else the old guess.
        let slot_uv = |id: u32| p.candidates.iter().enumerate().filter(|(_, c)| c.0 == id).find_map(|(k, _)| p.cand_slots.get(k).and_then(|s| p.resolved.get(s)).filter(|u| !u.is_empty()));
        // Sprite cards sample their sheet (a mask) with the first resolved coordinate; wear blends keep their base colour's and mask's.
        // A baked road (colour from shader constants) tiles with its grain mask's coordinate: the first resolved mask that is not the wear streak.
        let wear_id = dark_ids[pi].map(|d| d.1);
        let grain_uv = || p.candidates.iter().filter(|c| mask(c.0) && Some(c.0) != wear_id).find_map(|c| slot_uv(c.0));
        let colour_uv = if let Some((_, src)) = checker_of[pi] { slot_uv(src) } else if sprite.is_some() { p.resolved.values().next() } else { colours.first().and_then(|c| slot_uv(c.0)).or_else(|| if colours.first().is_some_and(|c| p.candidates.iter().any(|x| x.0 == c.0)) { None } else { grain_uv() }) };
        // A baked road whose grain coordinate the shader builds from world position (through per-view constants the files do not carry): lay the
        // grain out in world metres, `FM2_GRAIN_M` per repeat (default 2.5; `FM2_NO_PLANAR_GRAIN=1` keeps the old x32 guess).
        let planar_grain: Option<Vec<[f32; 2]>> = (colour_uv.is_none()
            && sprite.is_none()
            && colours.first().is_some_and(|c| c.0 >= 0xFFFF_2000)
            && p.candidates.iter().any(|c| mask(c.0) && Some(c.0) != wear_id)
            && std::env::var_os("FM2_NO_PLANAR_GRAIN").is_none())
        .then(|| {
            let m: f32 = std::env::var("FM2_GRAIN_M").ok().and_then(|v| v.parse().ok()).unwrap_or(2.5);
            p.positions.iter().map(|q| [q[0] / m, q[2] / m]).collect()
        });
        let colour_uv = colour_uv.or(planar_grain.as_ref());
        let mask_uv = if blend { masks.first().and_then(|m| slot_uv(m.0)) } else { None };
        shader_uv_draws += colour_uv.is_some() as usize;
        // A baked road with no grain mask is one flat colour: its coordinate does not matter.
        let flat_road = colours.first().is_some_and(|c| c.0 >= 0xFFFF_2000) && !p.candidates.iter().any(|c| mask(c.0) && Some(c.0) != wear_id);
        // Runtime textures (PVS flags 1) of at most 32x32 are placeholders the game fills in at run time (reflection probes): no real colour.
        let placeholder = !colours.is_empty()
            && colours.iter().all(|c| {
                p.candidates.iter().any(|x| x.0 == c.0 && x.1 == 1)
                    && std::fs::read(dir.join("textures").join(format!("{:08x}.dds", c.0))).is_ok_and(|b| {
                        let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
                        u(12) * u(16) <= 32 * 32
                    })
            });
        if colour_uv.is_none() && sprite.is_none() && !colours.is_empty() && !flat_road && !placeholder {
            colour_fallback += 1;
            if std::env::var_os("FM2_UV_FALLBACK").is_some() {
                let slots: Vec<usize> = colours.iter().flat_map(|c| p.candidates.iter().enumerate().filter(|(_, x)| x.0 == c.0).filter_map(|(k, _)| p.cand_slots.get(k).copied()).collect::<Vec<_>>()).collect();
                let why = if slots.is_empty() { if sprite.is_some() { "sprite" } else if dark_ids[pi].is_some() { "wear blend over a baked road / asphalt" } else { "other synthetic colour" } } else if p.resolved.is_empty() { "nothing resolved" } else { "other samplers resolved, not the colour" };
                if std::env::var("FM2_UV_MODEL").ok().and_then(|v| v.parse::<usize>().ok()) == Some(p.model) {
                    println!("[fm2] uv model {}: colour slots {slots:?} resolved {:?} cand_slots {:?} cands {:?}", p.model, p.resolved.keys().collect::<Vec<_>>(), p.cand_slots, p.candidates);
                }
                *fallback_why.entry(format!("{why}; model {}", if slots.is_empty() { 0 } else { p.model })).or_default() += 1;
            }
        }
        b.uvs[0].extend_from_slice(colour_uv.unwrap_or(if sprite.is_some() { &p.raw } else { &p.uv }));
        if blend {
            b.uvs[1].extend_from_slice(mask_uv.or(p.mask_raw.as_ref()).unwrap_or(&p.raw));
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
    if !fallback_why.is_empty() {
        let mut v: Vec<_> = fallback_why.iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(*x.1));
        for (k, n) in v.iter().take(25) {
            println!("[fm2] uv fallback {n}: {k}");
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
        "[fm2] {}: {shader_uv_draws} draws with shader-derived colour uv ({colour_fallback} coloured draws still on the x32 guess), {flipped} draws rewound, {textured} draws ({blends} terrain blends, {bare} lightmap-only: flat grey), {} materials, {} textures, {} tiles",
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

/// Cuts the sub-rectangle `[su, sv, ou, ov]` (fractions of the texture) out of a BC1 / BC2 / BC3 / RGBA8 DDS into a new texture.
fn crop_atlas(path: &Path, rect: [f32; 4]) -> Option<crate::textures::Converted> {
    let dds = std::fs::read(path).ok()?;
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
    let (h, w) = (u(12), u(16));
    let mut rgba = vec![0u8; w * h * 4];
    match u(128) {
        71 | 74 | 77 => {
            let fmt = match u(128) {
                71 => texpresso::Format::Bc1,
                74 => texpresso::Format::Bc2,
                _ => texpresso::Format::Bc3,
            };
            fmt.decompress(dds.get(148..148 + fmt.compressed_size(w, h))?, w, h, &mut rgba);
        }
        28 => rgba.copy_from_slice(dds.get(148..148 + w * h * 4)?),
        _ => return None,
    }
    let x0 = ((rect[2].clamp(0.0, 1.0)) * w as f32).round() as usize;
    let y0 = ((rect[3].clamp(0.0, 1.0)) * h as f32).round() as usize;
    let cw = ((rect[0] * w as f32).round() as usize).clamp(1, w.saturating_sub(x0).max(1));
    let ch = ((rect[1] * h as f32).round() as usize).clamp(1, h.saturating_sub(y0).max(1));
    if cw < 2 || ch < 2 || (cw == w && ch == h) {
        return None;
    }
    let mut out = Vec::with_capacity(cw * ch * 4);
    for y in 0..ch {
        let row = (y0 + y).min(h - 1) * w;
        for x in 0..cw {
            let o = (row + (x0 + x).min(w - 1)) * 4;
            out.extend_from_slice(&rgba[o..o + 4]);
        }
    }
    crate::textures::rgba_to_dds(out, cw as u32, ch as u32).ok()
}

/// Whether a BC1 / BC2 / BC3 DDS holds a greyscale image (R = G = B within a few levels on average).
fn is_grey_texture(path: &Path) -> bool {
    let Ok(dds) = std::fs::read(path) else { return false };
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
    let fmt = match u(128) {
        71 => texpresso::Format::Bc1,
        74 => texpresso::Format::Bc2,
        77 => texpresso::Format::Bc3,
        _ => return false,
    };
    let (h, w) = (u(12), u(16));
    let Some(data) = dds.get(148..148 + fmt.compressed_size(w, h)) else { return false };
    let mut rgba = vec![0u8; w * h * 4];
    fmt.decompress(data, w, h, &mut rgba);
    let d: f32 = rgba.chunks_exact(4).map(|p| (p[0] as f32 - p[1] as f32).abs() + (p[1] as f32 - p[2] as f32).abs()).sum::<f32>() / (w * h) as f32;
    d < 4.0
}


/// A DDS (DX10 header) of the formats the importer writes, decoded to RGBA8. One-channel BC4 is replicated into rgb as the sampler hands it over.
fn decode_rgba(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let b = std::fs::read(path).ok()?;
    let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
    let (h, w, fmt) = (u(12), u(16), u(128));
    let mut rgba = vec![0u8; w * h * 4];
    let f = match fmt {
        71 => texpresso::Format::Bc1,
        74 => texpresso::Format::Bc2,
        77 => texpresso::Format::Bc3,
        80 => texpresso::Format::Bc4,
        83 => texpresso::Format::Bc5,
        28 => {
            rgba.copy_from_slice(b.get(148..148 + w * h * 4)?);
            return Some((w, h, rgba));
        }
        _ => return None,
    };
    f.decompress(b.get(148..148 + f.compressed_size(w, h))?, w, h, &mut rgba);
    if fmt == 80 {
        // `FM2_BC4_SRGB=1`: single-channel BC4 textures are sRGB-encoded on the console (STATUS.md: with the lit lightmap texels the shader then matches
        // Xenia on Nurburgring, Tsukuba, Test Track and Sunset). Marked with alpha 250 and decoded per texel in `sample_rgba`.
        let marker = 250;
        for p in rgba.chunks_exact_mut(4) {
            (p[1], p[2], p[3]) = (p[0], p[0], marker);
        }
    }
    Some((w, h, rgba))
}

/// Bilinear, wrapping, mip 0.
fn sample_rgba(t: &(usize, usize, Vec<u8>), uv: [f32; 2], dec_bc4: bool) -> [f32; 4] {
    let (w, h, d) = (t.0, t.1, &t.2);
    let (fx, fy) = (uv[0] * w as f32 - 0.5, uv[1] * h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let px = |x: f32, y: f32| {
        let (xi, yi) = ((x as i64).rem_euclid(w as i64) as usize, (y as i64).rem_euclid(h as i64) as usize);
        let p = &d[(yi * w + xi) * 4..][..4];
        if p[3] == 250 && dec_bc4 {
            let dec = |b: u8| {
                let c = b as f32 / 255.0;
                if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
            };
            return [dec(p[0]), dec(p[1]), dec(p[2]), 1.0];
        }
        let a = if p[3] == 250 { 1.0 } else { p[3] as f32 / 255.0 };
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, a]
    };
    let (a, b, c, e) = (px(x0, y0), px(x0 + 1.0, y0), px(x0, y0 + 1.0), px(x0 + 1.0, y0 + 1.0));
    std::array::from_fn(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + e[k] * tx) * ty)
}

thread_local! {
    /// Texture files of runtime placeholder slots (1x1 black / tiny runtime textures): the interpreter samples them as white, as the slot's neutral value.
    static WHITE_TEX: std::cell::RefCell<std::collections::HashSet<std::path::PathBuf>> = Default::default();
}

/// The colour a draw's own pixel shader produces with neutral lighting: the lightmap blend and the sun term off, fog off, the sampled textures
/// and the draw's constants as they are. Run on up to 24 of the draw's vertices (their resolved texture coordinates feed the samplers) and
/// averaged. None when the shader or its inputs are not available (or it uses something the interpreter does not know).
fn shader_albedo(p: &Pending, texdir: &Path, cache: &mut HashMap<std::path::PathBuf, Option<(usize, usize, Vec<u8>)>>) -> Option<[f32; 3]> {
    shader_eval(p, texdir, cache, &[], &[], 24)
}

/// `shader_albedo` with constant overrides and interpolator-register overrides (applied after the neutral defaults), averaged over up to `samples` vertices.
fn shader_eval(p: &Pending, texdir: &Path, cache: &mut HashMap<std::path::PathBuf, Option<(usize, usize, Vec<u8>)>>, const_over: &[(u32, [f32; 4])], reg_over: &[(usize, [f32; 4])], samples: usize) -> Option<[f32; 3]> {
    use fh1_shaders::ps_cpu::{fog_register, Machine};
    let ps = p.ps_run.as_ref()?;
    let n = p.positions.len();
    if n == 0 || ps.words.is_empty() {
        return None;
    }
    // Samplers decoded as sRGB (the family's table entry; with FM2_BC4_SRGB on and no entry: the BC4 rule), and placeholder samplers read white.
    let gmask = family_gamma(p).unwrap_or(if bc4_srgb_on() { BC4 } else { 0 });
    // Item position -> its texture file, and the textures themselves.
    let mut tex_of_item: HashMap<usize, std::path::PathBuf> = HashMap::new();
    for (k, c) in p.candidates.iter().enumerate() {
        if let Some(&slot) = p.cand_slots.get(k) {
            tex_of_item.insert(slot, texdir.join(format!("{:08x}.dds", c.0)));
        }
    }
    for path in tex_of_item.values() {
        cache.entry(path.clone()).or_insert_with(|| decode_rgba(path));
    }
    let mut consts: HashMap<u32, [f32; 4]> = HashMap::new();
    let nl = ps.literals.len() as u32;
    for (k, l) in ps.literals.iter().enumerate() {
        consts.insert(256 - nl + k as u32, *l);
    }
    for (r, v) in &p.consts {
        consts.insert(*r, *v);
    }
    for (r, v) in [(184, [1.0; 4]), (185, [1.0; 4]), (102, [0.0, 0.0, 0.0, 1.0]), (104, [0.0, 0.0, 0.0, 1.0]), (114, [0.0; 4]), (101, [0.0, 0.7, 0.7, 0.0]), (105, [1.0, 1.0, 1.0, 0.0])] {
        consts.insert(r, v);
    }
    for (r, v) in const_over {
        consts.insert(*r, *v);
    }
    let fog = fog_register(&ps.words)?;
    let step = (n / samples.max(1)).max(1);
    let (mut sum, mut cnt) = ([0.0f32; 3], 0);
    for v in (0..n).step_by(step) {
        let sampler_tex: HashMap<u32, &(usize, usize, Vec<u8>)> = ps
            .item_sampler
            .iter()
            .filter_map(|(item, s)| Some((*s, cache.get(tex_of_item.get(item)?)?.as_ref()?)))
            .collect();
        let white: std::collections::HashSet<u32> = WHITE_TEX.with(|w| {
            let w = w.borrow();
            ps.item_sampler.iter().filter(|(item, _)| tex_of_item.get(item).is_some_and(|pth| w.contains(pth))).map(|(_, s)| *s).collect()
        });
        let sample = |s: u32, uv: [f32; 2]| {
            if white.contains(&s) {
                return [1.0; 4];
            }
            let Some(t) = sampler_tex.get(&s) else { return [0.0; 4] };
            let mut v = sample_rgba(t, uv, gmask & BC4 != 0);
            if s < 31 && gmask >> s & 1 == 1 {
                for c in &mut v[..3] {
                    *c = if *c <= 0.04045 { *c / 12.92 } else { ((*c + 0.055) / 1.055).powf(2.4) };
                }
            }
            v
        };
        let mut m = Machine::new(consts.clone(), HashMap::new(), &sample);
        m.r[2] = [0.0, 0.7, 0.7, 0.0];
        m.r[3] = [0.0, 1.0, 0.0, 0.0];
        for (r, v) in reg_over {
            m.r[*r] = *v;
        }
        for (item, s) in &ps.item_sampler {
            if let (Some(reg), Some(uvs)) = (ps.sampler_reg.get(s), p.resolved.get(item)) {
                if let Some(u) = uvs.get(v) {
                    m.r[*reg as usize][0] = u[0];
                    m.r[*reg as usize][1] = u[1];
                }
            }
        }
        m.r[fog as usize][0] = 1.0;
        m.run(&ps.words);
        if !m.unsupported.is_empty() {
            return None;
        }
        if m.killed {
            continue;
        }
        for k in 0..3 {
            sum[k] += m.out[k];
        }
        cnt += 1;
    }
    (cnt > 0).then(|| sum.map(|x| (x / cnt as f32).clamp(0.0, 4.0)))
}

/// The game's specular term of a draw's pixel shader per unit of `GlobalSpecularLevel` and light colour, with the highlight at its peak (half vector
/// along the normal): the output rise when `c102` and `c104` are 1 with `c0` as the draw sets it. The view-vector and normal interpolator registers are
/// not known per shader, so every pair of registers that carry no texture coordinate is tried and the largest rise wins (a wrong pair gives about 0).
/// `exp` (the material exponent) only matters off the peak. None when the shader cannot be run or has no highlight.
fn shader_spec_k(p: &Pending, texdir: &Path, cache: &mut HashMap<std::path::PathBuf, Option<(usize, usize, Vec<u8>)>>) -> Option<f32> {
    let ps = p.ps_run.as_ref()?;
    let (exp_reg, exp) = ps.spec_exp?;
    let uvs: std::collections::BTreeSet<u32> = ps.sampler_reg.values().copied().collect();
    let free: Vec<usize> = (0..8usize).filter(|r| !uvs.contains(&(*r as u32))).collect();
    let off_c = [(102u32, [0.0, 0.0, 0.0, 1.0]), (104u32, [0.0, 0.0, 0.0, 1.0]), (exp_reg, [exp; 4])];
    let base = shader_eval(p, texdir, cache, &off_c, &[], 4)?;
    let on_c = [(102u32, [1.0, 1.0, 1.0, 1.0]), (104u32, [1.0, 1.0, 1.0, 1.0]), (101u32, [0.6, 0.8, 0.0, 0.0]), (exp_reg, [exp; 4])];
    let mut best = 0.0f32;
    for &v in &free {
        for &n in &free {
            if v != n {
                if let Some(o) = shader_eval(p, texdir, cache, &on_c, &[(v, [6.0, -8.0, 0.0, 0.0]), (n, [0.0, 1.0, 0.0, 0.0])], 4) {
                    best = best.max(o[1] - base[1]);
                }
            }
        }
    }
    (best > 1e-3 && best < 0.7 && (3.0..40.0).contains(&exp)).then_some(best)
}
