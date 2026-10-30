//! FM2 spectators: `Media/Spectators.zip` (models, animations, textures, the sprite atlas) once under `imported/fm2/crowd/`, and per map the placement
//! from `<track>_track_NN.crowd` as `crowd/crowds.bin` + `index.json`, the files the engine's crowd system reads for Colorado (fh1setup `crowd.rs`).
//!
//! `.crowd` (big-endian, checked on Tsukuba: 52 groups cover 2919 spectators exactly): `u32 version (1), u32 groups, u32 spectators`; per group 0x34 bytes
//! (bbox min, bbox max, centre, f32 radius, u32 0, u32 first spectator, u32 count); then per spectator 32 bytes: f32 x, y, z, f32 sin(heading), u32 0,
//! f32 cos(heading), u8 crowd class (the `<crowdclass>` order of `spectators.xml`), u8 variation, u8, u8, u32, u32.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fh1_formats::{crowd, zip::Archive};
use serde_json::{json, Value};

fn be_f(d: &[u8], o: usize) -> f32 {
    f32::from_be_bytes(d[o..o + 4].try_into().unwrap())
}
fn be_u(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(d[o..o + 4].try_into().unwrap())
}

/// The shared assets' description: the atlas order of the sprite textures, the class table and the atlas (the shared parts of every map's index.json).
pub(crate) struct Shared {
    models: Vec<Value>,
    classes: Vec<Value>,
    atlas: Value,
}

pub(crate) fn shared(disc: &Path, out: &Path) -> Result<Shared> {
    let mut spec = Archive::open(disc.join("media/Spectators.zip"))?;
    let read = |ar: &mut Archive<std::fs::File>, name: &str| -> Result<Vec<u8>> {
        let e = ar.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)).cloned().with_context(|| format!("Spectators.zip: {name}"))?;
        Ok(ar.read(&e)?)
    };
    let (aw, ah, rgba) = crowd::decode_dxt1_alpha(&read(&mut spec, "sprites.xds")?)?;
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("sprites.dds"), crate::textures::rgba_to_dds(rgba, aw, ah)?.dds)?;
    for sub in ["models", "anims", "textures"] {
        std::fs::create_dir_all(out.join(sub))?;
    }
    let (mut models, mut anims) = (0, 0);
    for e in spec.entries.clone() {
        let name = e.name.replace('\\', "/");
        let file = name.rsplit('/').next().unwrap_or(&name).to_string();
        if name.ends_with(".skinbin") {
            let d = spec.read(&e)?;
            crowd::parse_skinbin(&d).with_context(|| name.clone())?;
            std::fs::write(out.join("models").join(&file), d)?;
            models += 1;
        } else if name.ends_with(".anim.bin") {
            std::fs::write(out.join("anims").join(&file), spec.read(&e)?)?;
            anims += 1;
        } else if let Some(stem) = file.strip_suffix(".dds.xds").filter(|_| name.starts_with("textures/")) {
            let (w, h, rgba) = crowd::decode_dxt1_alpha(&spec.read(&e)?).with_context(|| name.clone())?;
            std::fs::write(out.join("textures").join(format!("{stem}.dds")), crate::textures::rgba_to_dds(rgba, w, h)?.dds)?;
        }
    }
    // spectators.xml: every <texture> is one sprite sheet of 8 cells (4 standing, 4 seated) at (spritex, spritey) of a 32 x 8 cell atlas (64 x 128
    // cells), and `per_model` (8) is only the default stride.
    let xml = String::from_utf8_lossy(&read(&mut spec, "spectators.xml")?).trim_start_matches('\u{feff}').to_owned();
    let doc = crate::xml::to_json(&xml)?;
    let root = &doc["spectators"];
    let arr = |v: &Value| -> Vec<Value> {
        match v {
            Value::Array(a) => a.clone(),
            Value::Null => vec![],
            o => vec![o.clone()],
        }
    };
    // Models in xml order, one per <texture>; `cell` is its first sprite cell (spritey * 32 + spritex). Most sheets have 8 views (4 standing, 4 seated);
    // the Photographer, Marshall and Pit worker have 4 (standing only), so the cell is stored instead of derived from the index.
    let mut atlas_models: Vec<(String, String, usize)> = Vec::new();
    for m in arr(&root["models"]["model"]) {
        let name = m["name"].as_str().unwrap_or("").to_owned();
        for t in arr(&m["texture"]) {
            let (sx, sy) = (t["spritex"].as_f64().unwrap_or(0.0) as usize, t["spritey"].as_f64().unwrap_or(0.0) as usize);
            atlas_models.push((name.clone(), t["name"].as_str().unwrap_or("").trim_end_matches(".xds").trim_end_matches(".dds").to_owned(), sy * 32 + sx));
        }
    }
    let models_json: Vec<Value> = atlas_models.iter().map(|(n, t, c)| json!({"name": n, "texture": t, "cell": c})).collect();
    let sets: Vec<(String, Vec<String>)> = arr(&root["modelset"]).iter().map(|s| (s["name"].as_str().unwrap_or("").to_owned(), arr(&s["model"]).iter().map(|m| m["name"].as_str().unwrap_or("").to_owned()).collect())).collect();
    let skeletons: Vec<(String, String)> = arr(&root["skeleton"]).iter().map(|s| (s["name"].as_str().unwrap_or("").to_owned(), s["suffix"].as_str().unwrap_or("").to_owned())).collect();
    let names = |v: &Value, key: &str| -> Vec<String> { arr(&v[key]).iter().filter_map(|a| a["name"].as_str().map(str::to_owned)).collect() };
    let mut classes = Vec::new();
    for (i, c) in arr(&root["crowdclass"]).iter().enumerate() {
        let skel = c["skeleton"]["name"].as_str().unwrap_or("").to_owned();
        let set = c["modelset"]["name"].as_str().unwrap_or("").to_owned();
        let members = sets.iter().find(|s| s.0.eq_ignore_ascii_case(&set)).map(|s| s.1.clone()).unwrap_or_default();
        let atlas: Vec<usize> = atlas_models.iter().enumerate().filter(|(_, m)| members.iter().any(|n| n.eq_ignore_ascii_case(&m.0))).map(|(k, _)| k).collect();
        classes.push(json!({
            "index": i,
            "name": c["name"],
            "sitting": skel == "Sitting",
            "skeleton": skel,
            "suffix": skeletons.iter().find(|s| s.0 == skel).map(|s| s.1.clone()).unwrap_or_default(),
            "idle": names(c, "idle"),
            "cheer": names(c, "cheer"),
            "models": atlas,
            "bb_offset_y": 0.0,
        }));
    }
    println!("[fm2 crowd] {models} models, {anims} anims, {} sprite sheets, {} classes; atlas {aw} x {ah}", atlas_models.len(), classes.len());
    let atlas = json!({"file": "../../crowd/sprites.dds", "width": aw, "height": ah, "cell": [64, 128], "per_model": 8, "models": atlas_models.iter().map(|m| m.0.clone()).collect::<Vec<_>>()});
    Ok(Shared { models: models_json, classes, atlas })
}

/// One map: its `.crowd` file -> `<dir>/crowds.bin` (`FH1CRWD1`; the file's z already runs the engine's way, unlike the AI line and spawns, so it is copied as is) and `<dir>/index.json`.
pub(crate) fn map(shared: &Shared, crowd_file: &Path, dir: &Path) -> Result<usize> {
    let d = std::fs::read(crowd_file).with_context(|| crowd_file.display().to_string())?;
    anyhow::ensure!(d.len() >= 12 && be_u(&d, 0) == 1, "{}: not a version 1 .crowd", crowd_file.display());
    let (g, s) = (be_u(&d, 4) as usize, be_u(&d, 8) as usize);
    let base = 12 + 52 * g;
    anyhow::ensure!(d.len() == base + 32 * s, "{}: {} bytes, expected {}", crowd_file.display(), d.len(), base + 32 * s);
    std::fs::create_dir_all(dir)?;
    let mut blob = b"FH1CRWD1".to_vec();
    blob.extend((s as u32).to_le_bytes());
    let mut objects = Vec::new();
    let mut seen = 0usize;
    for i in 0..g {
        let o = 12 + 52 * i;
        let (first, count) = (be_u(&d, o + 44) as usize, be_u(&d, o + 48) as usize);
        anyhow::ensure!(first == seen && first + count <= s, "{}: group {i} out of order", crowd_file.display());
        seen += count;
        if count == 0 {
            continue;
        }
        // The bounds come from the spectators themselves (a stored box can be empty or inverted: the engine clamps to it).
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for k in first..first + count {
            let o = base + 32 * k;
            for (a, off) in [(0, 0), (1, 4), (2, 8)] {
                let v = be_f(&d, o + off);
                lo[a] = lo[a].min(v);
                hi[a] = hi[a].max(v);
            }
        }
        objects.push(json!({"name": format!("group{i}"), "named": true, "first": first, "count": count, "min": lo, "max": hi}));
    }
    for k in 0..s {
        let o = base + 32 * k;
        let (x, y, z) = (be_f(&d, o), be_f(&d, o + 4), be_f(&d, o + 8));
        let heading = be_f(&d, o + 12).atan2(be_f(&d, o + 20));
        let h = ((heading / std::f32::consts::TAU).rem_euclid(1.0) * 256.0) as u8;
        for v in [x, y, z] {
            blob.extend(v.to_le_bytes());
        }
        // The 4th byte (0 before) is the record's byte at offset 26: 0..255, probably the spectator's share of sun (MODLOG 122; NOT verified).
        blob.extend([h, d[o + 24], d[o + 25], d[o + 26]]);
    }
    std::fs::write(dir.join("crowds.bin"), &blob)?;
    let mut index = json!({"objects": objects, "classes": shared.classes, "models": shared.models, "atlas": shared.atlas});
    // The map's `<Crowds dark_x.. light_x..>` colours (TrackSettings.xml, two folders above the ribbon's .crowd file) and the mean of that byte / 255.
    let xml = crowd_file.parent().and_then(Path::parent).map(|p| p.join("TrackSettings.xml")).and_then(|p| std::fs::read_to_string(p).ok());
    if let Some(el) = xml.as_deref().and_then(|x| x.find("<Crowds ").map(|i| &x[i..i + x[i..].find('>').unwrap_or(0)])) {
        let attr = |k: &str| -> Option<f64> {
            let p = el.find(&format!(" {k}=\""))? + k.len() + 3;
            el[p..p + el[p..].find('"')?].trim().parse().ok()
        };
        let rgb = |pre: &str| Some([attr(&format!("{pre}_x"))?, attr(&format!("{pre}_y"))?, attr(&format!("{pre}_z"))?]);
        if let (Some(dark), Some(light)) = (rgb("dark"), rgb("light")) {
            let mean = (0..s).map(|k| d[base + 32 * k + 26] as f64).sum::<f64>() / (s.max(1) as f64 * 255.0);
            index["tint"] = json!({"dark": dark, "light": light, "mean": mean});
        }
    }
    std::fs::write(dir.join("index.json"), serde_json::to_vec_pretty(&index)?)?;
    Ok(s)
}

/// The ribbon's `.crowd` file (`<track>_track_NN.crowd`, any case).
pub(crate) fn crowd_file(tracks: &Path, track: &str, ribbon: u32) -> Option<PathBuf> {
    let dir = std::fs::read_dir(tracks).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(track))?.path();
    let rib = std::fs::read_dir(&dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&format!("Ribbon_{ribbon:02}")))?.path();
    std::fs::read_dir(rib).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("crowd")))
}
