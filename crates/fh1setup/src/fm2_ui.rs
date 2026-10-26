//! `fh1setup import-fm2 --only ui`: FM2's in-race HUD (`Media/UI.zip` scenes, `Media/UI/Textures.zip`, `Media/UI/Fonts.zip`,
//! `Media/StringTables/EN.zip`) -> `imported/fm2/ui/{hud.json, tex/*.png, fonts/*}`.
//!
//! The scenes are Anark `.xgf` trees (`fh1_formats::xgf`). Their nesting does not say which container (`component` / `group`) an item
//! sits in, so the frame of an item is found like this (checked against the Xenia screenshots: LAPS panel, RACE timer rows, gear and
//! speed digits all land on the measured pixels):
//! - a container is a *leaf record* (`first_child` none): its content is the node right after it and that node's subtree; a container with a
//!   `first_child` frames its own subtree;
//! - a node belongs to the innermost container whose content range holds the node's whole unit (a container's unit also spans its content,
//!   so a following row component is not swallowed by the previous row);
//! - position = parent position + own position (scene units: 1280 x 720 centred, y up); scale is not inherited (measured on the tach plate).
//!
//! A model's picture is the first `image` after it (before the next model / container); a model whose buffer is a Rectangle is a 100 x 100
//! quad scaled by its `scale`. Text strings are resolved from the scene's own string table (`_locstr(<scene>, <id>)`).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result};
use fh1_formats::xgf::Xgf;
use fh1_formats::xds;
use serde_json::{json, Value};

/// (scene, folder under `Scenes/I_Scenes`).
const SCENES: [(&str, &str); 4] = [("I_HUD_All_Divided_PartOne", "i_hud"), ("I_HUD_All_Divided_PartTwo", "i_hud"), ("I_PRE_All_Modes", "i_pre"), ("I_321_Go", "i_321")];
const NONE: usize = usize::MAX;

pub fn import(disc: &Path, root: &Path) -> Result<()> {
    let out = root.join("ui");
    std::fs::create_dir_all(out.join("tex"))?;
    std::fs::create_dir_all(out.join("fonts"))?;
    let mut ui_zip = fh1_formats::zip::Archive::open(disc.join("media/UI.zip")).context("media/UI.zip")?;
    let strings = fh1_ui::strtable::StringTables::load_zip(disc.join("media/StringTables/EN.zip")).context("media/StringTables/EN.zip")?;
    let mut tex_names: BTreeMap<String, ()> = BTreeMap::new();
    let mut scenes = serde_json::Map::new();
    let mut meshes: BTreeMap<String, Value> = BTreeMap::new();
    for (scene, folder) in SCENES {
        let want = format!("scenes/i_scenes/{folder}/{}.xgf", scene.to_ascii_lowercase());
        let e = ui_zip.entries.iter().find(|e| e.name.replace('\\', "/").to_ascii_lowercase() == want).with_context(|| format!("{want} not in UI.zip"))?.clone();
        let x = Xgf::parse(&ui_zip.read(&e)?)?;
        let items = items(&x, scene, &strings, &mut tex_names, &mut meshes);
        println!("[fm2] ui: {scene}: {} nodes, {} items", x.nodes.len(), items.len());
        scenes.insert(scene.to_owned(), Value::Array(items));
    }
    // Textures: the HUD folder (and the 3-2-1 lights), decoded to PNG. `gamma` = the console decodes the colour channels as sRGB.
    let mut tex_ar = fh1_formats::zip::Archive::open(disc.join("media/UI/Textures.zip")).context("media/UI/Textures.zip")?;
    let mut textures = serde_json::Map::new();
    for e in tex_ar.entries.clone() {
        let name = e.name.replace('\\', "/");
        let lower = name.to_ascii_lowercase();
        let Some(stem) = name.rsplit('/').next().and_then(|f| f.strip_suffix(".xds").or_else(|| f.strip_suffix(".XDS"))) else { continue };
        // The HUD folders, plus any texture a scene names (the pause screen's list buttons live in X_Map / P_Map).
        let named = tex_names.keys().any(|n| n.eq_ignore_ascii_case(stem));
        if !named && !["i_map/i_hud/", "i_map/i_321/"].iter().any(|p| lower.starts_with(p)) {
            continue;
        }
        let (h, img) = xds::decode_base(&tex_ar.read(&e)?).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        let mut rgba = xds::to_rgba8(&img).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        if matches!(h.format, xds::Format::Dxt5A | xds::Format::Dxt3A) {
            for px in rgba.chunks_exact_mut(4) {
                let v = px[0];
                px.copy_from_slice(&[255, 255, 255, v]);
            }
        }
        let gamma = (h.fetch[0] >> 2) & 0x3F == 0x3F;
        let file = std::fs::File::create(out.join("tex").join(format!("{stem}.png")))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), h.width, h.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut w| w.write_image_data(&rgba)).map_err(|e| anyhow::anyhow!("{stem}.png: {e}"))?;
        textures.insert(stem.to_owned(), json!({ "file": format!("tex/{stem}.png"), "w": h.width, "h": h.height, "gamma": gamma }));
    }
    // Fonts: copied as the disc has them (names lower-cased so `<target>_vector_aa.dt` finds them).
    let mut font_ar = fh1_formats::zip::Archive::open(disc.join("media/UI/Fonts.zip")).context("media/UI/Fonts.zip")?;
    for e in font_ar.entries.clone() {
        let base = e.name.rsplit(['/', '\\']).next().unwrap_or(&e.name).to_ascii_lowercase();
        if base.ends_with("_vector_aa.dt") || base == "fontmap.xml" {
            std::fs::write(out.join("fonts").join(&base), font_ar.read(&e)?)?;
        }
    }
    let missing: Vec<&String> = tex_names.keys().filter(|n| !textures.contains_key(*n)).collect();
    if !missing.is_empty() {
        println!("[fm2] ui: textures named by the scenes but not decoded: {missing:?}");
    }
    // Labels the HUD's Lua sets (row titles from `Forza_Flow:IDS_HUD_*`, the speed unit from `I_HUD_All` / `UnitStrings`).
    let mut labels = serde_json::Map::new();
    for (key, file, id) in [
        ("race", "Forza_Flow", "IDS_HUD_RACE"),
        ("lap", "Forza_Flow", "IDS_HUD_LAP"),
        ("best", "Forza_Flow", "IDS_HUD_BEST"),
        ("last", "Forza_Flow", "IDS_HUD_LAST"),
        ("penalty", "Forza_Flow", "IDS_HUD_PENALTY"),
        ("mph", "I_HUD_All", "IDS_Label_HUD_MPH"),
        ("kmh", "UnitStrings", "IDS_Units_KmPerHour"),
        ("laps", "I_HUD_All", "IDS_Label_HUD_Laps"),
        ("resume", "Forza_Flow", "IDS_Resume"),
        ("restart", "Forza_Flow", "IDS_Restart"),
        ("photo", "Forza_Flow", "IDS_ShootPhoto"),
        ("quit_race", "Forza_Flow", "IDS_QuitRace"),
        ("audio", "Forza_Flow", "IDS_Audio"),
        ("hud", "Forza_Flow", "IDS_HUD"),
        ("select", "Forza_Flow", "IDS_Select"),
        ("back", "Forza_Flow", "IDS_Back"),
        ("driver", "Forza_Flow", "IDS_Driver"),
        ("pl", "Forza_Flow", "IDS_PL"),
        ("best_lap", "Forza_Flow", "IDS_BestLap"),
        ("options", "DebugMainMenu", "IDS_Options"),
    ] {
        // FM2's id hashes are not FH1's: find the id by the name the table stores.
        let found = strings.file(file).and_then(|t| t.names.iter().find(|(_, n)| n.name.eq_ignore_ascii_case(id)).and_then(|(h, _)| t.texts.get(h)));
        match found {
            Some(t) => {
                let t = t.trim_start_matches("{0}").trim();
                labels.insert(key.to_owned(), json!(if key == "kmh" { t.to_ascii_uppercase() } else { t.to_owned() }));
            }
            None => println!("[fm2] ui: string {file}:{id} not found"),
        }
    }
    let doc = json!({ "scenes": scenes, "textures": textures, "labels": labels, "meshes": meshes });
    std::fs::write(out.join("hud.json"), serde_json::to_vec_pretty(&doc)?)?;
    println!("[fm2] ui: {} textures, hud.json written to {}", textures.len(), out.display());
    Ok(())
}

fn nums(s: Option<&str>, n: usize, default: f32) -> Vec<f32> {
    let mut v: Vec<f32> = s.map(|s| s.split_whitespace().filter_map(|t| t.parse().ok()).collect()).unwrap_or_default();
    v.resize(n, default);
    v
}

/// Node attributes with the library entry (`xmlid == xmlref`) underneath: the instance's own values win.
fn merged(x: &Xgf, lib: &HashMap<String, usize>) -> Vec<HashMap<String, String>> {
    x.nodes
        .iter()
        .map(|n| {
            let mut m: HashMap<String, String> = HashMap::new();
            if let Some(l) = n.attr("xmlref").and_then(|r| lib.get(r)) {
                for (k, v) in &x.nodes[*l].attrs {
                    m.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in &n.attrs {
                m.insert(k.clone(), v.clone());
            }
            m
        })
        .collect()
}

fn items(x: &Xgf, scene: &str, strings: &fh1_ui::strtable::StringTables, tex_names: &mut BTreeMap<String, ()>, meshes_out: &mut BTreeMap<String, Value>) -> Vec<Value> {
    let n = x.nodes.len();
    // Library entries: nodes with an xmlid and no xmlref.
    let mut lib: HashMap<String, usize> = HashMap::new();
    for (i, nd) in x.nodes.iter().enumerate() {
        if let (Some(id), None) = (nd.attr("xmlid"), nd.attr("xmlref")) {
            lib.entry(id.to_owned()).or_insert(i);
        }
    }
    let full = merged(x, &lib);
    let ty = |i: usize| full[i].get("type").map(String::as_str).unwrap_or("");
    // Subtree ends (first node after the subtree): the next sibling, else the parent's end.
    let mut end = vec![NONE; n];
    fn end_of(x: &Xgf, i: usize, memo: &mut Vec<usize>) -> usize {
        if memo[i] != NONE {
            return memo[i];
        }
        // Next sibling = the following child of the parent.
        let r = match x.nodes[i].parent {
            Some(p) => {
                let sibs = &x.nodes[p].children;
                let k = sibs.iter().position(|&c| c == i).unwrap();
                if k + 1 < sibs.len() { sibs[k + 1] } else { end_of(x, p, memo) }
            }
            None => x.nodes.len(),
        };
        memo[i] = r;
        r
    }
    for i in 0..n {
        end_of(x, i, &mut end);
    }
    let containers: Vec<usize> = (0..n).filter(|&i| matches!(ty(i), "component" | "group")).collect();
    // Content range of each container.
    let mut range: HashMap<usize, (usize, usize)> = HashMap::new();
    for &c in &containers {
        let r = if !x.nodes[c].children.is_empty() { (c + 1, end[c]) } else if c + 1 < n { (c + 1, end[c + 1]) } else { (c + 1, c + 1) };
        range.insert(c, r);
    }
    let parent_frame = |i: usize| -> Option<usize> {
        let hi = if let Some(r) = range.get(&i) { r.1.max(i + 1) } else { i + 1 };
        containers.iter().copied().filter(|&y| y != i).filter(|&y| { let (a, b) = range[&y]; a <= i && hi <= b && a > y }).min_by_key(|&y| range[&y].1 - range[&y].0)
    };
    // Absolute position / scale per node (memoised, parents first).
    let mut abs: Vec<Option<([f32; 3], [f32; 3], Option<usize>)>> = vec![None; n];
    fn place(i: usize, full: &Vec<HashMap<String, String>>, pf: &dyn Fn(usize) -> Option<usize>, abs: &mut Vec<Option<([f32; 3], [f32; 3], Option<usize>)>>) -> ([f32; 3], [f32; 3], Option<usize>) {
        if let Some(a) = abs[i] {
            return a;
        }
        let p = pf(i);
        let (bp, bs) = match p {
            Some(p) => {
                let a = place(p, full, pf, abs);
                (a.0, a.1)
            }
            None => ([0.0; 3], [1.0; 3]),
        };
        let pos = nums(full[i].get("position").map(String::as_str), 3, 0.0);
        let sc = nums(full[i].get("scale").map(String::as_str), 3, 1.0);
        // Scale is not inherited: a component's scale (72 on the tach pieces) is for its own meshes; its children's offsets stay in
        // scene units (the 256 px tach plate sits 7 units below its component, not 7 x 72).
        let _ = bs;
        let a = ([bp[0] + pos[0], bp[1] + pos[1], bp[2] + pos[2]], [sc[0], sc[1], sc[2]], p);
        abs[i] = Some(a);
        a
    }
    let mut out = Vec::new();
    for i in 0..n {
        let kind = ty(i);
        // The tach script's settings (`TachHatches` behaviours) ride along as `kind: "tach"` with their `m*` attributes as they are.
        if kind == "behavior" && full[i].get("name").is_some_and(|n| n == "TachHatches") {
            let (p, _, parent) = place(i, &full, &parent_frame, &mut abs);
            let params: serde_json::Map<String, Value> = full[i].iter().filter(|(k, _)| k.starts_with('m')).map(|(k, v)| (k.clone(), json!(v))).collect();
            out.push(json!({ "scene": scene, "index": i, "kind": "tach", "name": "TachHatches", "parent": parent, "x": p[0], "y": p[1], "params": params }));
            continue;
        }
        if !matches!(kind, "model" | "text" | "component" | "group") {
            continue;
        }
        let (p, s, parent) = place(i, &full, &parent_frame, &mut abs);
        // Container names up the chain, innermost first.
        let mut path = Vec::new();
        let mut cur = parent;
        while let Some(c) = cur {
            if let Some(nm) = full[c].get("name") {
                path.push(nm.clone());
            }
            cur = abs[c].and_then(|a| a.2);
        }
        let mut it = json!({
            "scene": scene, "index": i, "kind": kind, "name": full[i].get("name"),
            "parent": parent, "path": path,
            "x": p[0], "y": p[1], "z": p[2], "sx": s[0], "sy": s[1], "sz": s[2],
            "rotation": nums(full[i].get("rotation").map(String::as_str), 3, 0.0),
            "opacity": full[i].get("opacity"),
        });
        match kind {
            "model" => {
                let buf = full[i].get("bufferref").and_then(|b| lib.get(b)).map(|&b| &x.nodes[b]);
                it["buffer"] = json!(buf.and_then(|b| b.attr("name")));
                it["buffer_data"] = json!(buf.and_then(|b| b.attr("modeldata")));
                it["pivot"] = json!(nums(full[i].get("pivot").map(String::as_str), 3, 0.0));
                // Meshes of the third-person tach cluster (needle, tick marks, arcs) are exported with their vertices (x, y, z, u, v).
                let in_3p = path.iter().any(|p| p == "Tach_Speed_Combined_3P") || full[i].get("name").is_some_and(|n| n == "Tach_Speed_Combined_3P");
                if let (true, Some(mi), false) = (in_3p, buf.and_then(|b| b.attr("modeldata")).and_then(|m| m.parse::<usize>().ok()), it["rect"].as_bool().unwrap_or(false)) {
                    if let Some(m) = x.meshes.get(mi) {
                        let key = format!("{scene}:{mi}");
                        meshes_out.entry(key.clone()).or_insert_with(|| {
                            json!({
                                "vertices": m.vertices.iter().map(|v| json!([v.pos[0], v.pos[1], v.pos[2], v.uv[0], v.uv[1]])).collect::<Vec<_>>(),
                                "groups": m.groups,
                            })
                        });
                        it["mesh"] = json!(key);
                    }
                }
                it["rect"] = json!(buf.is_some_and(|b| b.attr("primitivetype") == Some("1") && b.attr("subtype") == Some("2")));
                // Material (after the model): the instance's own diffuse colour and opacity win over the library's.
                if let Some(m) = (i + 1..n).take_while(|&j| !matches!(ty(j), "model" | "component" | "group")).find(|&j| ty(j) == "material") {
                    let d = nums(full[m].get("diffuse").map(String::as_str), 4, 255.0);
                    it["diffuse"] = json!([d[0], d[1], d[2]]);
                    it["mat_opacity"] = json!(full[m].get("opacity").and_then(|o| o.parse::<f32>().ok()).unwrap_or(100.0));
                }
                // The scale of the container the model sits in (meshes take it on top of their own: the tach's red arc is 1 x 72).
                if let Some(pc) = parent {
                    it["container_scale"] = json!(nums(full[pc].get("scale").map(String::as_str), 3, 1.0));
                }
                // The picture: the first image after the model, before the next model / container.
                let mut j = i + 1;
                while j < n && !matches!(ty(j), "model" | "component" | "group") {
                    if ty(j) == "image" {
                        let buf = full[j].get("bufferref").and_then(|b| lib.get(b)).map(|&b| &x.nodes[b]);
                        if let Some(path) = buf.and_then(|b| b.attr("imagedata")) {
                            let stem = path.rsplit(['\\', '/']).next().unwrap_or("").rsplit_once('.').map_or("", |(s, _)| s).to_owned();
                            tex_names.insert(stem.clone(), ());
                            it["texture"] = json!(stem);
                            it["scalev"] = json!(full[j].get("scalev"));
                            it["tile_h"] = json!(full[j].get("tilingmodehorz"));
                            it["tile_v"] = json!(full[j].get("tilingmodevert"));
                        }
                        break;
                    }
                    j += 1;
                }
            }
            "text" => {
                let raw = full[i].get("textstring").cloned().unwrap_or_default();
                let text = match raw.strip_prefix("_locstr(").and_then(|r| r.strip_suffix(')')) {
                    Some(inner) => inner
                        .split_once(',')
                        .and_then(|(f, id)| strings.file(f.trim())?.texts.get(&id.trim().parse::<u16>().ok()?).cloned())
                        .unwrap_or(raw.clone()),
                    None => raw.clone(),
                };
                for k in ["size", "font", "textcolor", "horzalign", "vertalign", "tracking", "boxwidth", "renderstyle", "leading", "locid"] {
                    it[k] = json!(full[i].get(k));
                }
                it["text"] = json!(text);
            }
            _ => {}
        }
        out.push(it);
    }
    out
}
