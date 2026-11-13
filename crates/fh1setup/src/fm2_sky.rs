//! FM2 sky: `<track>out.sky.rmb.bin` is a dome drawn in several passes (one texture each), a sun disc and a glow quad. Its textures are the
//! PVS texture records no numbered model lists (item k of the sky model = the k-th such record). Writes `scenery/sky/textures/*.dds` and
//! `scenery/sky.json` (the draws' geometry with raw uvs, for the engine's sky layer).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::Result;
use fh1_formats::{fm2, zip::{Archive, Entry}};
use serde_json::json;
use texpresso::{Algorithm, Format, Params};

pub fn import(ar: &mut Archive<std::fs::File>, by_name: &HashMap<String, Entry>, pvs: &fm2::Pvs, dir: &Path, gsl: Option<f32>, tune_sky_gain: Option<f32>) -> Result<()> {
    let Some(e) = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".sky.rmb.bin")).cloned() else { return Ok(()) };
    let raw = ar.read(&e)?;
    let model = fm2::parse_model(&raw)?;
    let used: std::collections::HashSet<u32> = pvs.models.iter().flatten().copied().collect();
    let free: Vec<usize> = (0..pvs.textures.len()).filter(|i| !used.contains(&(*i as u32))).collect();
    let mut wanted = BTreeMap::new();
    for &i in &free {
        wanted.insert(pvs.textures[i].file_id, pvs.textures[i].flags & 4 != 0);
    }
    let tex_dir = dir.join("sky/textures");
    std::fs::create_dir_all(&tex_dir)?;
    let mut done = crate::scenery::convert_textures(ar, by_name, &wanted, &tex_dir)?;
    // PVS "runtime" textures (flags 1) live in the track's `.stx.bin`, at the entry named by the record's `object` (Tsukuba's and Silverstone's
    // zenith cap `shader_sky_cap`).
    if free.iter().any(|&i| pvs.textures[i].flags == 1 && !done.contains_key(&pvs.textures[i].file_id)) {
        if let Some((d, ents)) = crate::scenery::read_stx(ar, by_name) {
            for &i in &free {
                let t = pvs.textures[i];
                if t.flags != 1 || done.contains_key(&t.file_id) {
                    continue;
                }
                let Some(&ct) = ents.get(t.object as usize) else { continue };
                // A 1 x 1 entry is the placeholder of a texture the game fills at run time (Tsukuba's cap): nothing to draw.
                if ct.width * ct.height <= 1 {
                    continue;
                }
                match crate::textures::caff_texture_to_dds(&d, ct) {
                    Ok(c) => {
                        std::fs::write(tex_dir.join(format!("{:08x}.dds", t.file_id)), &c.dds)?;
                        done.insert(t.file_id, (c.alpha, c.word));
                    }
                    Err(e) => println!("[fm2] sky stx texture {} (_0x{:08X}): {e}", t.object, t.file_id),
                }
            }
        }
    }
    // Textures whose format word ends in 0x86 hold blue and red the other way round (the sky shaders swizzle them; Silverstone and Suzuka),
    // those ending in 0x54 do not (Road Atlanta): swap the first kind so both read as the sky they are.
    for (&id, &(_, word)) in &done {
        if word & 0xff == 0x86 {
            swap_red_blue(&tex_dir.join(format!("{id:08x}.dds")));
        }
    }
    // FM2_SKY_DBG=1: each sky texture's game format word (bits 8-13: 0x3F = colour, 0x01 = mask/normal), which decides whether the
    // console decoded it as sRGB. The engine loads every sky texture as sRGB today; this is the data that says if that is right.
    if std::env::var_os("FM2_SKY_DBG").is_some() {
        for (&id, &(alpha, word)) in &done {
            println!("[fm2 sky] texture {id:08x} alpha {alpha} word 0x{word:08X} (bits 8-13 0x{:02X})", (word >> 8) & 0x3F);
        }
    }
    if std::env::var_os("FM2_DUMP_SKY_PS").is_some() {
        crate::fm2_uv::dump_shaders(&raw, &model);
        let uvs = crate::fm2_uv::resolve(&raw, &model, &|_| crate::fm2_uv::ItemKind::Other);
        // Container header words (MODLOG 337): magic, virtual_size, physical_size, ctab, defs, shader_off. A shader
        // whose disassembly comes out as nothing usually has a bogus physical_size or shader_off here.
        // NOTE: `section` returns the section SIZE, and `.data` starts at 0x190 + size(.gpu) -- not at 0x190 + size(.data).
        if let (Some(g), Some(_dt)) = (crate::fm2_uv::section(&raw, b".gpu"), crate::fm2_uv::section(&raw, b".data")) {
            let base = 0x190 + g;
            for (i, d) in model.draws.iter().enumerate() {
                let Some(off) = d.ps_container else { continue };
                let w = |o: usize| raw.get(base + o..base + o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()));
                let hdr: Vec<String> = (0..7).map(|k| format!("{:#010x}", w(off + k * 4).unwrap_or(0))).collect();
                // Program header at `shader_off` (word 6): physical_offset, code_size, ?, fieldC.
                let sh_off = w(off + 6 * 4).unwrap_or(0) as usize;
                let prog: Vec<String> = (0..6).map(|k| format!("{:#010x}", w(off + sh_off + k * 4).unwrap_or(0))).collect();
                // Two candidate microcode bases: `virtual_size + physical_offset` (what parse uses) and `physical_offset`
                // itself. Print the first dwords of each so a wrong one is visible as zeros.
                let virt = w(off + 4).unwrap_or(0) as usize;
                let po = w(off + sh_off).unwrap_or(0) as usize;
                let dump8 = |base: usize| -> String {
                    (0..8).map(|k| format!("{:08x}", w(off + base + k * 4).unwrap_or(0))).collect::<Vec<_>>().join(" ")
                };
                println!("[fm2 sky] draw {i}: container header at {off:#x}: {}  program @{sh_off:#x}: {}", hdr.join(" "), prog.join(" "));
                println!("[fm2 sky] draw {i}:   microcode @virt+po ({:#x}): {}", virt + po, dump8(virt + po));
                println!("[fm2 sky] draw {i}:   microcode @po      ({:#x}): {}", po, dump8(po));
            }
        }
        for (i, u) in uvs.iter().enumerate() {
            if let Some(ps) = &u.ps {
                println!("[fm2 sky] draw {i}: consts {:?}
{}", ps.consts, ps.text);
            } else {
                println!("[fm2 sky] draw {i}: no pixel shader resolved");
            }
        }
    }
    // Plan v10 P3 (MODLOG 328): find the sky material block. The draw sets registers < 100; the shader's named
    // constants (c_skyMultiplier at 188, c_globalSpecularLevel at 104) live at >= 100 and come from the material
    // record, which this importer has never read. Locate the block by searching `.data` for one of the draw's own
    // non-zero constants and print the float4s around it, so the >= 100 registers can be read off.
    if std::env::var_os("FM2_SKY_MAT").is_some() {
        // `.data` starts at 0x190 + size(.gpu) (section() returns a size, not an offset) -- corrected in MODLOG 337.
        let gt = crate::fm2_uv::section(&raw, b".gpu");
        let uvs = crate::fm2_uv::resolve(&raw, &model, &|_| crate::fm2_uv::ItemKind::Other);
        if let Some(data) = gt.and_then(|g| raw.get(0x190 + g..)) {
            for (i, u) in uvs.iter().enumerate() {
                let Some(ps) = &u.ps else { continue };
                let Some((k, v)) = ps.consts.iter().find(|(_, v)| v.iter().any(|x| *x != 0.0)) else { continue };
                let mut pat = Vec::with_capacity(16);
                for x in v.iter() {
                    pat.extend_from_slice(&x.to_be_bytes());
                }
                let at = (0..data.len().saturating_sub(16)).step_by(4).find(|&o| data[o..o + 16] == pat[..]);
                let Some(o) = at else { continue };
                let vec_at = |p: usize| -> Option<[f32; 4]> {
                    let w = |j: usize| data.get(p + 4 * j..p + 4 * j + 4).map(|b| f32::from_be_bytes(b.try_into().unwrap()));
                    Some([w(0)?, w(1)?, w(2)?, w(3)?])
                };
                // `fm2_uv::spec_exponent` reads consecutive registers at a 0x14 (20-byte) stride, so the block stores
                // one extra word per register. Print 32 registers from `k - 8` at that stride, labelled.
                let base = o.saturating_sub(8 * 0x14);
                let row: Vec<String> = (0..32)
                    .filter_map(|n| vec_at(base + n * 0x14).map(|v| format!("c{}={:.2?}", (*k as i64 - 8 + n as i64).max(0), v)))
                    .collect();
                println!("[fm2 sky] draw {i}: block at {o:#x} (match c{k}) -- {}", row.join(" "));
            }
        }
    }
    let mut out = Vec::new();
    for d in &model.draws {
        let (uv0, _) = d.uv_offsets();
        let uv: Vec<[f32; 2]> = d.uv_at(uv0);
        let files: Vec<u32> = d.items.iter().filter_map(|&it| free.get(it as usize).map(|&i| pvs.textures[i].file_id)).collect();
        // The game's own gamma flag for each of those textures (format word bits 8-13: 0x3F = colour, decoded as sRGB; anything
        // else, e.g. 0x01 = mask/normal, linear). The engine used to upload every sky texture as sRGB (MODLOG 158/159).
        let gamma: Vec<bool> = files.iter().map(|f| done.get(f).is_some_and(|&(_, w)| (w >> 8) & 0x3F == 0x3F)).collect();
        // The pixel-shader constants the model sets for the draw (a flare / sun draw keeps its colour there).
        let constants: BTreeMap<String, [f32; 4]> = d.constants.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        out.push(json!({
            "stride": d.stride,
            "positions": d.positions,
            "uv": uv,
            "indices": d.indices,
            "textures": files,
            "gamma": gamma,
            "constants": constants,
        }));
    }
    // The sun glow card (a 4-vertex quad at the sun): its texture is a flare star, one channel (BC4) or the G channel of an 8_8 pair (BC3 here).
    // Written as a grey PNG for the engine (`sky/glow.png`); the draw's record says so.
    for (i, d) in model.draws.iter().enumerate() {
        if d.positions.len() != 4 {
            continue;
        }
        let Some(&file) = out[i]["textures"].as_array().and_then(|t| t.first()).and_then(|t| t.as_u64()).map(|t| t as u32).as_ref() else { continue };
        if let Some((w, h, grey)) = glow_grey(&tex_dir.join(format!("{file:08x}.dds"))) {
            let rgba: Vec<u8> = grey.iter().flat_map(|&g| [g, g, g, 255]).collect();
            crate::model::write_png(&dir.join("sky/glow.png"), w as u32, h as u32, &rgba)?;
            out[i]["glow"] = json!("sky/glow.png");
        }
    }
    // The game's sky formula, baked into each dome texture (MODLOG 337). The pixel shader computes
    //   out.rgb = tex.rgb * (1 + alpha * m * (1 - w))
    // with `m` = c188.x (c_skyMultiplier, the per-draw constant in `constants` slot 1) and `w` = c104.w
    // (c_globalSpecularLevel). Our renderer draws the colour alone, so the alpha term never reaches the screen;
    // baking it here keeps the engine path unchanged and stays inside the game's own numbers.
    // The value is the track's own `GlobalSpecularLevel` (TrackSettings.xml), which is what the shader's
    // `c104` register is named for; the engine does not pass it, so it is applied here. `w = 1` is the identity,
    // which is also the fallback when the track has no such setting. `FH1_FM2_SKY_W` overrides it for sweeps and
    // `FH1_FM2_SKY_BAKE=0` turns the whole step off.
    //
    // MEASURED (MODLOG 341): at the track's own level, Nurburgring's sky band goes 26.33 -> 24.66 and the frame
    // 34.81 -> 33.90; Mugello 40.50 -> 39.28 and 31.06 -> 30.60. The ten other tracks cannot respond at all --
    // their drawn colour textures carry `m = 0`, or the only `m > 0` draw is the zenith cap, whose texture is BC4
    // (a mask, not a colour image) and is skipped here exactly as the engine skips it.
    // OFF by default: the mechanism is verified (MODLOG 341: applying this to the imported textures moves the
    // twelve-track mean 30.519 -> 30.410), but THIS implementation is not -- the importer's own texture rewrite
    // produces a different level 0 from the verified standalone tool (mean RGB 109 vs 133 where the tool gives
    // 133/150/158), and the resulting capture is worse than shipped (34.34 vs 34.81 frame). Do not turn this on
    // until the two paths agree. `FH1_FM2_SKY_BAKE=1` enables it for that comparison.
    if std::env::var("FH1_FM2_SKY_BAKE").is_ok_and(|v| v == "1") {
        let w: f32 = std::env::var("FH1_FM2_SKY_W").ok().and_then(|v| v.parse().ok()).or(gsl).unwrap_or(1.0);
        let mut baked = 0usize;
        let mut seen: Vec<u32> = Vec::new();
        for d in &model.draws {
            // The model's own c188.x when it carries one; otherwise the track's `skyGain`, which IS the runtime
            // c188.x (MODLOG 456: exact on the three dumped tracks) and which the model does not carry on ten of
            // twelve -- that is why the old bake could only respond on two tracks.
            let m = d.constants.get(&1).map(|v| v[0]).filter(|&v| v > 0.0).or(tune_sky_gain).unwrap_or(0.0);
            if m <= 0.0 {
                continue;
            }
            let Some(&first) = d.items.first().and_then(|&it| free.get(it as usize)) else { continue };
            let id = pvs.textures[first].file_id;
            // One texture can be drawn by several passes; baking it once per draw would compound the gain.
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let p = tex_dir.join(format!("{id:08x}.dds"));
            let Ok(mut dds) = std::fs::read(&p) else { continue };
            let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
            // The data starts after the 128-byte legacy header, plus the DX10 extension when present -- the old code
            // assumed 148 unconditionally (MODLOG 461).
            let off = if dds.get(84..88) == Some(b"DX10") { 148 } else { 128 };
            let (h, wpx) = (u(12), u(16));
            let fmt = match u(128) {
                77 => Format::Bc3,
                71 => Format::Bc1,
                _ => continue,
            };
            let len = fmt.compressed_size(wpx, h);
            let Some(src) = dds.get(off..off + len).map(|s| s.to_vec()) else { continue };
            let mut rgba = vec![0u8; wpx * h * 4];
            fmt.decompress(&src, wpx, h, &mut rgba);
            let k = 1.0 - w;
            for px in rgba.chunks_exact_mut(4) {
                let f = 1.0 + (px[3] as f32 / 255.0) * m * k;
                for c in 0..3 {
                    // The gain is a light-transport value, so it applies in LINEAR light: the console fetches the
                    // sRGB texture into linear, does this arithmetic there, and the sRGB framebuffer converts back.
                    px[c] = linear_to_srgb8(srgb8_to_linear(px[c]) * f);
                }
            }
            // Re-encode LEVEL 0 IN PLACE, exactly as the verified standalone tool does (`examples/sky_bake.rs`):
            // the header and every stored mip level are copied through byte for byte. The old path re-encoded the
            // whole file with `rgba_to_dds`, which dropped the mips and produced a different level (mean 109
            // against the tool's 133) -- the mismatch that kept this bake off (MODLOG 459/461).
            let params = Params { algorithm: Algorithm::RangeFit, ..Default::default() };
            let mut out = vec![0u8; len];
            fmt.compress(&rgba, wpx, h, params, &mut out);
            dds[off..off + len].copy_from_slice(&out);
            if std::fs::write(&p, &dds).is_ok() {
                baked += 1;
            }
        }
        if baked > 0 {
            println!("[fm2] sky: alpha term baked into {baked} dome textures (w = {w:.3})");
        }
    }
    std::fs::write(dir.join("sky.json"), serde_json::to_vec(&json!({ "draws": out }))?)?;
    println!("[fm2] sky: {} draws, {} textures", out.len(), wanted.len());
    Ok(())
}

/// The glow card's intensity picture: level 0 of a BC4 DDS (the one channel) or the green channel of a BC3 one (the 8_8 texture's second channel).
fn glow_grey(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let dds = std::fs::read(path).ok()?;
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
    let (fmt, channel) = match u(128) {
        80 => (texpresso::Format::Bc4, 0),
        77 => (texpresso::Format::Bc3, 1),
        _ => return None,
    };
    let (h, w) = (u(12), u(16));
    let data = dds.get(148..148 + fmt.compressed_size(w, h))?;
    let mut rgba = vec![0u8; w * h * 4];
    fmt.decompress(data, w, h, &mut rgba);
    Some((w, h, rgba.chunks_exact(4).map(|p| p[channel]).collect()))
}

/// Rewrites a BC1 / BC2 / BC3 DDS with its red and blue channels swapped (other formats, such as BC4 masks, are left alone).
fn swap_red_blue(path: &Path) {
    let Ok(dds) = std::fs::read(path) else { return };
    let u = |o: usize| u32::from_le_bytes(dds.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
    let fmt = match u(128) {
        71 => texpresso::Format::Bc1,
        74 => texpresso::Format::Bc2,
        77 => texpresso::Format::Bc3,
        _ => return,
    };
    let (h, w) = (u(12), u(16));
    let Some(data) = dds.get(148..148 + fmt.compressed_size(w, h)) else { return };
    let mut rgba = vec![0u8; w * h * 4];
    fmt.decompress(data, w, h, &mut rgba);
    for p in rgba.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    if let Ok(c) = crate::textures::rgba_to_dds(rgba, w as u32, h as u32) {
        let _ = std::fs::write(path, &c.dds);
    }
}

/// An 8-bit sRGB value as linear light (the curve the GPU's sRGB fetch applies).
fn srgb8_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear light back to an 8-bit sRGB value, rounded.
fn linear_to_srgb8(lin: f32) -> u8 {
    let c = lin.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A track's `GlobalSpecularLevel` out of its `TrackSettings.xml` -- the setting the sky shader's `c104` register
/// is named for (`c_globalSpecularLevel`). `None` when the track has no such setting, in which case the alpha term
/// is the identity and nothing is baked.
pub fn global_specular_level(xml: &str) -> Option<f32> {
    // The shader's `w` is `c104.W` (MODLOG 397: captured as 1 on one sky draw and 0 on another, never 1.2), so this
    // must read the **alpha** component. It used to read `g=`, which is 1.2 on Mugello and 1.2 on New York -- giving
    // k = 1 - w = -0.2 and DARKENING those skies where the game leaves them alone (MODLOG 420).
    let i = xml.find("<GlobalSpecularLevel")?;
    let seg = &xml[i..];
    let a = seg.find("a=\"")? + 3;
    seg[a..].split('"').next()?.parse().ok()
}

/// A track's `skyGain` out of its `TrackSettings.xml`. **This is the shader's `c188.x` exactly** (MODLOG 456: 0.40 /
/// 0.00 / 2.25 on the three tracks with a verified runtime dump) -- the `c_skyMultiplier` the alpha-term bake needs.
pub fn sky_gain(xml: &str) -> Option<f32> {
    let i = xml.find("<skyGain")?;
    let seg = &xml[i..];
    let a = seg.find("value=\"")? + 7;
    seg[a..].split('"').next()?.parse().ok()
}
