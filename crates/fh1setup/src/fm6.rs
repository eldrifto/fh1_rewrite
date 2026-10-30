//! `fh1setup import-fm6 <FM6 Mount folder> [--data <dir>] [--cars A,B]`: Forza Motorsport 6 (Xbox One) cars into the
//! ACTIVE install, as `imported/fm6/cars/<MediaName>/` like the other imported games (docs/DESIGN.md "FM6 IDx", step 5).
//! The game folder is only read. Without `--cars` every car that has a zip is tried; a car that fails is logged and
//! skipped.
//!
//! WHAT IS GAME DATA: the body, interior, lights, glass, wheels and brakes (`.modelbin`, decoded by
//! `fh1_formats::modelbin`), their placement (the skeleton, `Locators.xml`), the rim size (the wheel model's own morph
//! targets, solved for the database's rim), the material of every mesh (`.materialbin` chains through the shared
//! `media/cars/_library`), colour textures (`.swatchbin`, `fh1_formats::swatch`), the paint colour (the car's colour file),
//! and `physics.json` (FM6's `GameDB.slt` through the FH1 builder, `cars::physics_for`).
//!
//! WHAT IS INFERRED (see MODLOG 105 to 108): which material parameter is the base colour (`1f9b6488`, `938926b0`; the
//! paint `c0cb2820`), the UV rect (third of five, as offset + scale), metal and glass looks from shader names, where the
//! shared-library parts go (control arms / brakes: their bone's x, the wheel locator's y and z), rim width = tyre width,
//! the tyre's whole shape (no tyre model exists in the game files) and its rubber colour. No damage, no morph normals,
//! no ambient occlusion maps, no normal maps, no cockpit, no `MAXData` (FM6 has none; the engine falls back to the
//! database's body box).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

use fh1_formats::materialbin::{self, Material, Value as MatValue};
use fh1_formats::{modelbin, swatch, tyre, zip};

use crate::fm2_cars::Gltf;

/// `import-fm6`.
pub fn run() -> Result<()> {
    run_game("fm6")
}

/// `import-fm5`: Forza Motorsport 5 uses the same file family, older versions (DESIGN.md "FM5 cars").
pub fn run_fm5() -> Result<()> {
    run_game("fm5")
}

/// `import-fh2xb1`: Forza Horizon 2 for Xbox One, FM5's file versions; `imported/fh2xb1` (`imported/fh2` is the
/// Xbox 360 FH2 import) (DESIGN.md "FH2 (Xbox One) cars").
pub fn run_fh2xb1() -> Result<()> {
    run_game("fh2xb1")
}

fn run_game(game: &str) -> Result<()> {
    let mut it = std::env::args().skip(2);
    let (mut source, mut data, mut only) = (None::<PathBuf>, PathBuf::from("data"), None::<Vec<String>>);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().context("--data needs a path")?.into(),
            "--cars" => only = Some(it.next().context("--cars needs A,B")?.split(',').map(|s| s.trim().to_ascii_lowercase()).collect()),
            _ => source = Some(PathBuf::from(a)),
        }
    }
    let disc = source.with_context(|| format!("usage: fh1setup import-{game} <{} Mount folder> [--data <dir>] [--cars A,B]", game.to_uppercase()))?;
    anyhow::ensure!(crate::fm4::gamedb(&disc).exists(), "{}: no game database (media/db/gamedb.slt or media/stripped/gamedbRC.slt): not an {} Mount folder", disc.display(), game.to_uppercase());
    let inst: Value = serde_json::from_slice(&std::fs::read(data.join("installation.json")).context("installation.json: run fh1setup on the FH1 disc first")?)?;
    let private = data.join("installations").join(inst["id"].as_str().context("installation.json id")?).join("assets/private");
    let root = private.join("imported").join(game);
    std::fs::create_dir_all(&root)?;
    crate::fh2::swap(&root.join("cars"), |stage| cars(game, &disc, &private, stage, only.as_deref()))?;
    println!("[{game}] done: {}", root.display());
    Ok(())
}

fn cars(game: &str, disc: &Path, private: &Path, out: &Path, only: Option<&[String]>) -> Result<()> {
    let db = Connection::open_with_flags(crate::fm4::gamedb(disc), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let library = Library::load(&[disc.join("media/cars/_library"), disc.join("media/_library")])?;
    println!("[{game}] shared library: {} zips, {} entries", library.zips.len(), library.entries.len());
    let mut all = crate::cars::rows(&db, "SELECT * FROM Data_Car ORDER BY Id", [])?;
    if let Some(o) = only {
        all.retain(|c| c["MediaName"].as_str().is_some_and(|m| o.iter().any(|x| x.eq_ignore_ascii_case(m))));
    }
    let mut index = Vec::new();
    for car in &all {
        let media = car["MediaName"].as_str().context("MediaName")?.to_owned();
        let zip_path = find_ci(&disc.join("media/cars"), &format!("{media}.zip"));
        let Some(zip_path) = zip_path else {
            println!("[{game}] {media}: no media/cars/{media}.zip, skipped");
            continue;
        };
        let dir = out.join(&media);
        std::fs::create_dir_all(&dir)?;
        let physics = match crate::cars::physics_for(&db, car, disc).and_then(|p| fm6_tyre_curves(&db, p)) {
            Ok(p) => p,
            Err(e) => {
                println!("[{game}] {media}: physics failed ({e:#}), skipped");
                std::fs::remove_dir_all(&dir).ok();
                continue;
            }
        };
        std::fs::write(dir.join("physics.json"), serde_json::to_vec_pretty(&physics)?)?;
        let db_paint = physics["colors"].as_array().and_then(|c| c.first()).and_then(|c| c["RGB"].as_i64()).map(|v| v as u32);
        let has_model = match build_model(game, &zip_path, &library, car, db_paint, &media, &dir) {
            Ok((report, bbox, body)) => {
                println!("[{game}] {media}: {report}");
                write_box(&dir.join("physics.json"), bbox, body)?;
                true
            }
            Err(e) => {
                println!("[{game}] {media}: no model ({e:#})");
                false
            }
        };
        index.push(json!({
            "id": car["Id"], "media_name": media, "year": car["Year"], "display_name": car["DisplayName"], "make_id": car["MakeID"],
            "class_id": car["ClassID"], "drive_type_id": car["DriveTypeID"], "performance_index": car["PerformanceIndex"], "has_model": has_model,
        }));
    }
    std::fs::write(out.join("index.json"), serde_json::to_vec_pretty(&index)?)?;
    crate::fm4::finish(disc, private, out, game)
}

/// FM6 splits the tyre's longitudinal friction curve into `FrictionMultiCurveLongitudinalAccelID` and `...BrakeID`; FH1 (and
/// the engine) have one, `friction_longitudinal`. It gets the acceleration curve (INFERRED: the engine has one curve for both);
/// the braking curve is kept as `friction_longitudinal_brake` for later use.
fn fm6_tyre_curves(db: &Connection, mut p: Value) -> Result<Value> {
    let id = |k: &str| p["tires"]["compound"].get(k).and_then(Value::as_i64);
    let (accel, brake) = (id("FrictionMultiCurveLongitudinalAccelID"), id("FrictionMultiCurveLongitudinalBrakeID"));
    if p["tires"].is_object() && p["tires"]["friction_longitudinal"].is_null() {
        p["tires"]["friction_longitudinal"] = crate::cars::multi_curve(db, accel)?;
        p["tires"]["friction_longitudinal_brake"] = crate::cars::multi_curve(db, brake)?;
    }
    // FM6 stores TireFricWidth0/1 and TireFricScale0/1 as 0 (it has DiameterAffectFriction* columns instead, which FH1's
    // model does not know). FH1 multiplies grip by the width scale, so 0 meant no grip at all (MODLOG 110). Zeros become
    // the engine's own neutral defaults (scale 1.0 over 150..650 mm); FM6's diameter rule is not ported (INFERRED).
    let c = &mut p["tires"]["compound"];
    if c.is_object() && ["TireFricScale0", "TireFricScale1"].iter().all(|k| c[*k].as_f64() == Some(0.0)) {
        for (k, v) in [("TireFricWidth0", 150.0), ("TireFricWidth1", 650.0), ("TireFricScale0", 1.0), ("TireFricScale1", 1.0)] {
            c[k] = json!(v);
        }
    }
    Ok(p)
}

/// FM5 / FM6 ship no MAXData, and FM6's `Data_CarBody.PristineBoundingBox*` are all 0, so the engine's cameras (chase
/// radius, bumper and look-back mounts) collapsed to the middle of the car (MODLOG 111). Two boxes of the placed model go
/// in, game frame, model origin (INFERRED: the games' own camera and contact boxes are not in their files):
/// - `body.PristineBoundingBox*` (replaced when all zero, else kept): the BODY only. The engine also places its fallback
///   body contact points from this box (fh1-engine data.rs `bbox_contact_points`, "low" = box bottom + 0.1 m); with the
///   wheels in it the bottom was the tyres' contact patch, a contact point rested on the ground and took weight off the
///   E21's front tyres (MODLOG 114).
/// - `maxdata.Collision.BoundingBox` (added when there is no MAXData): the WHOLE car, for the car-mounted cameras.
fn write_box(phys: &Path, whole: [[f32; 3]; 2], body: [[f32; 3]; 2]) -> Result<()> {
    let mut p: Value = serde_json::from_slice(&std::fs::read(phys)?)?;
    let keys = ["MinX", "MinY", "MinZ", "MaxX", "MaxY", "MaxZ"];
    let vals = |b: [[f32; 3]; 2]| [b[0][0], b[0][1], b[0][2], b[1][0], b[1][1], b[1][2]];
    if keys.iter().all(|k| p["body"][format!("PristineBoundingBox{k}")].as_f64().unwrap_or(0.0) == 0.0) {
        for (k, v) in keys.iter().zip(vals(body)) {
            p["body"][format!("PristineBoundingBox{k}")] = json!(v);
        }
    }
    if p["maxdata"].is_null() {
        let bb: serde_json::Map<String, Value> = keys.iter().zip(vals(whole)).map(|(k, v)| (k.to_string(), json!(v))).collect();
        p["maxdata"] = json!({"Collision": {"BoundingBox": bb}});
    }
    std::fs::write(phys, serde_json::to_vec_pretty(&p)?)?;
    Ok(())
}

fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name)))
}

// ---------------------------------------------------------------------------------------------------------------
// Reading files: the car's zip and the shared library (`media/cars/_library/*.zip`)

/// Every entry of the shared libraries' zips (`media/cars/_library`, then `media/_library`), keyed `family/path` in lower case with `/` (family = the zip's name without
/// `_pri_NN`: `materials`, `textures`), plus a file-name index (texture names carry a GUID, so they are unique).
struct Library {
    zips: Vec<PathBuf>,
    entries: HashMap<String, (usize, zip::Entry)>,
    by_name: HashMap<String, (usize, zip::Entry)>,
    open: RefCell<HashMap<usize, zip::Archive<std::fs::File>>>,
}

impl Library {
    fn load(dirs: &[PathBuf]) -> Result<Self> {
        let mut zips = Vec::new();
        for dir in dirs {
            let mut v: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip"))).collect();
            v.sort();
            zips.extend(v);
        }
        let mut entries = HashMap::new();
        let mut by_name = HashMap::new();
        for (i, z) in zips.iter().enumerate() {
            let stem = z.file_stem().unwrap().to_string_lossy().to_ascii_lowercase();
            let family = stem.split("_pri_").next().unwrap_or(&stem).to_owned();
            let Ok(ar) = zip::Archive::open(z) else { continue };
            for e in &ar.entries {
                let path = e.name.replace('\\', "/").to_ascii_lowercase();
                entries.entry(format!("{family}/{path}")).or_insert((i, e.clone()));
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                by_name.entry(name).or_insert((i, e.clone()));
            }
        }
        Ok(Library { zips, entries, by_name, open: RefCell::new(HashMap::new()) })
    }

    /// `Game:\Media\cars\_library\<family>\<path>` (or `Game:\Media\_library\...`, looked up by file name).
    fn read(&self, game_path: &str) -> Option<Vec<u8>> {
        let p = game_path.replace('\\', "/").to_ascii_lowercase();
        let rest = p.split("/_library/").nth(1)?;
        let hit = self.entries.get(rest).or_else(|| self.by_name.get(rest.rsplit('/').next()?))?;
        let mut open = self.open.borrow_mut();
        let ar = match open.entry(hit.0) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => v.insert(zip::Archive::open(&self.zips[hit.0]).ok()?),
        };
        ar.read(&hit.1).ok()
    }
}

/// The car's own zip, entries keyed by lower-case path with `/`.
struct CarZip {
    ar: RefCell<zip::Archive<std::fs::File>>,
    by_path: HashMap<String, zip::Entry>,
}

impl CarZip {
    fn open(path: &Path) -> Result<Self> {
        let ar = zip::Archive::open(path)?;
        let by_path = ar.entries.iter().map(|e| (e.name.replace('\\', "/").to_ascii_lowercase(), e.clone())).collect();
        Ok(CarZip { ar: RefCell::new(ar), by_path })
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let e = self.by_path.get(&path.replace('\\', "/").to_ascii_lowercase())?;
        self.ar.borrow_mut().read(e).ok()
    }

    /// `Game:\Media\cars\<car>\<path>` -> the entry `<path>`.
    fn read_game(&self, game_path: &str) -> Option<Vec<u8>> {
        let p = game_path.replace('\\', "/").to_ascii_lowercase();
        let rest = p.split("/media/cars/").nth(1)?.split_once('/')?.1;
        self.read(rest)
    }

    fn paths(&self, prefix: &str, ext: &str) -> Vec<String> {
        let mut v: Vec<String> = self.by_path.keys().filter(|k| k.starts_with(prefix) && k.ends_with(ext)).cloned().collect();
        v.sort();
        v
    }
}

fn read_any(car: &CarZip, lib: &Library, game_path: &str) -> Option<Vec<u8>> {
    if game_path.to_ascii_lowercase().replace('\\', "/").contains("/_library/") {
        lib.read(game_path)
    } else {
        car.read_game(game_path)
    }
}

/// A material instance laid over its parent chain (at most 5 deep).
fn resolve(car: &CarZip, lib: &Library, cache: &mut HashMap<String, Option<Material>>, inst: &[u8]) -> Material {
    let mut m = materialbin::parse(inst).unwrap_or_default();
    for _ in 0..5 {
        let Some(parent) = m.parent.clone() else { break };
        let p = cache
            .entry(parent.to_ascii_lowercase())
            .or_insert_with(|| read_any(car, lib, &parent).filter(|d| d.len() >= 20).and_then(|d| materialbin::parse(&d).ok()))
            .clone();
        match p {
            Some(p) => m = m.over(&p),
            None => break,
        }
    }
    m
}

// ---------------------------------------------------------------------------------------------------------------
// The model

const BASE_COLOUR: u32 = 0x1f9b_6488; // textured shaders (INFERRED, MODLOG 105)
const SIMPLE_COLOUR: u32 = 0x9389_26b0; // simple-diffuse shaders (INFERRED)
const PAINT_COLOUR: u32 = 0xc0cb_2820; // the car's colour file (INFERRED from red_IDxNismo)

struct Look {
    name: String,
    colour: [f32; 4],
    metallic: f32,
    roughness: f32,
    texture: Option<usize>,
    blend: bool,
    /// Cut-out: the texture's alpha is an opacity mask.
    mask: bool,
}

/// FM6 car space (front +z) -> the imported-car files (front -z): negate z.
fn to_engine(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[1], -p[2]]
}

/// Adds one mesh (positions and normals already in the engine frame, triangles already in the engine frame's winding).
fn add_mesh(g: &mut Gltf, name: &str, pos: &[[f32; 3]], nrm: &[[f32; 3]], uv: Option<&[[f32; 2]]>, idx: &[u32], material: usize) -> usize {
    let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
    for q in pos {
        for k in 0..3 {
            mn[k] = mn[k].min(q[k]);
            mx[k] = mx[k].max(q[k]);
        }
    }
    let f3 = |v: &[[f32; 3]]| -> Vec<u8> { v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect() };
    let vp = g.view(&f3(pos), Some(34962), None);
    let pa = g.accessor(vp, 5126, pos.len(), "VEC3", Some((mn, mx)));
    let mut attrs = json!({"POSITION": pa});
    if nrm.len() == pos.len() {
        let vn = g.view(&f3(nrm), Some(34962), None);
        attrs["NORMAL"] = json!(g.accessor(vn, 5126, nrm.len(), "VEC3", None));
    }
    if let Some(uv) = uv.filter(|u| u.len() == pos.len()) {
        let b: Vec<u8> = uv.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let vt = g.view(&b, Some(34962), None);
        attrs["TEXCOORD_0"] = json!(g.accessor(vt, 5126, uv.len(), "VEC2", None));
    }
    let ib: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
    let vi = g.view(&ib, Some(34963), None);
    let ia = g.accessor(vi, 5125, idx.len(), "SCALAR", None);
    g.triangles += idx.len() / 3;
    g.meshes.push(json!({"name": name, "primitives": [{"attributes": attrs, "indices": ia, "material": material}]}));
    g.meshes.len() - 1
}

fn add_material(g: &mut Gltf, l: &Look) -> usize {
    let mut m = json!({"name": l.name, "pbrMetallicRoughness": {"baseColorFactor": l.colour, "metallicFactor": l.metallic, "roughnessFactor": l.roughness}, "doubleSided": true});
    if let Some(t) = l.texture {
        m["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": t});
    }
    if l.blend {
        m["alphaMode"] = json!("BLEND");
    } else if l.mask {
        m["alphaMode"] = json!("MASK");
        m["alphaCutoff"] = json!(0.5);
    }
    g.materials.push(m);
    g.materials.len() - 1
}

fn srgb_to_linear(v: u32) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb8(c: f32) -> u32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u32
}

/// The look of one material: paint, colour texture, colour parameter or grey; metal and glass by shader name (INFERRED).
#[allow(clippy::too_many_arguments)]
fn look_for(g: &mut Gltf, mat: &Material, name: &str, paint: [f32; 4], car: &CarZip, lib: &Library, dir: &Path, textures: &mut HashMap<String, Option<(usize, bool, bool)>>) -> Look {
    let shader = mat.shader.clone().unwrap_or_default().to_ascii_lowercase();
    let metal = shader.starts_with("m_") || shader.starts_with("int_m_") || shader.starts_with("rh_m_") || shader.contains("metal");
    let glass = ["glass", "window", "windsheild", "windshield", "gls_", "glm_"].iter().any(|k| shader.contains(k));
    let mut look = Look { name: name.to_owned(), colour: [0.5, 0.5, 0.5, 1.0], metallic: if metal { 1.0 } else { 0.0 }, roughness: if metal { 0.3 } else { 0.6 }, texture: None, blend: false, mask: false };
    if shader.starts_with("carpaint") {
        look.colour = paint;
        look.metallic = 0.0;
        look.roughness = 0.25;
    } else if glass {
        look.colour = [0.05, 0.05, 0.05, 0.35];
        look.roughness = 0.05;
        look.blend = true;
    }
    let colour = [BASE_COLOUR, SIMPLE_COLOUR].iter().find_map(|h| match mat.get(*h) {
        Some(MatValue::Color(c)) => Some(*c),
        _ => None,
    });
    if let Some(c) = colour.filter(|_| !shader.starts_with("carpaint") && !glass) {
        look.colour = [c[0], c[1], c[2], 1.0];
    }
    // Colour texture (`_diff_`) and opacity mask (`_opac_`), not the game's default maps, merged into one RGBA image (the
    // mask resampled to the colour texture's size). A mask without a colour texture is drawn white x the material colour.
    // Both read with UV set 0 (only checked for the colour atlas, MODLOG 105).
    let pick = |kind: &str| {
        mat.textures()
            .find(|t| {
                let t = t.to_ascii_lowercase();
                t.contains(kind) && !t.contains("defaultshader") && !t.contains("defaultch")
            })
            .map(str::to_owned)
    };
    let paint_only = shader.starts_with("carpaint") && !mat.textures().any(|t| t.to_ascii_lowercase().contains("livery"));
    let diff = pick("_diff_").filter(|_| !paint_only);
    let opac = pick("_opac_").filter(|_| !glass);
    if diff.is_none() && opac.is_none() {
        return look;
    }
    let key = format!("{}|{}", diff.as_deref().unwrap_or("").to_ascii_lowercase(), opac.as_deref().unwrap_or("").to_ascii_lowercase());
    let tex = *textures.entry(key).or_insert_with(|| {
        let decode = |p: &str| -> Option<(usize, usize, Vec<u8>)> {
            let s_bytes = read_any(car, lib, p)?;
            let s = swatch::parse(&s_bytes).ok()?;
            let (w, h) = s.mip_pixels(0);
            Some((w, h, s.decode_mip(0).ok()?))
        };
        let d = diff.as_deref().and_then(decode);
        let o = opac.as_deref().and_then(decode);
        let (w, h, mut rgba) = match (&d, &o) {
            (Some(d), _) => d.clone(),
            (None, Some(o)) => (o.0, o.1, vec![255u8; o.0 * o.1 * 4]),
            _ => return None,
        };
        if let Some((ow, oh, op)) = &o {
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = (x * ow / w, y * oh / h);
                    rgba[(y * w + x) * 4 + 3] = op[(sy * ow + sx) * 4]; // BC4 mask: red channel
                }
            }
        }
        let stem = |p: &str| {
            let n = p.rsplit(['/', '\\']).next().unwrap_or(p).to_ascii_lowercase();
            n.trim_end_matches(".swatchbin").split('_').filter(|q| q.len() != 36).collect::<Vec<_>>().join("_")
        };
        let used: Vec<String> = [d.as_ref().and(diff.as_deref()), o.as_ref().and(opac.as_deref())].into_iter().flatten().map(stem).collect();
        let png = format!("{}.png", used.join("+"));
        crate::model::write_png(&dir.join(&png), w as u32, h as u32, &rgba).ok()?;
        Some((g.add_texture(&png), d.is_some(), o.is_some()))
    });
    if let Some((t, has_colour, has_mask)) = tex {
        look.texture = Some(t);
        if has_colour {
            look.colour = [1.0, 1.0, 1.0, if look.blend { look.colour[3] } else { 1.0 }];
        }
        look.mask = has_mask;
    }
    look
}

struct Corner {
    name: &'static str,
    /// Wheel centre (FM6 frame), from `Locators.xml`.
    at: [f32; 3],
    right: bool,
    front: bool,
}

fn locators(xml: &str) -> Vec<Corner> {
    let mut out = Vec::new();
    for (name, right, front) in [("LF", false, true), ("RF", true, true), ("LR", false, false), ("RR", true, false)] {
        let Some(part) = xml.split("<Locator ").find(|p| p.contains(&format!("\"carLocator_wheel{name}\""))) else { continue };
        let val = |k: &str| part.split(&format!("value._{k}=\"")).nth(1).and_then(|s| s.split('"').next()).and_then(|v| v.parse::<f32>().ok());
        if let (Some(x), Some(y), Some(z)) = (val("41"), val("42"), val("43")) {
            out.push(Corner { name, at: [x, y, z], right, front });
        }
    }
    out
}

/// A right-side part is the left-side model turned half a circle about the vertical (keeps the triangle winding).
fn half_turn(p: [f32; 3], on: bool) -> [f32; 3] {
    if on {
        [-p[0], p[1], -p[2]]
    } else {
        p
    }
}

struct Axle {
    rim_radius: f32,
    tyre_radius: f32,
    width: f32,
    tyre: tyre::TyreSize,
}

/// Morph weights for one axle, read from the wheel model itself (MODLOG 107): radius target (last) and width target
/// (first); rim width = tyre width (INFERRED: the database has no rim width).
fn wheel_weights(meshes: &[&modelbin::Mesh], axle: &Axle) -> Option<[f32; 2]> {
    let mut far = (0.0f32, 0.0f32);
    let mut inner = (f32::MIN, 0.0f32);
    for m in meshes.iter().filter(|m| m.morphs.len() == 2) {
        for (i, p) in m.positions.iter().enumerate() {
            let r = (p[1] * p[1] + p[2] * p[2]).sqrt();
            if let Some(d) = m.morphs[1].get(i) {
                if r > far.0 {
                    far = (r, (d[1] * p[1] + d[2] * p[2]) / r.max(1e-6));
                }
            }
            if let Some(d) = m.morphs[0].get(i) {
                if p[0] > inner.0 && d[0].abs() > 1e-3 {
                    inner = (p[0], d[0]);
                }
            }
        }
    }
    (far.1.abs() > 1e-6 && inner.1.abs() > 1e-6).then(|| [(axle.width - inner.0) / inner.1, (axle.rim_radius - far.0) / far.1])
}

/// Writes `model.gltf`, `model.bin`, textures and `model.json` for one car; returns a one-line report.
/// Returns a one-line report and two boxes in the game frame (front +z), `[min, max]`: the whole placed model and its body only.
fn build_model(game: &str, zip_path: &Path, lib: &Library, car: &serde_json::Map<String, Value>, db_paint: Option<u32>, media: &str, dir: &Path) -> Result<(String, [[f32; 3]; 2], [[f32; 3]; 2])> {
    let cz = CarZip::open(zip_path)?;
    let skel_path = cz.paths("scene/", "_skeleton.modelbin").into_iter().next().context("no skeleton modelbin")?;
    let bones = modelbin::parse_skeleton(&cz.read(&skel_path).context("skeleton")?)?;
    let mut corners = locators(&String::from_utf8_lossy(&cz.read("locators.xml").unwrap_or_default()));
    anyhow::ensure!(corners.len() == 4, "Locators.xml: {} of 4 wheel locators", corners.len());
    let num = |k: &str| car.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let axle = |end: &str| {
        let (w, a, d) = (num(&format!("{end}TireWidthMM")), num(&format!("{end}TireAspect")), num(&format!("{end}WheelDiameterIN")));
        let t = tyre::TyreSize::from_code(w, a, d);
        Axle { rim_radius: t.rim_radius, tyre_radius: t.outer_radius(), width: t.width, tyre: t }
    };
    let axles = [axle("Front"), axle("Rear")];
    // Hub height: FH1's own rule (cars.rs: tyre radius - stock ride height above the body's underside), not the locator's
    // y; on FM5's E21 the rear locator is 2.2 cm lower than that rule and tipped the raked car onto its rear wheels
    // (MODLOG 113). The locators keep x and z.
    for c in corners.iter_mut() {
        let ride = num(if c.front { "FrontStockRideHeight" } else { "RearStockRideHeight" });
        if ride > 0.0 {
            c.at[1] = axles[if c.front { 0 } else { 1 }].tyre_radius - ride;
        }
    }

    // Paint: the first colour file the car ships (`materials/carcolors/*.materialbin`), parameter `PAINT_COLOUR`.
    let paint = cz
        .paths("materials/carcolors/", ".materialbin")
        .iter()
        .find_map(|p| match materialbin::parse(&cz.read(p)?).ok()?.get(PAINT_COLOUR) {
            Some(MatValue::Vector(c)) | Some(MatValue::Color(c)) => Some([c[0], c[1], c[2], 1.0]),
            _ => None,
        })
        // no colour file (FM5): the database's first Combo_Colors row, sRGB 0xRRGGBB (INFERRED: its meaning in FM5 / FM6 not checked)
        .or_else(|| db_paint.map(|v| [srgb_to_linear((v >> 16) & 255), srgb_to_linear((v >> 8) & 255), srgb_to_linear(v & 255), 1.0]))
        .unwrap_or([0.5, 0.5, 0.5, 1.0]);

    let mut g = Gltf::default();
    // the placed model's boxes, engine frame (front -z): everything, and the body only (no wheels, tyres, brakes,
    // suspension arms: the shared-library parts)
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let (mut blo, mut bhi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut grow = |q: [f32; 3], body: bool| {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
            if body {
                blo[k] = blo[k].min(q[k]);
                bhi[k] = bhi[k].max(q[k]);
            }
        }
    };
    let mut mat_cache: HashMap<String, Option<Material>> = HashMap::new();
    let mut textures: HashMap<String, Option<(usize, bool, bool)>> = HashMap::new();
    let mut body_nodes = Vec::new();
    let mut wheel_children: [Vec<usize>; 4] = Default::default();
    let (mut parts, mut meshes_n, mut skipped) = (0, 0, Vec::new());
    let mut wheel_models: Vec<(String, Vec<modelbin::Mesh>, Vec<Material>)> = Vec::new();

    for path in cz.paths("scene/", ".modelbin") {
        if path.ends_with("_skeleton.modelbin") || path.ends_with("proxylod.modelbin") {
            continue;
        }
        let Some(bytes) = cz.read(&path) else { continue };
        let meshes = match modelbin::parse(&bytes) {
            Ok(m) => m,
            Err(e) => {
                skipped.push(format!("{path} ({e})"));
                continue;
            }
        };
        let insts = modelbin::material_instances(&bytes).unwrap_or_default();
        let mats: Vec<Material> = insts.iter().map(|i| resolve(&cz, lib, &mut mat_cache, i)).collect();
        let stem = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".modelbin").to_owned();
        if path.contains("/wheels/") {
            wheel_models.push((stem, meshes, mats));
            continue;
        }
        parts += 1;
        let library_part = path.contains("_library/");
        // shared-library parts: their bone's x, the matching wheel locator's y and z (INFERRED, MODLOG 106)
        let side = ["lf", "rf", "lr", "rr"].iter().position(|s| stem.contains(s));
        let mut look_cache: HashMap<u16, usize> = HashMap::new();
        for m in meshes.iter().filter(|m| m.lod == 0 && m.material != 0xFFFF) {
            let (place, corner): (Box<dyn Fn([f32; 3]) -> [f32; 3]>, Option<usize>) = if library_part || m.bone == 0 {
                let Some(si) = side else {
                    continue;
                };
                let c = &corners[si];
                let bone_name = if stem.starts_with("controlarm") { format!("controlArm_{}", c.name) } else { format!("hub{}", c.name) };
                let Some(b) = bones.iter().find(|b| b.name.eq_ignore_ascii_case(&bone_name)) else { continue };
                let at = [b.world[3][0], c.at[1], c.at[2]];
                let turn = c.right && !stem.starts_with("controlarm");
                if stem.contains("rotor") {
                    // rotors spin: a child of the wheel node, relative to the wheel centre
                    let off = [at[0] - c.at[0], 0.0, 0.0];
                    (Box::new(move |p| {
                        let q = half_turn(p, turn);
                        [q[0] + off[0], q[1] + off[1], q[2] + off[2]]
                    }), Some(si))
                } else {
                    (Box::new(move |p| {
                        let q = half_turn(p, turn);
                        [q[0] + at[0], q[1] + at[1], q[2] + at[2]]
                    }), None)
                }
            } else {
                let b = bones.get(m.bone as usize).context("mesh bone")?.clone();
                (Box::new(move |p| b.to_car(p)), None)
            };
            let turn_n = |n: [f32; 3]| -> [f32; 3] {
                if library_part && side.is_some_and(|s| corners[s].right) && !stem.starts_with("controlarm") {
                    [-n[0], n[1], -n[2]]
                } else if !library_part && m.bone != 0 {
                    bones[m.bone as usize].dir_to_car(n)
                } else {
                    n
                }
            };
            let pos: Vec<[f32; 3]> = m.positions.iter().map(|p| to_engine(place(*p))).collect();
            if corner.is_none() {
                pos.iter().for_each(|q| grow(*q, !library_part));
            }
            let nrm: Vec<[f32; 3]> = m.normals.iter().map(|n| to_engine(turn_n(*n))).collect();
            let rect = m.uv_rects[2];
            let uv: Option<Vec<[f32; 2]>> = m.uvs.iter().find(|(i, _)| *i == 0).map(|(_, set)| set.iter().map(|t| [rect[0] + t[0] * rect[1], rect[2] + t[1] * rect[3]]).collect());
            let idx: Vec<u32> = m.indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
            let mi = match look_cache.get(&m.material) {
                Some(&i) => i,
                None => {
                    let mat = mats.get(m.material as usize).cloned().unwrap_or_default();
                    let l = look_for(&mut g, &mat, &format!("{stem}_{}", m.material), paint, &cz, lib, dir, &mut textures);
                    let i = add_material(&mut g, &l);
                    look_cache.insert(m.material, i);
                    i
                }
            };
            let mesh = add_mesh(&mut g, &format!("{stem}_{}", m.material), &pos, &nrm, uv.as_deref(), &idx, mi);
            let node = g.node(json!({"name": format!("{stem}_{}", m.material), "mesh": mesh}));
            meshes_n += 1;
            match corner {
                Some(si) => wheel_children[si].push(node),
                None => body_nodes.push(node),
            }
        }
    }

    // Wheels: the rim (morphed per axle), our tyre, both relative to the wheel centre; right side turned.
    // FM6's IDx has one wheel model for all four; FM5's E21 has `WheelLF` (front) and `WheelLR` (rear)
    anyhow::ensure!(!wheel_models.is_empty(), "no wheel model in scene/_library/scene/wheels");
    let wheel_for = |front: bool| {
        let ends: &[&str] = if front { &["lf", "rf"] } else { &["lr", "rr"] };
        wheel_models.iter().find(|w| ends.iter().any(|e| w.0.to_ascii_lowercase().ends_with(e))).unwrap_or(&wheel_models[0])
    };
    let wheel_name = wheel_models.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(", ");
    let rubber = |g: &mut Gltf, n: &str| add_material(g, &Look { name: n.into(), colour: [0.03, 0.03, 0.03, 1.0], metallic: 0.0, roughness: 0.9, texture: None, blend: false, mask: false });
    let tread_mat = rubber(&mut g, "tyre_tread");
    let side_mat = rubber(&mut g, "tyre_sidewall");
    let mut rim_looks: HashMap<(String, u16), usize> = HashMap::new();
    for (ci, c) in corners.iter().enumerate() {
        let ax = &axles[if c.front { 0 } else { 1 }];
        let (wname, wmeshes, wmats) = wheel_for(c.front);
        let w0: Vec<&modelbin::Mesh> = wmeshes.iter().filter(|m| m.lod == 0 && m.material != 0xFFFF).collect();
        let w = wheel_weights(&w0, ax).with_context(|| format!("{wname}: wheel morph targets"))?;
        for m in &w0 {
            // the motion-blur disc (`wheelblur` shader) is the game's fast-spin layer: not drawn at rest (MODLOG 111)
            if wmats.get(m.material as usize).and_then(|t| t.shader.as_deref()).is_some_and(|s| s.to_ascii_lowercase().starts_with("wheelblur")) {
                continue;
            }
            let pos: Vec<[f32; 3]> = m
                .positions
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut q = *p;
                    for (t, wt) in w.iter().enumerate() {
                        if let Some(d) = m.morphs.get(t).and_then(|d| d.get(i)) {
                            for k in 0..3 {
                                q[k] += wt * d[k];
                            }
                        }
                    }
                    to_engine(half_turn(q, c.right))
                })
                .collect();
            let nrm: Vec<[f32; 3]> = m.normals.iter().map(|n| to_engine(half_turn(*n, c.right))).collect();
            let rect = m.uv_rects[2];
            let uv: Option<Vec<[f32; 2]>> = m.uvs.iter().find(|(i, _)| *i == 0).map(|(_, set)| set.iter().map(|t| [rect[0] + t[0] * rect[1], rect[2] + t[1] * rect[3]]).collect());
            let idx: Vec<u32> = m.indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
            let mi = match rim_looks.get(&(wname.clone(), m.material)) {
                Some(&i) => i,
                None => {
                    let mat = wmats.get(m.material as usize).cloned().unwrap_or_default();
                    let l = look_for(&mut g, &mat, &format!("{wname}_{}", m.material), paint, &cz, lib, dir, &mut textures);
                    let i = add_material(&mut g, &l);
                    rim_looks.insert((wname.clone(), m.material), i);
                    i
                }
            };
            let mesh = add_mesh(&mut g, &format!("rim_{}_{}", c.name, m.material), &pos, &nrm, uv.as_deref(), &idx, mi);
            wheel_children[ci].push(g.node(json!({"name": format!("rim_{}_{}", c.name, m.material), "mesh": mesh})));
        }
        let (tread, sides) = tyre::build(&ax.tyre, 72);
        for (label, part, mat) in [("tread", &tread, tread_mat), ("sidewall", &sides, side_mat)] {
            let pos: Vec<[f32; 3]> = part.positions.iter().map(|p| to_engine(half_turn(*p, c.right))).collect();
            let at = to_engine(c.at);
            pos.iter().for_each(|q| grow([q[0] + at[0], q[1] + at[1], q[2] + at[2]], false));
            let nrm: Vec<[f32; 3]> = part.normals.iter().map(|n| to_engine(half_turn(*n, c.right))).collect();
            let idx: Vec<u32> = part.indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
            let mesh = add_mesh(&mut g, &format!("tyre_{label}_{}", c.name), &pos, &nrm, Some(&part.uvs), &idx, mat);
            wheel_children[ci].push(g.node(json!({"name": format!("tyre_{label}_{}", c.name), "mesh": mesh})));
        }
    }
    let mut kids = body_nodes;
    for (ci, c) in corners.iter().enumerate() {
        let at = to_engine(c.at);
        kids.push(g.node(json!({"name": format!("wheel_{}", c.name), "translation": at, "children": wheel_children[ci]})));
    }
    let triangles = g.triangles;
    let texture_count = g.images.len();
    let root = g.node(json!({"name": media, "children": kids, "translation": [0.0, 0.0, 0.0]}));
    g.write(dir, root)?;

    let rgb = (linear_to_srgb8(paint[0]) << 16) | (linear_to_srgb8(paint[1]) << 8) | linear_to_srgb8(paint[2]);
    let hubs: Vec<Value> = corners.iter().map(|c| json!({"corner": c.name, "position": to_engine(c.at)})).collect();
    let model = json!({
        "media_name": media,
        "forward": "-Z",
        "wheel_radius": axles[0].tyre_radius,
        "wheel_width": axles[0].width,
        "lod0_stub_sections": [],
        "kit": {},
        "mesh_offset": [0.0, 0.0, 0.0],
        "paint": {"rgb": rgb, "metallic": false, "sequence": 1},
        "rim": wheel_name,
        "axles": axles.iter().map(|a| json!({"tyre_radius": a.tyre_radius, "tyre_width": a.width, "rim_radius": a.rim_radius})).collect::<Vec<_>>(),
        "hubs": hubs,
        "winding_flipped": false,
        "triangles": triangles,
        "imported_from": game,
        "wheels_from": wheel_name,
        "tyre": "built from the database sizes (no tyre model in the game files)",
    });
    std::fs::write(dir.join("model.json"), serde_json::to_vec_pretty(&model)?)?;
    let mut report = format!("{parts} parts, {meshes_n} body meshes, 4 wheels, {triangles} triangles, {texture_count} textures, paint #{rgb:06x}");
    let mut failed: Vec<String> = textures.iter().filter(|(_, t)| t.is_none()).map(|(k, _)| k.rsplit(['/', '\\']).next().unwrap_or(k).to_owned()).collect();
    failed.sort();
    if !failed.is_empty() {
        report += &format!("; textures not loaded or not decoded: {}", failed.join(", "));
    }
    if !skipped.is_empty() {
        report += &format!("; skipped {}", skipped.join(", "));
    }
    // back to the game frame (front +z): negate z, which swaps the z bounds
    let game_frame = |lo: [f32; 3], hi: [f32; 3]| [[lo[0], lo[1], -hi[2]], [hi[0], hi[1], -lo[2]]];
    Ok((report, game_frame(lo, hi), game_frame(blo, bhi)))
}
