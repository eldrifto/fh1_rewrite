//! `fh1setup import-fm1 <Forza Motorsport 1 disc folder> [--data <dir>] [--only cars]`: Forza Motorsport 1 (original Xbox, 2005) cars into the
//! ACTIVE install (`imported/fm1/cars/<name>/`, the same layout as the other imported games: `model.gltf` + `model.bin`, `model.json`,
//! `physics.json`, `index.json`).
//!
//! The disc must be extracted (`extract-xiso -x forza1.iso <folder>`): the importer reads `Media/cabs/Cars.ca2` and `Media/cabs/car_*.cab`
//! (`fh1_formats::fm1`).
//!
//! WHAT IS REAL: the body parts and their uv (`car_<name>.cab`, the stock part of each kit family named by the car's `Upgrades` XML), the
//! hub positions, the detail atlas (`XPR0`, 1024 x 1024 DXT5: lamps, interior, grilles), the stock paint (the car's first `<livery>`), and
//! the physics tables in `Cars.ca2`: weight, wheelbase / track / collision spheres (`MAXData`), the stock gearbox, the engine's torque curve
//! and redline, tyre sizes, drive type.
//! WHAT IS MATCHED, NOT READ: the files of `Cars.ca2` are keyed by a hash of their path (31 bits, the function lives in `default.xbe` and was not
//! recovered), so a car's files are found by position: the archive is stored one folder per car in case-insensitive alphabetical order, the same
//! order as the sorted `car_<name>.cab` names (231 of each; checked: the stored `MAXData` bounding-box height equals the cab geometry's on 134 of
//! the 231 cars to 2 cm, a one-car shift matches 40), and the engines in the order of the sorted engine file names the cars' `<E0 File=...>` name.
//! WHAT IS BORROWED: the wheels (an FH1 car's rim + tyre meshes scaled to the tyre size), and everything FM1 has no table decoded for
//! here (suspension, tyre compound curves, aero, camera, brakes), which comes from the same FH1 donor's `physics.json` (the closest car by
//! mass, drive type and year, as the FM2 importer does).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fh1_formats::carbin_fm2::{Part, Run, Vertex};
use fh1_formats::fm1::{self, Entry};
use serde_json::{json, Value};

use crate::fm2_cars::{f, paint_rgb, pick, signed_volume, Colour, Copier, Gltf, WheelDonor, PALETTE};

pub fn run() -> Result<()> {
    let mut it = std::env::args().skip(2);
    let (mut source, mut data, mut only) = (None, PathBuf::from("data"), None::<Vec<String>>);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().context("--data needs a path")?.into(),
            "--only" => only = Some(it.next().context("--only needs cars")?.split(',').map(str::to_owned).collect()),
            _ => source = Some(PathBuf::from(a)),
        }
    }
    let source = source.context("usage: fh1setup import-fm1 <Forza Motorsport 1 ISO|zip|disc folder> [--data <dir>] [--only cars]")?;
    let inst: Value = serde_json::from_slice(&std::fs::read(data.join("installation.json")).context("installation.json: run fh1setup on the FH1 disc first")?)?;
    // An ISO or zip is extracted first (the launcher hands over whatever the user picked).
    let source = crate::extract::resolve_any(&source, &data.join("work_fm1"))?;
    let disc = disc_root(&source)?;
    println!("[fm1] disc: {}", disc.display());
    let private = data.join("installations").join(inst["id"].as_str().context("installation.json id")?).join("assets/private");
    let root = private.join("imported/fm1");
    std::fs::create_dir_all(&root)?;
    let want = |p: &str| only.as_ref().is_none_or(|o| o.iter().any(|x| x == p));
    if want("cars") {
        crate::fh2::swap(&root.join("cars"), |stage| cars(&disc, &private, stage))?;
    }
    println!("[fm1] done: {}", root.display());
    Ok(())
}

/// The folder holding `default.xbe` (or `Media`), given the disc folder or its `Media` folder.
fn disc_root(source: &Path) -> Result<PathBuf> {
    anyhow::ensure!(source.is_dir(), "{} is not a folder: extract the ISO first (`extract-xiso -x forza1.iso <folder>`)", source.display());
    for dir in [source.to_path_buf(), source.join("disc")] {
        if child(&dir, "media").is_some_and(|m| child(&m, "cabs").is_some()) {
            return Ok(dir);
        }
    }
    if source.file_name().is_some_and(|n| n.eq_ignore_ascii_case("media")) && child(source, "cabs").is_some() {
        return Ok(source.parent().unwrap().to_path_buf());
    }
    anyhow::bail!("{}: no Media/cabs folder (not an extracted Forza Motorsport 1 disc)", source.display())
}

fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

/// One file of a car's folder in `Cars.ca2`, by root element, `n`-th of its kind.
fn nth<'a>(files: &'a [(String, String)], kind: &str, n: usize) -> Option<&'a str> {
    files.iter().filter(|(k, _)| k == kind).nth(n).map(|(_, t)| t.as_str())
}

/// A forgiving XML -> JSON (the same shape as `crate::xml::to_json`): FM1's files have unquoted or half-quoted attribute values
/// (`ui=10021"`, `red=186`), spaces around `=` and stray bytes, which a strict parser stops on.
fn xml(text: &str) -> Value {
    fn insert(parent: &mut serde_json::Map<String, Value>, name: String, v: Value) {
        match parent.get_mut(&name) {
            None => {
                parent.insert(name, v);
            }
            Some(Value::Array(a)) => a.push(v),
            Some(e) => {
                let prev = e.take();
                *e = Value::Array(vec![prev, v]);
            }
        }
    }
    let b = text.as_bytes();
    let mut stack: Vec<(String, serde_json::Map<String, Value>)> = vec![(String::new(), Default::default())];
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"<?") || b[i..].starts_with(b"<!") {
            i = text[i..].find('>').map_or(b.len(), |e| i + e + 1);
            continue;
        }
        if b[i..].starts_with(b"</") {
            i = text[i..].find('>').map_or(b.len(), |e| i + e + 1);
            if stack.len() > 1 {
                let (n, o) = stack.pop().unwrap();
                insert(&mut stack.last_mut().unwrap().1, n, Value::Object(o));
            }
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
            j += 1;
        }
        let name = text[i + 1..j].to_owned();
        let (mut attrs, mut selfclose) = (serde_json::Map::new(), false);
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                break;
            }
            if b[j] == b'>' {
                j += 1;
                break;
            }
            if b[j] == b'/' {
                if b.get(j + 1) == Some(&b'>') {
                    selfclose = true;
                    j += 2;
                    break;
                }
                j += 1;
                continue;
            }
            let ks = j;
            while j < b.len() && b[j] != b'=' && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
                j += 1;
            }
            let key = text[ks..j].to_owned();
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if b.get(j) != Some(&b'=') {
                continue;
            }
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let val = if b.get(j) == Some(&b'"') {
                j += 1;
                let vs = j;
                while j < b.len() && b[j] != b'"' {
                    j += 1;
                }
                let v = &text[vs..j];
                j += 1;
                v
            } else {
                let vs = j;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && !(b[j] == b'/' && b.get(j + 1) == Some(&b'>')) {
                    j += 1;
                }
                text[vs..j].trim_end_matches('"')
            };
            if !key.is_empty() {
                attrs.insert(key, crate::xml::scalar(val));
            }
        }
        i = j;
        if selfclose {
            insert(&mut stack.last_mut().unwrap().1, name, Value::Object(attrs));
        } else {
            stack.push((name, attrs));
        }
    }
    while stack.len() > 1 {
        let (n, o) = stack.pop().unwrap();
        insert(&mut stack.last_mut().unwrap().1, n, Value::Object(o));
    }
    Value::Object(stack.pop().unwrap().1)
}

/// A number the XML parser stored (always as `f64`).
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn cars(disc: &Path, private: &Path, out: &Path) -> Result<()> {
    let media = child(disc, "media").context("media folder")?;
    let cabs = child(&media, "cabs").context("cabs folder")?;
    let ca2 = fm1::unchunk(&std::fs::read(child(&cabs, "Cars.ca2").context("Cars.ca2")?)?);
    let entries = fm1::archive(&ca2).map_err(|e| anyhow::anyhow!("Cars.ca2: {e}"))?;
    let files = fm1::car_files(&entries);
    // The car cabs in the archive's folder order (case-insensitive alphabetical).
    let mut names: Vec<String> = std::fs::read_dir(&cabs)?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let l = n.to_ascii_lowercase();
            (l.starts_with("car_") && l.ends_with(".cab")).then(|| n[4..n.len() - 4].to_owned())
        })
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    println!("[fm1] {} car cabs, {} car folders and {} engine folders in Cars.ca2", names.len(), files.cars.len(), files.engines.len());
    anyhow::ensure!(names.len() == files.cars.len(), "{} car cabs but {} car folders in Cars.ca2: the folder order assumption fails", names.len(), files.cars.len());

    // The engine folders are stored in the order of the sorted engine names the cars refer to.
    let engine_of = |car: &Value| -> Option<(String, String)> {
        let e = &car["Car"]["Upgrades"]["Engines"]["E0"];
        Some((e["File"].as_str()?.to_owned(), e["Name"].as_str().unwrap_or_default().to_owned()))
    };
    let car_xml: Vec<Value> = files.cars.iter().map(|g| g.iter().filter(|(k, t)| k == "Car" && t.len() > 1000).map(|(_, t)| xml(t)).next().unwrap_or(Value::Null)).collect();
    // Every engine file a car names (`E0`, `E1`, ...), sorted case-insensitively.
    let mut engine_names: Vec<String> = Vec::new();
    for c in &car_xml {
        for (k, e) in c["Car"]["Upgrades"]["Engines"].as_object().into_iter().flatten() {
            if let (true, Some(file)) = (k.starts_with('E'), e["File"].as_str()) {
                if !engine_names.iter().any(|n| n.eq_ignore_ascii_case(file)) {
                    engine_names.push(file.to_owned());
                }
            }
        }
    }
    engine_names.sort_by_key(|n| n.to_ascii_lowercase());
    println!("[fm1] {} engine names ({} engine folders)", engine_names.len(), files.engines.len());

    // The game's car list (`Mega.ca2`, the largest `<carlist>`): name, make, display names and the performance class (0 = not in a class: race
    // specials and tuner cars; 1..5, 5 the prototypes). A car without an entry keeps its engine's name.
    let mega = fm1::archive(&fm1::unchunk(&std::fs::read(child(&cabs, "Mega.ca2").context("Mega.ca2")?)?)).map_err(|e| anyhow::anyhow!("Mega.ca2: {e}"))?;
    let carlist = mega.iter().filter_map(|e| std::str::from_utf8(&e.data).ok()).filter(|t| t.contains("<carlist")).max_by_key(|t| t.len()).unwrap_or_default();
    struct Listed {
        name: String,
        display: String,
        short: String,
        make: String,
        class: Option<String>,
    }
    let listed: Vec<Listed> = carlist
        .split("<car ")
        .skip(1)
        .filter_map(|c| {
            let tag = &c[..c.find('>').unwrap_or(c.len())];
            let class = attr(tag, "BasePerformanceClass").filter(|k| k != "0");
            Some(Listed { name: attr(tag, "name")?, display: attr(tag, "displayName")?, short: attr(tag, "shortName").unwrap_or_default(), make: attr(tag, "Make").unwrap_or_default(), class })
        })
        .collect();
    println!("[fm1] {} cars in the game's car list", listed.len());
    let wheels = load_wheels(&mega);
    println!("[fm1] {} wheel models in Mega.ca2", wheels.len());
    let sound = crate::fh2::Donors::load(private);
    let donors = WheelDonor::load_all(private);
    anyhow::ensure!(!donors.is_empty(), "no FH1 car with a model + physics.json to borrow wheels from (run fh1setup on the FH1 disc first)");

    let (mut index, mut skipped) = (Vec::new(), Vec::new());
    for (i, name) in names.iter().enumerate() {
        let car = &car_xml[i];
        let (efile, ename) = engine_of(car).unwrap_or_default();
        if efile.is_empty() {
            println!("[fm1] {name}: no <E0 File=...> in its Car XML (keys: {:?})", car["Car"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()));
        }
        let engine = engine_names.iter().position(|e| e.eq_ignore_ascii_case(&efile)).and_then(|k| files.engines.get(k));
        let cab = std::fs::read(cabs.join(format!("car_{name}.cab")).as_path()).ok().or_else(|| child(&cabs, &format!("car_{name}.cab")).and_then(|p| std::fs::read(p).ok()));
        let media_name = listed.iter().find(|l| l.name.eq_ignore_ascii_case(name)).map(|l| l.name.clone()).unwrap_or_else(|| name.clone());
        let r = cab.context("cab").and_then(|c| build_car(&c, &files.cars[i], car, engine.map(Vec::as_slice), &donors, &wheels, i as i64, &media_name, &out.join(&media_name)));
        match r {
            Ok(mut b) => {
                if let Some(d) = sound.pick(&media_name, &b.physics["car"]) {
                    b.physics["sound_donor"] = json!(d);
                }
                std::fs::write(out.join(&media_name).join("physics.json"), serde_json::to_vec_pretty(&b.physics)?)?;
                let entry = listed.iter().find(|l| l.name.eq_ignore_ascii_case(name));
                let display = entry.map(|l| l.display.clone()).filter(|d| !d.is_empty()).unwrap_or(if ename.is_empty() { media_name.clone() } else { ename });
                let maker = entry.map(|l| l.make.clone()).unwrap_or_default();
                let model = entry.map(|l| l.short.trim().to_owned()).filter(|m| !m.is_empty()).unwrap_or_else(|| display.strip_prefix(maker.as_str()).map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()).unwrap_or(display.clone()));
                index.push(json!({
                    "id": 1000 + i,
                    "media_name": media_name,
                    "name": display,
                    "model": model,
                    "maker": maker,
                    "year": Value::Null,
                    "class": entry.and_then(|l| l.class.clone()),
                    "pi": Value::Null,
                    "drive": b.drive,
                    "selectable": true,
                    "has_model": true,
                }));
            }
            Err(e) => skipped.push(format!("{name}: {e:#}")),
        }
    }
    std::fs::write(out.join("index.json"), serde_json::to_vec_pretty(&index)?)?;
    println!("[fm1] {} cars imported, {} skipped", index.len(), skipped.len());
    for s in skipped.iter().take(12) {
        println!("[fm1]   skipped {s}");
    }
    Ok(())
}

/// `key="value"` in a tag's text.
fn attr(s: &str, key: &str) -> Option<String> {
    let k = format!("{key}=\"");
    let i = s.find(&k)? + k.len();
    Some(s[i..i + s[i..].find('"')?].to_owned())
}

/// `key=123` (unquoted, as the livery files write them) or `key="123"`.
fn int_after(s: &str, key: &str) -> Option<u32> {
    let i = s.find(key)? + key.len();
    s[i..].trim_start_matches('"').chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

/// One of the wheel models in `Mega.ca2` (rim + tyre, axle along x, centred): no car -> wheel link is recovered (the files are found by
/// a path hash), so a car takes the model whose tyre size is closest to its own.
struct WheelMesh {
    part: Part,
    radius: f32,
    width: f32,
}

fn load_wheels(mega: &[Entry]) -> Vec<WheelMesh> {
    let mut out = Vec::new();
    for e in mega.iter().filter(|e| fm1::is_car_blob(&e.data)) {
        let Ok(car) = fm1::parse_car(&e.data) else { continue };
        let Some(p) = car.parts.first().filter(|p| p.name.to_ascii_lowercase().starts_with("wheel")) else { continue };
        let (mut materials, mut runs) = (Vec::<String>::new(), Vec::new());
        for s in p.subparts.iter().filter(|s| s.lod == 1 && !s.indices.is_empty()) {
            // The motion-blur overlay discs are not geometry.
            let name = match fm1::MATERIALS.get(s.material as usize).copied().unwrap_or("None") {
                "BlurredRim" | "BlurredRimChrome" => continue,
                "Tire" | "Tire2" => "rubber_trim",
                "InnerRim" => "black",
                _ => "chrome",
            };
            let mi = materials.iter().position(|m| m == name).unwrap_or_else(|| {
                materials.push(name.to_owned());
                materials.len() - 1
            });
            runs.push(Run { material: mi, lod: 1, indices: s.indices.clone() });
        }
        if runs.is_empty() {
            continue;
        }
        let vertices: Vec<Vertex> = p.vertices.iter().map(|v| Vertex { position: [v.position[0], v.position[1], -v.position[2]], uv: v.uv }).collect();
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &vertices {
            for k in 0..3 {
                mn[k] = mn[k].min(v.position[k]);
                mx[k] = mx[k].max(v.position[k]);
            }
        }
        out.push(WheelMesh { radius: (mx[1] - mn[1]) / 2.0, width: mx[0] - mn[0], part: Part { name: "wheel".into(), centre: Some([0.0; 3]), vertices, materials, runs } });
    }
    out
}

/// The wheel closest to a tyre (`radius`, `width` in metres); among equally close ones the `pick`-th, so cars of one size do not all share one design.
fn pick_wheel(wheels: &[WheelMesh], radius: f64, width: f64, pick: usize) -> Option<usize> {
    let cost = |w: &WheelMesh| (w.radius as f64 - radius).abs() + (w.width as f64 - width).abs() * 0.5;
    let best = wheels.iter().map(cost).fold(f64::MAX, f64::min);
    if best > 0.03 {
        return None;
    }
    let near: Vec<usize> = (0..wheels.len()).filter(|i| cost(&wheels[*i]) <= best + 0.004).collect();
    near.get(pick % near.len().max(1)).copied()
}

struct Built {
    physics: Value,
    drive: &'static str,
}

/// The FM2-style material name the engine's car styling knows each FM1 material by.
fn material_name(id: u32) -> &'static str {
    match fm1::MATERIALS.get(id as usize).copied().unwrap_or("None") {
        "Body" => "body",
        "Window" => "window",
        "TailLights" => "tail_light",
        "HeadLights" => "head_light",
        "ReverseLights" => "reverse_light",
        "WhiteTailLights" => "reverse_light",
        "TailLightsOn" => "tail_light_on",
        "BlackGlass" => "black_glass",
        "RedGlass" => "red_glass",
        "Chrome" | "RimChrome" | "InnerRimChrome" | "BlurredRimChrome" => "chrome",
        "Black" | "Number" | "None" => "black",
        "Interior" => "interior",
        "Frame" => "frame",
        "Matte" => "matte_colors",
        "CarbonFiber" => "carbon_fiber",
        "Plastic" => "dullplastic",
        "Plastic2" | "Grille2" | "Misc2" => "plastic2",
        "RubberTrim" => "rubber_trim",
        "NamePlate" | "Badge" => "emblem",
        "Misc" => "misc",
        "HeadlightGlass" => "lights_gls_head",
        "Reflectors" => "reflector",
        "Grille1" => "grille1",
        _ => "misc",
    }
}

/// Materials whose uv land on the detail atlas (`matsurvey`: 94-100 % of their vertices on opaque texels; the body paint, glass, carbon
/// fibre and the emblems do not).
const ATLAS_MATERIALS: [&str; 14] = ["tail_light", "head_light", "reverse_light", "tail_light_on", "interior", "frame", "matte_colors", "misc", "black", "dullplastic", "plastic2", "rubber_trim", "reflector", "grille1"];

/// Lower-case part name without the `_LOD1` suffix, as the car XML's `Parts P0="bumperFa_LOD"` spells it.
fn base(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    n.strip_suffix('1').filter(|s| s.ends_with("_lod")).map(str::to_owned).unwrap_or(n)
}

/// Stock part names (the first variant of every kit family) and every kit part name (all variants).
fn kit_parts(car: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut stock, mut all) = (BTreeSet::new(), BTreeSet::new());
    let up = &car["Car"]["Upgrades"];
    for fam in ["FrontBumper", "RearBumper", "Wing", "SideSkirt", "Hood", "Mirror", "Headlight", "RearLense", "Antenna"] {
        let Some(o) = up[fam].as_object() else { continue };
        for (k, v) in o {
            if !k.starts_with('P') {
                continue;
            }
            for (pk, pv) in v["Parts"].as_object().into_iter().flatten() {
                if pk.starts_with('P') {
                    if let Some(s) = pv.as_str() {
                        all.insert(s.to_ascii_lowercase());
                        if k == "P0" {
                            stock.insert(s.to_ascii_lowercase());
                        }
                    }
                }
            }
        }
    }
    (stock, all)
}

/// The renderable stock parts of a car in the engine's frame (z negated: front is -z), as `carbin_fm2` parts so the FM2 glTF writer can take them.
fn stock_parts(car: &fm1::Car, xml: &Value) -> Vec<Part> {
    let (stock, all) = kit_parts(xml);
    let mut out = Vec::new();
    for p in &car.parts {
        let n = p.name.to_ascii_lowercase();
        let b = base(&p.name);
        let skip = n.chars().next().is_some_and(|c| c.is_ascii_digit()) // brake calipers / rotors
            || n.contains("damage") || n.starts_with("driver") || n.starts_with("cage") || n.starts_with("nocage") || n.ends_with("race");
        let keep = !skip && (stock.contains(&b) || !all.contains(&b));
        if !keep {
            continue;
        }
        let (mut materials, mut runs) = (Vec::<String>::new(), Vec::new());
        for s in p.subparts.iter().filter(|s| s.lod == 1 && !s.indices.is_empty()) {
            let name = material_name(s.material).to_owned();
            let mi = materials.iter().position(|m| *m == name).unwrap_or_else(|| {
                materials.push(name);
                materials.len() - 1
            });
            runs.push(Run { material: mi, lod: 1, indices: s.indices.clone() });
        }
        if runs.is_empty() {
            continue;
        }
        let vertices = p.vertices.iter().map(|v| Vertex { position: [v.position[0], v.position[1], -v.position[2]], uv: v.uv }).collect();
        out.push(Part { name: p.name.clone(), centre: Some([0.0; 3]), vertices, materials, runs });
    }
    out
}

fn build_car(cab: &[u8], folder: &[(String, String)], car_xml: &Value, engine: Option<&[(String, String)]>, donors: &[WheelDonor], wheels: &[WheelMesh], index: i64, media: &str, dir: &Path) -> Result<Built> {
    let entries: Vec<Entry> = fm1::archive(cab).map_err(|e| anyhow::anyhow!("cab: {e}"))?;
    let blob = entries.iter().find(|e| fm1::is_car_blob(&e.data)).context("no car model in the cab")?;
    let car = fm1::parse_car(&blob.data).map_err(|e| anyhow::anyhow!("car blob: {e}"))?;
    let hubs_game = car.hubs.context("no wheel hubs (Attribs)")?;
    let parts = stock_parts(&car, car_xml);
    anyhow::ensure!(!parts.is_empty(), "no usable parts");

    // ---- tables
    let up = &car_xml["Car"]["Upgrades"];
    let level = |k: &str| num(&up[k]["StockLevel"]) as usize;
    let maxdata = folder.iter().find(|(k, _)| k == "MAXData").map(|(_, t)| xml(t)["MAXData"].clone()).unwrap_or(Value::Null);
    // The stock level's file, or the nearest lower level that is not a placeholder (some L3 files carry `Mass="0"`).
    let stock_file = |kind: &str, root: &str, ok: &dyn Fn(&Value) -> bool| -> Value {
        (0..=level(kind).min(3)).rev().filter_map(|l| nth(folder, kind, l)).map(|t| xml(t)[root].clone()).find(|v| ok(v)).unwrap_or(Value::Null)
    };
    let weight = stock_file("Weight", "Weight", &|v| num(&v["Mass"]) > 0.0);
    let trans = stock_file("Transmission", "Transmission", &|v| num(&v["GearRatios"]["v1"]) > 0.0);
    let nonup = folder.iter().find(|(k, _)| k == "NonUpgradeData").map(|(_, t)| xml(t)["NonUpgradeData"].clone()).unwrap_or(Value::Null);
    anyhow::ensure!(num(&weight["Mass"]) > 0.0, "no Weight file");
    let mass = f(&weight, "Mass");
    let drive_name = nonup["Drivetrain"]["Type"].as_str().unwrap_or("RWD");
    let (drive, drive_id) = match drive_name {
        "FWD" => ("FWD", 1),
        "AWD" | "4WD" => ("AWD", 3),
        _ => ("RWD", 2),
    };
    // The engine: stock tuning level from the engine folder's own `Upgrades` file, then that level's torque curve.
    let (mut tq_scale, mut max_rpm, mut samples, mut redline, mut stall, mut start) = (0.0, 0.0, Vec::<f64>::new(), 7000.0, 800.0, 3500.0);
    let (mut engine_mass, mut engine_inertia, mut tank) = (150.0, 0.2, 60.0);
    if let Some(eng) = engine {
        let lvl = eng.iter().find(|(k, t)| k == "Engine" && t.contains("<Upgrades>")).and_then(|(_, t)| int_after(t, "<EngineTuning StockLevel=")).unwrap_or(0) as usize;
        if let Some(d) = eng.iter().find(|(k, t)| k == "Engine" && t.contains("<Data Mass=")).map(|(_, t)| xml(t)["Engine"]["Data"].clone()) {
            engine_mass = f(&d, "Mass").max(1.0);
            engine_inertia = f(&d, "Inertia");
            tank = f(&d, "GasTankSize");
        }
        let tuned = (0..=lvl.min(3)).rev().filter_map(|l| nth(eng, "EngineTuning", l)).map(xml).find(|t| (0..128).any(|i| num(&t["EngineTuning"]["TorqueCurves"]["FullThrottle"][format!("v{i}")]) > 0.0));
        if let Some(t) = tuned {
            let et = &t["EngineTuning"];
            let outer = et["TorqueScale"].as_f64().unwrap_or(1.0);
            let full = &et["TorqueCurves"]["FullThrottle"];
            let n = (num(&et["TorqueCurves"]["NumCurveValuesToRead"]) as usize).clamp(2, 128);
            samples = (0..n).map(|i| full[&format!("v{i}")].as_f64().unwrap_or(0.0)).collect();
            tq_scale = full["TorqueScale"].as_f64().unwrap_or(0.0) * outer;
            max_rpm = f(&et["TorqueCurves"], "MaxRPM");
            redline = f(&et["Data"], "RedlineRPM").max(1000.0);
            stall = f(&et["Data"], "StallRPM");
            start = f(&et["Data"], "StartRPM");
        }
    }
    anyhow::ensure!(!samples.is_empty() && max_rpm > 0.0, "no torque curve");

    // ---- geometry frame (the engine's: -z front; the game's z is forward)
    let tire = &up["Tire"][format!("L{}", level("Tire"))];
    let sized = |axle: &str| -> (f64, f64) {
        let t = &tire[axle];
        let (w, a, r) = (t["TireWidth"].as_f64().unwrap_or(225.0), t["TireRatio"].as_f64().unwrap_or(40.0), t["WheelDiameter"].as_f64().unwrap_or(17.0));
        (r * 0.0254 * 0.5 + w * 0.001 * a * 0.01, w * 0.001)
    };
    let ((rf, wf), (rr, wr)) = (sized("Fronts"), sized("Rears"));
    let h = hubs_game.map(|h| [h[0] as f64, h[1] as f64, -(h[2] as f64)]);
    // `Attribs` order: front left, front right, rear right, rear left.
    let hubs = [("LF", h[0]), ("RF", h[1]), ("LR", h[3]), ("RR", h[2])];
    let wheelbase = ((h[0][2] + h[1][2]) - (h[2][2] + h[3][2])).abs() / 2.0;
    let centre = {
        let c = &maxdata["Misc"]["BottomCenterWheelbasePos"];
        [f(c, "x"), f(c, "y"), -f(c, "z")]
    };
    let (track_f, track_r) = (f(&maxdata["Misc"], "FrontTrackOuter").max(1.2), f(&maxdata["Misc"], "RearTrackOuter").max(1.2));
    let (ride_f, ride_r) = (f(&maxdata["Misc"], "FrontStockRideHeight"), f(&maxdata["Misc"], "RearStockRideHeight"));

    // ---- paint: the first livery's colour
    let livery = entries.iter().filter_map(|e| std::str::from_utf8(&e.data).ok()).find(|t| t.trim_start().starts_with("<livery>"));
    let rgb8 = livery.and_then(|t| Some((int_after(t, " red=")?, int_after(t, " green=")?, int_after(t, " blue=")?))).map(|(r, g, b)| (r.min(255) << 16) | (g.min(255) << 8) | b.min(255));
    let metallic = livery.is_some_and(|t| t.contains("\\metallic.xml") && !t.contains("nonmetallic"));
    let paint: Colour = match rgb8 {
        Some(v) => [(v >> 16) & 255, (v >> 8) & 255, v & 255].map(|c| (c as f32 / 255.0).powf(2.2)),
        None => PALETTE[index as usize % PALETTE.len()],
    };
    let colors: Vec<Value> = entries
        .iter()
        .filter_map(|e| std::str::from_utf8(&e.data).ok())
        .filter(|t| t.trim_start().starts_with("<livery>"))
        .filter_map(|t| {
            let rgb = (int_after(t, " red=")?.min(255) << 16) | (int_after(t, " green=")?.min(255) << 8) | int_after(t, " blue=")?.min(255);
            Some(json!({"Sequence": 0, "RGB": rgb, "Metallic": i32::from(t.contains("\\metallic.xml") && !t.contains("nonmetallic"))}))
        })
        .enumerate()
        .map(|(i, mut c)| {
            c["Sequence"] = json!(i);
            c
        })
        .collect();

    // ---- donor
    let donor = pick(donors, mass, drive_id, 2000.0);
    let mut dphys: Value = serde_json::from_slice(&std::fs::read(donor.dir.join("physics.json"))?)?;
    let dmodel: Value = serde_json::from_slice(&std::fs::read(donor.dir.join("model.json"))?)?;
    let dgltf: Value = serde_json::from_slice(&std::fs::read(donor.dir.join("model.gltf"))?)?;
    let dbin = std::fs::read(donor.dir.join("model.bin"))?;

    std::fs::create_dir_all(dir)?;
    let mut g = Gltf::default();
    let mut kids = Vec::new();
    let (mut bmin, mut bmax) = ([f32::MAX; 3], [f32::MIN; 3]);
    // Textures: the detail atlas on the materials that use it, a transparent paint texture (the engine's paint shading draws lerp(paint,
    // atlas, atlas alpha)), and a cut-out copy of the atlas for the emblems.
    let atlas = entries.iter().filter_map(|e| fm1::xpr_decode(&e.data)).max_by_key(|t| t.width * t.height);
    if let Some(mut a) = atlas {
        let mut cut = a.rgba.clone();
        for px in cut.chunks_exact_mut(4) {
            if px[3] < 128 {
                px[3] = 0;
            }
        }
        crate::model::write_png(&dir.join("emblem.png"), a.width, a.height, &cut)?;
        let te = g.add_texture("emblem.png");
        g.mat_texture.insert("emblem".into(), te);
        g.cutout.insert("emblem".into());
        // Texels with no alpha carry stray colour; the game draws the paint / base material there, the glTF ignores the alpha.
        for px in a.rgba.chunks_exact_mut(4) {
            if px[3] < 128 {
                px.copy_from_slice(&[22, 22, 22, 255]);
            }
        }
        crate::model::write_png(&dir.join("atlas.png"), a.width, a.height, &a.rgba)?;
        let t = g.add_texture("atlas.png");
        for m in ATLAS_MATERIALS {
            g.mat_texture.insert(m.to_owned(), t);
        }
    }
    crate::model::write_png(&dir.join("paint.png"), 1, 1, &[0, 0, 0, 0])?;
    let paint_tex = g.add_texture("paint.png");
    g.mat_texture.insert("body".into(), paint_tex);
    g.paint_metallic = metallic;

    let refs: Vec<&Part> = parts.iter().collect();
    let flip = signed_volume(&refs) < 0.0;
    for p in &parts {
        kids.extend(g.part(p, [0.0; 3], flip, paint, &mut bmin, &mut bmax));
    }

    // Wheels: an FM1 wheel model of the tyre's size (see `WheelMesh`); the FH1 donor's meshes scaled to the tyre size when none is close.
    let mut copier = Copier::new(&donor.dir, dir);
    let mut own: [Option<usize>; 2] = [pick_wheel(wheels, rf, wf, index as usize), pick_wheel(wheels, rr, wr, index as usize)];
    if own[0].is_some() && own[1].is_none() {
        own[1] = own[0];
    }
    let mut meshes: std::collections::HashMap<usize, Option<usize>> = Default::default();
    let mut used_own = false;
    for (i, (corner, hub)) in hubs.iter().enumerate() {
        let front = i < 2;
        let mirrored = i % 2 == 1;
        if let Some(wi) = own[usize::from(!front)] {
            let mesh = *meshes.entry(wi).or_insert_with(|| {
                let w = &wheels[wi];
                let flip_w = signed_volume(&[&w.part]) < 0.0;
                let (mut a, mut b) = ([0.0; 3], [0.0; 3]);
                g.part_mesh(&w.part, [0.0; 3], flip_w, paint, &mut a, &mut b)
            });
            if let Some(mesh) = mesh {
                let (r, w) = if front { (rf, wf) } else { (rr, wr) };
                let sr = (r / wheels[wi].radius as f64).clamp(0.5, 2.0);
                let sx = (w / wheels[wi].width as f64).clamp(0.5, 2.0);
                let child = g.node(json!({"mesh": mesh}));
                kids.push(g.node(json!({"name": format!("wheel_{corner}"), "children": [child], "translation": hub, "scale": [if mirrored { -sx } else { sx }, sr, sr]})));
                used_own = true;
                continue;
            }
        }
        let (r, dr) = if front { (rf, f(&dmodel["axles"][0], "tyre_radius")) } else { (rr, f(&dmodel["axles"][1], "tyre_radius")) };
        let s = if dr > 0.05 { r / dr } else { 1.0 };
        let wnode = dgltf["nodes"].as_array().and_then(|a| a.iter().find(|n| n["name"].as_str() == Some(&format!("wheel_{corner}")))).context("donor wheel node")?;
        let mut children = Vec::new();
        for c in wnode["children"].as_array().into_iter().flatten() {
            let cn = &dgltf["nodes"][c.as_u64().unwrap_or(0) as usize];
            let Some(mesh) = cn["mesh"].as_u64() else { continue };
            let m = copier.mesh(&mut g, &dgltf, &dbin, mesh as usize)?;
            children.push(g.node(json!({"mesh": m})));
        }
        let mut n = json!({"name": format!("wheel_{corner}"), "children": children, "translation": hub});
        n["scale"] = json!([if mirrored { -s } else { s }, s, s]);
        kids.push(g.node(n));
    }
    let mesh_offset = [-centre[0], -centre[1], -centre[2]];
    let root = g.node(json!({"name": media, "children": kids, "translation": mesh_offset}));
    let triangles = g.triangles;
    g.write(dir, root)?;

    // ---- model.json
    let cockpit_eye = parts.iter().find(|p| p.name.eq_ignore_ascii_case("steering_wheel")).map(|p| {
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &p.vertices {
            for k in 0..3 {
                mn[k] = mn[k].min(v.position[k]);
                mx[k] = mx[k].max(v.position[k]);
            }
        }
        [(mn[0] + mx[0]) / 2.0, (mn[1] + mx[1]) / 2.0 + 0.21, (mn[2] + mx[2]) / 2.0 + 0.51]
    });
    let rim = |axle: &str| tire[axle]["WheelDiameter"].as_f64().unwrap_or(17.0) * 0.0254 * 0.5;
    let model = json!({
        "media_name": media,
        "forward": "-Z",
        "wheel_radius": rf,
        "wheel_width": wf,
        "lod0_stub_sections": [],
        "kit": {},
        "mesh_offset": mesh_offset,
        "paint": {"rgb": rgb8.unwrap_or_else(|| paint_rgb(paint)), "metallic": metallic, "sequence": 1},
        "rim": donor.media,
        "axles": [
            {"tyre_radius": rf, "tyre_width": wf, "rim_radius": rim("Fronts")},
            {"tyre_radius": rr, "tyre_width": wr, "rim_radius": rim("Rears")},
        ],
        "hubs": hubs.iter().map(|(c, p)| json!({"corner": c, "position": [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]]})).collect::<Vec<_>>(),
        "winding_flipped": flip,
        "triangles": triangles,
        "imported_from": "fm1",
        "wheels_from": if used_own { "fm1" } else { donor.media.as_str() },
        "cockpit_eye": cockpit_eye.map(|e| [e[0] as f64 - centre[0], e[1] as f64 - centre[1], e[2] as f64 - centre[2]]),
    });
    std::fs::write(dir.join("model.json"), serde_json::to_vec_pretty(&model)?)?;

    // ---- physics.json: the donor's, with everything FM1 has a table for replaced.
    let mut gears: Vec<f64> = (0..8).map(|i| f(&trans["GearRatios"], &format!("v{i}"))).collect();
    gears.resize(9, -1.0);
    let num_gears = gears[1..8].iter().filter(|r| **r > 0.0).count();
    let (mut best, mut best_rpm) = (0.0f64, 0.0f64);
    for (i, s) in samples.iter().enumerate() {
        let rpm = i as f64 * max_rpm / (samples.len() - 1) as f64;
        if s * rpm > best {
            best = s * rpm;
            best_rpm = rpm;
        }
    }
    let p = &mut dphys;
    p["id"] = json!(1000 + index);
    p["media_name"] = json!(media);
    p["imported_from"] = json!("fm1");
    p["donor"] = json!(donor.media);
    let c = p["car"].as_object_mut().context("donor car")?;
    c.insert("Id".into(), json!(1000 + index));
    c.insert("MediaName".into(), json!(media));
    c.insert("DisplayName".into(), json!(media));
    c.insert("CurbWeight".into(), json!(mass / 100.0));
    c.insert("WeightDistribution".into(), json!(f(&weight, "CM_BackFront")));
    c.insert("DriveTypeID".into(), json!(drive_id));
    c.insert("NumGears".into(), json!(num_gears));
    c.insert("FrontTireWidthMM".into(), json!(tire["Fronts"]["TireWidth"]));
    c.insert("FrontTireAspect".into(), json!(tire["Fronts"]["TireRatio"]));
    c.insert("FrontWheelDiameterIN".into(), json!(tire["Fronts"]["WheelDiameter"]));
    c.insert("RearTireWidthMM".into(), json!(tire["Rears"]["TireWidth"]));
    c.insert("RearTireAspect".into(), json!(tire["Rears"]["TireRatio"]));
    c.insert("RearWheelDiameterIN".into(), json!(tire["Rears"]["WheelDiameter"]));
    c.insert("SimPeakAngVel".into(), json!(best_rpm * std::f64::consts::TAU / 60.0));
    c.insert("SimPeakPower".into(), json!(best * tq_scale * std::f64::consts::TAU / 60.0));
    c.insert("SimRedlineAngVel".into(), json!(redline * std::f64::consts::TAU / 60.0));
    if let Some(top) = gears[1..8].iter().copied().filter(|r| *r > 0.0).reduce(f64::min) {
        let fd = f(&trans["Data"], "FinalDriveRatio");
        if fd > 0.0 {
            c.insert("SimTopSpeed".into(), json!(redline * std::f64::consts::TAU / 60.0 / (fd * top) * rr * 0.85));
        }
    }
    p["body"]["Id"] = json!(1000 + index);
    p["body"]["ModelWheelbase"] = json!(wheelbase);
    p["body"]["ModelFrontTrackOuter"] = json!(track_f);
    p["body"]["ModelRearTrackOuter"] = json!(track_r);
    p["body"]["ModelFrontStockRideHeight"] = json!(ride_f);
    p["body"]["ModelRearStockRideHeight"] = json!(ride_r);
    p["body"]["BottomCenterWheelbasePosx"] = json!(centre[0]);
    p["body"]["BottomCenterWheelbasePosy"] = json!(centre[1]);
    p["body"]["BottomCenterWheelbasePosZ"] = json!(centre[2]);
    let wheel_x = track_f.max(track_r) / 2.0;
    let ymin = (-ride_f.min(ride_r)).min(bmin[1] as f64 - centre[1]);
    for (k, v) in [
        ("PristineBoundingBoxMinX", (bmin[0] as f64 - centre[0]).min(-wheel_x)), ("PristineBoundingBoxMaxX", (bmax[0] as f64 - centre[0]).max(wheel_x)),
        ("PristineBoundingBoxMinY", ymin), ("PristineBoundingBoxMaxY", bmax[1] as f64 - centre[1]),
        ("PristineBoundingBoxMinZ", bmin[2] as f64 - centre[2]), ("PristineBoundingBoxMaxZ", bmax[2] as f64 - centre[2]),
    ] {
        p["body"][k] = json!(v);
    }
    let sp = &mut p["stock_parts"];
    if let Some(o) = sp.as_object_mut() {
        o.retain(|k, _| !(k.contains("Turbo") || k.ends_with("CSC") || k.ends_with("DSC")));
    }
    sp["List_UpgradeCarBodyWeight"]["Mass"] = json!(mass);
    sp["List_UpgradeCarBodyWeight"]["InitialMass"] = json!(mass);
    sp["List_UpgradeCarBodyWeight"]["CMHeight"] = weight["CM_Height"].clone();
    sp["List_UpgradeCarBodyWeight"]["CMBackFront"] = weight["CM_BackFront"].clone();
    sp["List_UpgradeCarBodyWeight"]["CMLeftRight"] = weight["CM_LeftRight"].clone();
    sp["List_UpgradeCarBodyWeight"]["BlockDimX"] = weight["BlockDim_x"].clone();
    sp["List_UpgradeCarBodyWeight"]["BlockDimY"] = weight["BlockDim_y"].clone();
    sp["List_UpgradeCarBodyWeight"]["BlockDimZ"] = weight["BlockDim_z"].clone();
    sp["List_UpgradeDrivetrainTransmission"]["MomentInertia"] = trans["Data"]["Inertia"].clone();
    sp["List_UpgradeDrivetrainTransmission"]["GearShiftTime"] = trans["Data"]["GearShiftTime"].clone();
    sp["List_UpgradeDrivetrainTransmission"]["FinalDriveRatio"] = trans["Data"]["FinalDriveRatio"].clone();
    sp["List_UpgradeDrivetrainTransmission"]["NumGears"] = json!(num_gears);
    for (i, r) in gears.iter().enumerate() {
        sp["List_UpgradeDrivetrainTransmission"][format!("GearRatio{i}")] = json!(r);
    }
    for (k, v) in [("RedlineRPM", json!(redline)), ("StallRPM", json!(stall)), ("StartRPM", json!(start)), ("TorqueCurveMaxRPM", json!(max_rpm)), ("NumRPMEntriesArray", json!(samples.len()))] {
        sp["List_UpgradeEngineCamshaft"][k] = v;
    }
    sp["List_UpgradeCarBodyTireWidthFront"]["FrontTireWidth"] = tire["Fronts"]["TireWidth"].clone();
    sp["List_UpgradeCarBodyTireWidthRear"]["RearTireWidth"] = tire["Rears"]["TireWidth"].clone();
    sp["List_UpgradeRimSizeFront"]["FrontWheelDiameter"] = tire["Fronts"]["WheelDiameter"].clone();
    sp["List_UpgradeRimSizeRear"]["RearWheelDiameter"] = tire["Rears"]["WheelDiameter"].clone();
    p["colors"] = Value::Array(if colors.is_empty() { vec![json!({"Sequence": 0, "RGB": rgb8.unwrap_or(0x808080), "Metallic": i32::from(metallic)})] } else { colors });
    p["torque_curve"] = json!({"id": 1000 + index, "torque_scale_nm": tq_scale, "zero_throttle_nm": Value::Null, "max_rpm": max_rpm, "samples": samples});
    p["engine"]["EngineMass-kg"] = json!(engine_mass);
    p["engine"]["MomentInertia"] = json!(engine_inertia);
    p["engine"]["GasTankSize"] = json!(tank);

    // MAXData (collision spheres, bounding box) into the engine's frame (z negated), with the FM3-style sphere flags added (1 = 0.05 m corner
    // marker, 2 = body sphere).
    let mut md = maxdata;
    if !md.is_null() {
        let neg = |v: &mut Value, k: &str| {
            if let Some(x) = v[k].as_f64() {
                v[k] = json!(-x);
            }
        };
        if let Some(sph) = md["Collision"]["CollSpheres"].as_object_mut() {
            for (k, v) in sph.iter_mut() {
                if k.starts_with("Sphere") {
                    neg(v, "PosZ");
                    if v.get("Flags").is_none() {
                        v["Flags"] = json!(if v["Radius"].as_f64().unwrap_or(1.0) <= 0.06 { 1.0 } else { 2.0 });
                    }
                }
            }
        }
        if let Some(bb) = md["Collision"]["BoundingBox"].as_object_mut() {
            let (lo, hi) = (bb.get("MinZ").and_then(Value::as_f64), bb.get("MaxZ").and_then(Value::as_f64));
            if let (Some(lo), Some(hi)) = (lo, hi) {
                bb.insert("MinZ".into(), json!(-hi));
                bb.insert("MaxZ".into(), json!(-lo));
            }
        }
        let c = &mut md["Misc"]["BottomCenterWheelbasePos"];
        neg(c, "z");
        p["maxdata"] = md;
    }
    Ok(Built { physics: dphys, drive })
}
