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
    let done = crate::scenery::convert_textures(ar, by_name, &wanted, &tex_dir)?;
    // Textures whose format word ends in 0x86 hold blue and red the other way round (the sky shaders swizzle them; Silverstone and Suzuka),
    // those ending in 0x54 do not (Road Atlanta): swap the first kind so both read as the sky they are.
    for (&id, &(_, word)) in &done {
        if word & 0xff == 0x86 {
            swap_red_blue(&tex_dir.join(format!("{id:08x}.dds")));
        }
    }
    let mut out = Vec::new();
    for d in &model.draws {
        let (uv0, _) = d.uv_offsets();
        let uv: Vec<[f32; 2]> = d.uv_at(uv0);
        let files: Vec<u32> = d.items.iter().filter_map(|&it| free.get(it as usize).map(|&i| pvs.textures[i].file_id)).collect();
        out.push(json!({
            "stride": d.stride,
            "positions": d.positions,
            "uv": uv,
            "indices": d.indices,
            "textures": files,
        }));
    }
    std::fs::write(dir.join("sky.json"), serde_json::to_vec(&json!({ "draws": out }))?)?;
    println!("[fm2] sky: {} draws, {} textures", out.len(), wanted.len());
    Ok(())
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
