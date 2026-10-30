//! FM2 sky: `<track>out.sky.rmb.bin` is a dome drawn in several passes (one texture each), a sun disc and a glow quad. Its textures are the
//! PVS texture records no numbered model lists (item k of the sky model = the k-th such record). Writes `scenery/sky/textures/*.dds` and
//! `scenery/sky.json` (the draws' geometry with raw uvs, for the engine's sky layer).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::Result;
use fh1_formats::{fm2, zip::{Archive, Entry}};
use serde_json::json;

pub fn import(ar: &mut Archive<std::fs::File>, by_name: &HashMap<String, Entry>, pvs: &fm2::Pvs, dir: &Path) -> Result<()> {
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
    if std::env::var_os("FM2_DUMP_SKY_PS").is_some() {
        crate::fm2_uv::dump_shaders(&raw, &model);
        let uvs = crate::fm2_uv::resolve(&raw, &model, &|_| crate::fm2_uv::ItemKind::Other);
        for (i, u) in uvs.iter().enumerate() {
            if let Some(ps) = &u.ps {
                println!("[fm2 sky] draw {i}: consts {:?}
{}", ps.consts, ps.text);
            } else {
                println!("[fm2 sky] draw {i}: no pixel shader resolved");
            }
        }
    }
    let mut out = Vec::new();
    for d in &model.draws {
        let (uv0, _) = d.uv_offsets();
        let uv: Vec<[f32; 2]> = d.uv_at(uv0);
        let files: Vec<u32> = d.items.iter().filter_map(|&it| free.get(it as usize).map(|&i| pvs.textures[i].file_id)).collect();
        // The pixel-shader constants the model sets for the draw (a flare / sun draw keeps its colour there).
        let constants: BTreeMap<String, [f32; 4]> = d.constants.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        out.push(json!({
            "stride": d.stride,
            "positions": d.positions,
            "uv": uv,
            "indices": d.indices,
            "textures": files,
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
