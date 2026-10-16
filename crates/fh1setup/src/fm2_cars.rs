//! Forza Motorsport 2 cars (`import-fm2 --only cars`): `imported/fm2/cars/<MediaName>/` like the other imported games
//! (`model.gltf` + `model.bin`, `model.json`, `physics.json`, `index.json` rows `{id, media_name, name, maker, year, class, pi, drive, has_model}`).
//!
//! WHAT IS REAL: the body parts. `fh1_formats::carbin_fm2` decodes every part of `<car>.carbin` (positions, part centres, uv, triangle
//! lists); the stock part of each kit family is kept (`bumperFa`, `hooda`, `winga`, `skirtLa` ... letter `a`, no `race` variants) and
//! written in world space with normals computed from the triangles. Dimensions, weight, gearbox, torque curve and tyre sizes come from
//! the FM2 gamedb (`Data_Car`, `List_UpgradeWeight` / `Transmission` / `EngineCamValves`, `List_TorqueCurve`) and the car's
//! `Physics/MAXData.xml` (collision spheres, track widths).
//!
//! WHAT IS APPROXIMATED: (1) the paint and part materials are flat colours by material name (the textures are `.xds` atlases and the
//! uv offset into them is not settled, see FM2_HANDOFF.md); (2) the wheels (rim + tyre meshes and textures) are copied from an FH1
//! donor car scaled to the FM2 tyre size; (3) everything the FM2 gamedb has no table for (tyre compound curves, aero, suspension, diff,
//! camera) comes from the same FH1 donor's `physics.json`, the closest car by mass, drive type and year. No `fx/` folder is written, so
//! the engine keeps the glTF body (fh1-render car.rs "keeping the glTF body") instead of the FH1 car shaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};

use fh1_formats::carbin_fm2::{self, Part};
use fh1_formats::zip::Archive;

pub(crate) type Row = Map<String, Value>;

pub(crate) fn cars(disc: &Path, private: &Path, out: &Path) -> Result<()> {
    let db = Connection::open_with_flags(disc.join("media/db/gamedb.slt"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut strings = crate::fm4::strings(disc);
    // FM2's gamedb names carry the stem hashes of `Data_Car.str` (car models) and `List_CarMake.str`.
    if let Some(s) = strings.as_mut() {
        s.alias_hash(43337, "Data_Car");
        s.alias_hash(13795, "List_CarMake");
    }
    let text = |r: &str| strings.as_ref().and_then(|s| s.resolve(r)).map(fh1_ui::strtable::strip_markup).unwrap_or_else(|| r.to_owned());
    let makes: HashMap<i64, String> = crate::cars::rows(&db, "SELECT ID, DisplayName FROM List_CarMake", [])?
        .into_iter()
        .filter_map(|r| Some((r["ID"].as_i64()?, text(r["DisplayName"].as_str()?))))
        .collect();
    // Class letter from the badge file (`X_XBX_CLS_B_Badge.tga`) and the game's PI display scale.
    let classes = crate::cars::rows(&db, "SELECT Id, BadgeSmallPath, MaxPerformanceIndex, MaxDisplayPerformanceIndex FROM CarClasses ORDER BY Id", [])?;
    let class_letter = |id: i64| -> Option<String> {
        let c = classes.iter().find(|c| c["Id"].as_i64() == Some(id))?;
        let p = c["BadgeSmallPath"].as_str()?;
        let s = &p[p.find("CLS_")? + 4..];
        Some(s[..s.find('_')?].to_owned())
    };
    let display_pi = |pi: f64| -> i64 {
        let (mut lo, mut lo_d) = (0.0, 100.0);
        for c in &classes {
            let (hi, hi_d) = (c["MaxPerformanceIndex"].as_f64().unwrap_or(1.0), c["MaxDisplayPerformanceIndex"].as_f64().unwrap_or(999.0));
            if pi <= hi {
                return (lo_d + (pi - lo) / (hi - lo).max(1e-9) * (hi_d - lo_d)).round() as i64;
            }
            (lo, lo_d) = (hi, hi_d);
        }
        lo_d as i64
    };
    let sound = crate::fh2::Donors::load(private);
    let donors = WheelDonor::load_all(private);
    anyhow::ensure!(!donors.is_empty(), "no FH1 car with a model + physics.json to borrow wheels from (run fh1setup on the FH1 disc first)");

    let cars = crate::cars::rows(&db, "SELECT * FROM Data_Car ORDER BY Id", [])?;
    let (mut index, mut skipped) = (Vec::new(), Vec::new());
    for car in &cars {
        let media = car["MediaName"].as_str().unwrap_or_default().to_owned();
        match build_car(&db, disc, &donors, car, &media, &out.join(&media)) {
            Ok(mut b) => {
                // FH1-style columns the engine's browser / garage read.
                let maker = car["MakeID"].as_i64().and_then(|m| makes.get(&m).cloned()).unwrap_or_default();
                let model = car["DisplayName"].as_str().map(&text).unwrap_or_else(|| media.clone());
                let name = if maker.is_empty() || model.starts_with(&maker) { model.clone() } else { format!("{maker} {model}") };
                if let Some(d) = sound.pick(&media, &b.physics["car"]) {
                    b.physics["sound_donor"] = json!(d);
                }
                std::fs::write(out.join(&media).join("physics.json"), serde_json::to_vec_pretty(&b.physics)?)?;
                let drive = match b.physics["car"]["DriveTypeID"].as_i64() {
                    Some(1) => "FWD",
                    Some(2) => "RWD",
                    Some(3) => "AWD",
                    _ => "",
                };
                index.push(json!({
                    "id": car["Id"],
                    "media_name": media,
                    "name": name,
                    "model": model,
                    "maker": maker,
                    "year": car["Year"],
                    "class": car["ClassID"].as_i64().and_then(&class_letter),
                    "pi": car["PerformanceIndex"].as_f64().map(display_pi),
                    "drive": drive,
                    "selectable": true,
                    "has_model": true,
                }));
            }
            Err(e) => skipped.push(format!("{media}: {e:#}")),
        }
    }
    std::fs::write(out.join("index.json"), serde_json::to_vec_pretty(&index)?)?;
    println!("[fm2] {} cars imported, {} skipped", index.len(), skipped.len());
    for s in skipped.iter().take(12) {
        println!("[fm2]   skipped {s}");
    }
    Ok(())
}

struct Built {
    physics: Value,
}

/// An FH1 car whose wheels and `physics.json` another game's car borrows.
pub(crate) struct WheelDonor {
    pub(crate) dir: PathBuf,
    pub(crate) media: String,
    pub(crate) mass: f64,
    pub(crate) drive: i64,
    pub(crate) year: f64,
}

impl WheelDonor {
    pub(crate) fn load_all(private: &Path) -> Vec<WheelDonor> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(private.join("cars")).into_iter().flatten().flatten() {
            let dir = e.path();
            let (Ok(phys), true) = (std::fs::read(dir.join("physics.json")), dir.join("model.gltf").exists() && dir.join("model.json").exists()) else { continue };
            let Ok(p) = serde_json::from_slice::<Value>(&phys) else { continue };
            let Some(mass) = p["stock_parts"]["List_UpgradeCarBodyWeight"]["Mass"].as_f64() else { continue };
            out.push(WheelDonor {
                media: dir.file_name().unwrap().to_string_lossy().into_owned(),
                mass,
                drive: p["car"]["DriveTypeID"].as_i64().unwrap_or(0),
                year: p["car"]["Year"].as_f64().unwrap_or(2000.0),
                dir,
            });
        }
        out.sort_by(|a, b| a.media.cmp(&b.media));
        out
    }
}

pub(crate) fn pick<'a>(donors: &'a [WheelDonor], mass: f64, drive: i64, year: f64) -> &'a WheelDonor {
    donors
        .iter()
        .min_by(|a, b| {
            let cost = |d: &WheelDonor| (d.mass - mass).abs() / 300.0 + if d.drive == drive { 0.0 } else { 2.0 } + (d.year - year).abs() / 50.0;
            cost(a).total_cmp(&cost(b))
        })
        .unwrap()
}

pub(crate) fn f(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.0)
}

pub(crate) fn rf_(r: &Row, k: &str) -> f64 {
    r.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// One stock-row query: the car's `IsStock = 1` row of a `List_Upgrade*` table keyed by `Ordinal`.
fn stock_row(db: &Connection, table: &str, key: &str, id: i64) -> Result<Option<Row>> {
    let rows = crate::cars::rows(db, &format!("SELECT * FROM {table} WHERE {key} = ?1 ORDER BY IsStock DESC, Level"), [id])?;
    Ok(rows.into_iter().next())
}

fn build_car(db: &Connection, disc: &Path, donors: &[WheelDonor], car: &Row, media: &str, dir: &Path) -> Result<Built> {
    let id = car["Id"].as_i64().context("car id")?;
    let zip = disc.join("media/cars").join(format!("{media}.zip"));
    let mut ar = Archive::open(&zip).with_context(|| zip.display().to_string())?;
    let mut read = |name: &str| -> Result<Option<Vec<u8>>> {
        match ar.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)).cloned() {
            Some(e) => Ok(Some(ar.read(&e)?)),
            None => Ok(None),
        }
    };
    let carbin = carbin_fm2::parse(&read(&format!("{media}.carbin"))?.context("no carbin")?).map_err(|e| anyhow::anyhow!("carbin: {e}"))?;
    let maxdata = match read("Physics/MAXData.xml")? {
        Some(b) => crate::xml::to_json(&String::from_utf8_lossy(&b))?.get("MAXData").cloned().unwrap_or(Value::Null),
        None => Value::Null,
    };

    // ---- gamedb rows
    let weight = stock_row(db, "List_UpgradeWeight", "Ordinal", id)?.context("no List_UpgradeWeight row")?;
    let trans = stock_row(db, "List_UpgradeTransmission", "Ordinal", id)?.context("no List_UpgradeTransmission row")?;
    let engine_up = stock_row(db, "List_UpgradeEngine", "Ordinal", id)?.context("no List_UpgradeEngine row")?;
    let engine_id = engine_up["EngineID"].as_i64().context("EngineID")?;
    let engine = crate::cars::rows(db, "SELECT * FROM Data_Engine WHERE EngineID = ?1", [engine_id])?.into_iter().next().context("no Data_Engine row")?;
    let cam = stock_row(db, "List_UpgradeEngineCamValves", "EngineID", engine_id)?.context("no cam row")?;
    let curve_row = |cid: i64| -> Result<Option<Row>> { Ok(crate::cars::rows(db, "SELECT * FROM List_TorqueCurve WHERE TorqueCurveID = ?1", [cid])?.into_iter().next()) };
    let full = curve_row(cam["TorqueCurveFullThrottleID"].as_i64().unwrap_or(-1))?.context("no torque curve")?;
    let n = (full["NumTorqueValues"].as_i64().unwrap_or(0) as usize).clamp(2, 101);
    let samples: Vec<f64> = (0..n).map(|i| full[&format!("v{i}")].as_f64().unwrap_or(0.0)).collect();
    let max_rpm = rf_(&cam, "TorqueCurveMaxRPM");
    let tq_scale = rf_(&full, "TorqueScale");

    // ---- body geometry
    // The stock kit letter of each family: `a` + the stock row's Sequence (List_UpgradeFrontBumper / RearBumper / Hood / SideSkirt / RearWing).
    let mut kit: Vec<(&'static str, char)> = Vec::new();
    for (table, stems) in [("List_UpgradeFrontBumper", &["bumperf"][..]), ("List_UpgradeRearBumper", &["bumperr"][..]), ("List_UpgradeHood", &["hood"][..]), ("List_UpgradeSideSkirt", &["skirtl", "skirtr"][..]), ("List_UpgradeRearWing", &["wing"][..])] {
        if let Some(seq) = stock_row(db, table, "Ordinal", id)?.and_then(|r| r.get("Sequence")?.as_i64()).filter(|q| (0..26).contains(q)) {
            for st in stems {
                kit.push((st, (b'a' + seq as u8) as char));
            }
        }
    }
    let stock = stock_parts(&carbin.parts, &kit);
    anyhow::ensure!(!stock.is_empty(), "no usable parts");
    let tyre = |w: &str, a: &str, r: &str| (car[r].as_f64().unwrap_or(17.0) * 0.0254 * 0.5 + car[w].as_f64().unwrap_or(225.0) * 0.001 * car[a].as_f64().unwrap_or(40.0) * 0.01, car[w].as_f64().unwrap_or(225.0) * 0.001);
    let (rf, wf) = tyre("StockTire-FrontWidth-mm", "StockTire-FrontAspect-pct", "StockTire-FrontWheel-in");
    let (rr, wr) = tyre("StockTire-RearWidth-mm", "StockTire-RearAspect-pct", "StockTire-RearWheel-in");
    let wheelbase = rf_(car, "ModelWheelbase");
    let (ride_f, ride_r) = (rf_(car, "ModelFrontStockRideHeight"), rf_(car, "ModelRearStockRideHeight"));
    let centre = [rf_(car, "BottomCenterWheelbasePosx"), rf_(car, "BottomCenterWheelbasePosy"), rf_(car, "BottomCenterWheelbasePosz")];
    let (track_f, track_r) = (f(&maxdata["Misc"], "FrontTrackOuter").max(1.2), f(&maxdata["Misc"], "RearTrackOuter").max(1.2));
    let hubs = [
        ("LF", [-(track_f / 2.0 - wf / 2.0), rf - ride_f, -wheelbase / 2.0]),
        ("RF", [track_f / 2.0 - wf / 2.0, rf - ride_f, -wheelbase / 2.0]),
        ("LR", [-(track_r / 2.0 - wr / 2.0), rr - ride_r, wheelbase / 2.0]),
        ("RR", [track_r / 2.0 - wr / 2.0, rr - ride_r, wheelbase / 2.0]),
    ];

    // ---- donor
    let drive = engine["DriveTypeID"].as_i64().unwrap_or(2);
    let donor = pick(donors, rf_(&weight, "Mass"), drive, rf_(car, "Year"));
    let dphys: Value = serde_json::from_slice(&std::fs::read(donor.dir.join("physics.json"))?)?;
    let dmodel: Value = serde_json::from_slice(&std::fs::read(donor.dir.join("model.json"))?)?;
    let dgltf: Value = serde_json::from_slice(&std::fs::read(donor.dir.join("model.gltf"))?)?;
    let dbin = std::fs::read(donor.dir.join("model.bin"))?;

    std::fs::create_dir_all(dir)?;
    let mut g = Gltf::default();
    let mut kids = Vec::new();
    let (mut bmin, mut bmax) = ([f32::MAX; 3], [f32::MIN; 3]);
    // Textures (VERIFIED uv = value / 1280 on lights.xds for the lamp parts and on nodamage.xds for the interior): the lamp atlas, the
    // exterior / interior detail atlas and the shared bumper frame, bound to their materials by name.
    let race_key = crate::cars::rows(db, "SELECT RGB FROM Combo_Colors WHERE Ordinal = ?1 ORDER BY Sequence LIMIT 1", [id])?.into_iter().next().and_then(|c| c["RGB"].as_i64()).map(|v| v as u32 & 0xFF_FFFF) == Some(0xFF0090);
    let mut paint_bound = false;
    for (file, png, mats) in [
        (
            "lights.xds",
            "lights.png",
            &["textured_reflector", "detail_glass_red", "detail_glass_clear", "detail_glass_amber", "reflector", "head_light", "tail_light", "tail_light_on", "reverse_light", "red_glass"][..],
        ),
        // Materials whose uv land on the exterior / interior detail atlas (fm2_mat_survey: 82-92% of their vertices on opaque texels, ~15% for random uv).
        (
            "nodamage.xds",
            "exterior.png",
            &["interior", "weatherstrip", "frame", "plastic2", "plastic_2", "plastic", "matte_colors", "misc", "black", "body2", "rubber_trim", "dullplastic"][..],
        ),
    ] {
        // Liveried cars ship `nodamageUncomp.xds`, the same atlas at 2048 x 2048 uncompressed (the game draws that one; the 1024 DXT version
        // blocks up the livery's thin swooshes when magnified).
        let hi = if file == "nodamage.xds" { read("nodamageuncomp.xds")? } else { None };
        if let Some(d) = hi.or(read(file)?) {
            if let Ok((_, img)) = fh1_formats::xds::decode_base(&d) {
                if let Ok(mut rgba) = fh1_formats::xds::to_rgba8(&img) {
                    // The detail atlas keeps stray colour (lime, magenta) under alpha 0, where the game draws the paint colour instead; the glTF
                    // ignores the alpha, so those texels become dark plastic.
                    if file == "nodamage.xds" {
                        for px in rgba.chunks_exact_mut(4) {
                            if px[3] < 128 {
                                px.copy_from_slice(&[22, 22, 22, 255]);
                            }
                        }
                    }
                    // Paint materials: the engine's car paint draws lerp(paint, atlas, atlas alpha). Normal cars have alpha 0 over their paint areas
                    // (the atlas only carries small decals there); race cars ship the whole livery in the atlas with alpha 1 and the team colour baked in
                    // (their `Combo_Colors` colour is the magenta 0xFF0090 key, which never shows), so binding the atlas as it is gives each car its livery.
                    if file == "nodamage.xds" {
                        if let Ok(mut orig) = fh1_formats::xds::to_rgba8(&img) {
                            // A race car's `Combo_Colors` colour is the magenta key 0xFF0090; the atlas keeps the team colour in the RGB under its
                            // transparent texels (C6.R: yellow), so the key must never show: make the atlas opaque.
                            if race_key {
                                for px in orig.chunks_exact_mut(4) {
                                    px[3] = 255;
                                }
                            }
                            crate::model::write_png(&dir.join("paint.png"), img.width, img.height, &orig)?;
                            let tp = g.add_texture("paint.png");
                            for m in ["body", "body_2", "badge", "race_kit_red"] {
                                g.mat_texture.insert(m.to_owned(), tp);
                            }
                            paint_bound = true;
                        }
                    }
                    // Emblems are logos on a transparent plate: they keep the atlas alpha (a cut-out) in a copy of the atlas.
                    if file == "nodamage.xds" {
                        let mut cut = fh1_formats::xds::to_rgba8(&img).unwrap_or_default();
                        for px in cut.chunks_exact_mut(4) {
                            if px[3] < 128 {
                                px[3] = 0;
                            }
                        }
                        crate::model::write_png(&dir.join("emblem.png"), img.width, img.height, &cut)?;
                        let te = g.add_texture("emblem.png");
                        g.mat_texture.insert("emblem".into(), te);
                        g.cutout.insert("emblem".into());
                    }
                    crate::model::write_png(&dir.join(png), img.width, img.height, &rgba)?;
                    let t = g.add_texture(png);
                    for m in mats {
                        g.mat_texture.insert((*m).to_owned(), t);
                    }
                }
            }
        }
    }
    for (file, png, mats) in [("bumper_frame.xds", "bumper_frame.png", &["bumper_frame"][..]), ("carbonFiber.xds", "carbon.png", &["carbon_fiber"][..]), ("grille1.xds", "grille1.png", &["grille1"][..])] {
        let Ok(d) = std::fs::read(disc.join("media/cars/shared").join(file)) else { continue };
        let Ok((_, img)) = fh1_formats::xds::decode_base(&d) else { continue };
        let Ok(rgba) = fh1_formats::xds::to_rgba8(&img) else { continue };
        crate::model::write_png(&dir.join(png), img.width, img.height, &rgba)?;
        let t = g.add_texture(png);
        for m in mats {
            g.mat_texture.insert((*m).to_owned(), t);
        }
    }
    // The engine's car-paint shading draws lerp(paint, atlas, atlas alpha): a paint material needs a texture whose alpha is 0 to show the paint
    // (FH1 bodies carry the exterior atlas for that), so the paint materials get a 1 x 1 transparent texture.
    if !paint_bound {
        crate::model::write_png(&dir.join("paint.png"), 1, 1, &[0, 0, 0, 0])?;
        let paint_tex = g.add_texture("paint.png");
        for m in ["body", "body_2", "badge", "race_kit_red"] {
            g.mat_texture.insert(m.to_owned(), paint_tex);
        }
    }
    let flip = signed_volume(&stock) < 0.0;
    // The car's first stock colour (`Combo_Colors`, RGB = 0xRRGGBB, sRGB); a palette entry when the car has none.
    let colour = crate::cars::rows(db, "SELECT RGB, Metallic FROM Combo_Colors WHERE Ordinal = ?1 ORDER BY Sequence LIMIT 1", [id])?.into_iter().next();
    let rgb8 = colour.as_ref().and_then(|c| c["RGB"].as_i64()).map(|v| v as u32 & 0xFF_FFFF);
    let metallic = colour.as_ref().and_then(|c| c["Metallic"].as_i64()) == Some(1);
    let paint: Colour = match rgb8 {
        Some(v) => [(v >> 16) & 255, (v >> 8) & 255, v & 255].map(|c| (c as f32 / 255.0).powf(2.2)),
        None => PALETTE[(id as usize) % PALETTE.len()],
    };
    g.paint_metallic = metallic;
    for p in &stock {
        let Some(c) = p.centre else { continue };
        let node = g.part(p, c, flip, paint, &mut bmin, &mut bmax);
        kids.extend(node);
    }
    // Wheels: the car's own FM2 wheel (one mesh, tyre + rim, face texture `wheel0.xds`) scaled to the tyre size; the donor's meshes when it has none.
    let own_wheel = load_wheel(db, disc, car);
    let wheel_mesh = match &own_wheel {
        Some(w) => {
            crate::model::write_png(&dir.join("wheel.png"), w.png.0, w.png.1, &w.png.2)?;
            let t = g.add_texture("wheel.png");
            g.mat_texture.insert("wheel_tire".into(), t);
            if let Some((aw, ah, argba)) = &w.atlas_png {
                crate::model::write_png(&dir.join("wheel_atlas.png"), *aw, *ah, argba)?;
                let ta = g.add_texture("wheel_atlas.png");
                g.mat_texture.insert("wheel_atlas".into(), ta);
            }
            if let Some((tw, th, trgba)) = &w.tyre_png {
                crate::model::write_png(&dir.join("tyre.png"), *tw, *th, trgba)?;
                let tt = g.add_texture("tyre.png");
                g.mat_texture.insert("wheel_tyre".into(), tt);
            }
            let flip_w = signed_volume(&[&w.part]) < 0.0;
            let (mut a, mut b) = ([0.0; 3], [0.0; 3]);
            g.part_mesh(&w.part, [0.0; 3], flip_w, paint, &mut a, &mut b)
        }
        None => None,
    };
    let mut copier = Copier::new(&donor.dir, dir);
    for (i, (corner, hub)) in hubs.iter().enumerate() {
        let front = i < 2;
        if let (Some(w), Some(mesh)) = (&own_wheel, wheel_mesh) {
            let (r, wd) = if front { (rf, wf) } else { (rr, wr) };
            let (sr, sx) = ((r / w.radius as f64).max(0.2), (wd / w.width as f64).max(0.2));
            let mirrored = i % 2 == 1;
            let child = g.node(json!({"mesh": mesh}));
            kids.push(g.node(json!({
                "name": format!("wheel_{corner}"),
                "children": [child],
                "translation": [hub[0] + centre[0], hub[1] + centre[1], hub[2] + centre[2]],
                "scale": [if mirrored { -sx } else { sx }, sr, sr],
            })));
            continue;
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
        let mirrored = i % 2 == 1;
        let xf = |a: f64, b: usize| a + centre[b];
        let mut n = json!({"name": format!("wheel_{corner}"), "children": children, "translation": [xf(hub[0], 0), xf(hub[1], 1), xf(hub[2], 2)]});
        n["scale"] = json!([if mirrored { -s } else { s }, s, s]);
        kids.push(g.node(n));
    }
    let mesh_offset = [-centre[0], -centre[1], -centre[2]];
    let root = g.node(json!({"name": media, "children": kids, "translation": mesh_offset}));
    let triangles = g.triangles;
    g.write(dir, root)?;

    // ---- model.json
    // The driver's eye for the cockpit camera (the engine reads `cockpit_eye` when a car has no cockpit model): the steering wheel part's centre
    // plus the game's default offset from the wheel (0, 0.21, 0.51 in mesh space: up and back), like fh1-render car.rs `cockpit_eye`.
    let cockpit_eye = stock.iter().find(|p| p.name.eq_ignore_ascii_case("steering_wheel")).and_then(|p| p.centre).map(|c| [c[0], c[1] + 0.21, c[2] + 0.51]);
    let rim_in = |k: &str| car[k].as_f64().unwrap_or(17.0) * 0.0254 * 0.5;
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
            {"tyre_radius": rf, "tyre_width": wf, "rim_radius": rim_in("StockTire-FrontWheel-in")},
            {"tyre_radius": rr, "tyre_width": wr, "rim_radius": rim_in("StockTire-RearWheel-in")},
        ],
        "hubs": hubs.iter().map(|(c, h)| json!({"corner": c, "position": h})).collect::<Vec<_>>(),
        "winding_flipped": flip,
        "triangles": triangles,
        "imported_from": "fm2",
        "wheels_from": donor.media,
        "cockpit_eye": cockpit_eye,
    });
    std::fs::write(dir.join("model.json"), serde_json::to_vec_pretty(&model)?)?;

    // ---- physics.json: the donor's, with everything FM2 has a table for replaced.
    let mut p = dphys;
    p["id"] = json!(id);
    p["media_name"] = json!(media);
    p["imported_from"] = json!("fm2");
    p["donor"] = json!(donor.media);
    let mut c = p["car"].as_object().cloned().unwrap_or_default();
    for (k, v) in car {
        c.insert(k.clone(), v.clone());
    }
    for (to, from) in [
        ("FrontTireWidthMM", "StockTire-FrontWidth-mm"), ("FrontTireAspect", "StockTire-FrontAspect-pct"), ("FrontWheelDiameterIN", "StockTire-FrontWheel-in"),
        ("RearTireWidthMM", "StockTire-RearWidth-mm"), ("RearTireAspect", "StockTire-RearAspect-pct"), ("RearWheelDiameterIN", "StockTire-RearWheel-in"),
        ("WeightDistribution", "FrontWeight-pct"),
    ] {
        if let Some(v) = car.get(from) {
            c.insert(to.into(), v.clone());
        }
    }
    c.insert("CurbWeight".into(), weight["Mass"].as_f64().map(|m| json!(m / 100.0)).unwrap_or(Value::Null));
    c.insert("DriveTypeID".into(), json!(drive));
    c.insert("Displacement".into(), engine["Disp_cc"].clone());
    c.insert("CylinderID".into(), engine["CylinderID"].clone());
    c.insert("EngineConfigID".into(), engine["ConfigID"].clone());
    c.insert("AspirationTypeId".into(), engine["AspirationID_Stock"].clone());
    c.insert("NumGears".into(), trans["NumGears"].clone());
    // Peak power rpm and a geared top speed from the curve.
    let (mut best, mut best_rpm) = (0.0f64, 0.0f64);
    for (i, s) in samples.iter().enumerate() {
        let rpm = i as f64 * max_rpm / (n - 1) as f64;
        if s * rpm > best {
            best = s * rpm;
            best_rpm = rpm;
        }
    }
    c.insert("SimPeakAngVel".into(), json!(best_rpm * std::f64::consts::TAU / 60.0));
    c.insert("SimPeakPower".into(), json!(best * tq_scale * std::f64::consts::TAU / 60.0));
    let top_gear = (1..8).filter_map(|i| trans[&format!("GearRatio{i}")].as_f64()).filter(|r| *r > 0.0).fold(f64::MAX, f64::min);
    let redline = rf_(&cam, "RedlineRPM");
    if top_gear.is_finite() && rf_(&trans, "FinalDriveRatio") > 0.0 {
        let v = redline * std::f64::consts::TAU / 60.0 / (rf_(&trans, "FinalDriveRatio") * top_gear) * rr;
        c.insert("SimTopSpeed".into(), json!(v * 0.85));
    }
    p["car"] = Value::Object(c);

    p["body"]["ModelWheelbase"] = json!(wheelbase);
    p["body"]["ModelFrontTrackOuter"] = json!(track_f);
    p["body"]["ModelRearTrackOuter"] = json!(track_r);
    p["body"]["ModelFrontStockRideHeight"] = json!(ride_f);
    p["body"]["ModelRearStockRideHeight"] = json!(ride_r);
    p["body"]["BottomCenterWheelbasePosx"] = json!(centre[0]);
    p["body"]["BottomCenterWheelbasePosy"] = json!(centre[1]);
    p["body"]["BottomCenterWheelbasePosZ"] = json!(centre[2]);
    // Pristine bounding box: the parts' extent in the origin-centred frame (wheels included).
    let wheel_x = (track_f.max(track_r)) / 2.0 + 0.0;
    let ymin = (-ride_f.min(ride_r)).min(bmin[1] as f64 - centre[1]);
    let bb = [
        ("PristineBoundingBoxMinX", (bmin[0] as f64 - centre[0]).min(-wheel_x)), ("PristineBoundingBoxMaxX", (bmax[0] as f64 - centre[0]).max(wheel_x)),
        ("PristineBoundingBoxMinY", ymin), ("PristineBoundingBoxMaxY", bmax[1] as f64 - centre[1]),
        ("PristineBoundingBoxMinZ", bmin[2] as f64 - centre[2]), ("PristineBoundingBoxMaxZ", bmax[2] as f64 - centre[2]),
    ];
    for (k, v) in bb {
        p["body"][k] = json!(v);
    }

    let sp = &mut p["stock_parts"];
    // FM2 rows replace the donor's weight / gearbox / camshaft; boost parts of the donor are dropped (FM2 stock cars are what the row says).
    if let Some(o) = sp.as_object_mut() {
        o.retain(|k, _| !(k.contains("Turbo") || k.ends_with("CSC") || k.ends_with("DSC")));
    }
    for k in ["Mass", "InitialMass", "CMHeight", "CMBackFront", "CMLeftRight", "BlockDimX", "BlockDimY", "BlockDimZ"] {
        sp["List_UpgradeCarBodyWeight"][k] = weight[k].clone();
    }
    for k in ["MomentInertia", "GearShiftTime", "FinalDriveRatio", "NumGears"] {
        sp["List_UpgradeDrivetrainTransmission"][k] = trans[k].clone();
    }
    for i in 0..9 {
        let key = format!("GearRatio{i}");
        sp["List_UpgradeDrivetrainTransmission"][&key] = trans.get(&key).cloned().filter(|v| !v.is_null()).unwrap_or(json!(-1.0));
    }
    for k in ["RedlineRPM", "StallRPM", "StartRPM", "TorqueCurveMaxRPM", "NumRPMEntriesArray"] {
        sp["List_UpgradeEngineCamshaft"][k] = cam[k].clone();
    }
    sp["List_UpgradeEngineCamshaft"]["TorqueCurveFullThrottleID"] = cam["TorqueCurveFullThrottleID"].clone();
    sp["List_UpgradeEngineCamshaft"]["TorqueCurveZeroThrottleID"] = cam["TorqueCurveZeroThrottleID"].clone();
    sp["List_UpgradeCarBodyTireWidthFront"]["FrontTireWidth"] = car["StockTire-FrontWidth-mm"].clone();
    sp["List_UpgradeCarBodyTireWidthRear"]["RearTireWidth"] = car["StockTire-RearWidth-mm"].clone();
    sp["List_UpgradeRimSizeFront"]["FrontWheelDiameter"] = car["StockTire-FrontWheel-in"].clone();
    sp["List_UpgradeRimSizeRear"]["RearWheelDiameter"] = car["StockTire-RearWheel-in"].clone();
    // Every Combo_Colors row (the donor's would paint the car with another car's colours).
    p["colors"] = Value::Array(crate::cars::rows(db, "SELECT Sequence, RGB, Metallic FROM Combo_Colors WHERE Ordinal = ?1 ORDER BY Sequence", [id])?.into_iter().map(Value::Object).collect());
    p["torque_curve"] = json!({"id": full["TorqueCurveID"], "torque_scale_nm": tq_scale, "zero_throttle_nm": Value::Null, "max_rpm": max_rpm, "samples": samples});
    p["engine"]["EngineID"] = json!(engine_id);
    for k in ["EngineMass-kg", "MomentInertia", "GasTankSize"] {
        p["engine"][k] = engine[k].clone();
    }

    // Suspension, anti-roll bars, tyre compound + friction curves, aero and the remaining stock part rows: FM2's own (same schema as FH1's).
    let one = |sql: &str, id: Option<i64>| -> Result<Value> {
        Ok(match id {
            Some(id) => crate::cars::rows(db, sql, [id])?.into_iter().next().map(Value::Object).unwrap_or(Value::Null),
            None => Value::Null,
        })
    };
    let geti = |r: &Option<Row>, k: &str| r.as_ref().and_then(|r| r.get(k)?.as_i64());
    let spring_row = stock_row(db, "List_UpgradeSpringDamper", "Ordinal", id)?;
    let sway_row = stock_row(db, "List_UpgradeAntiSway", "Ordinal", id)?;
    let tyre_row = stock_row(db, "List_UpgradeTireCompound", "Ordinal", id)?;
    let sp_q = "SELECT * FROM List_SpringDamperPhysics WHERE SpringDamperPhysicsID = ?1";
    let sw_q = "SELECT * FROM List_AntiSwayPhysics WHERE AntiSwayPhysicsID = ?1";
    for (key, v) in [
        ("front", one(sp_q, geti(&spring_row, "FrontSpringDamperPhysicsID"))?),
        ("rear", one(sp_q, geti(&spring_row, "RearSpringDamperPhysicsID"))?),
        ("anti_sway_front", one(sw_q, geti(&sway_row, "FrontAntiSwayPhysicsID"))?),
        ("anti_sway_rear", one(sw_q, geti(&sway_row, "RearAntiSwayPhysicsID"))?),
    ] {
        if !v.is_null() {
            p["suspension"][key] = v;
        }
    }
    if let Some(c) = one("SELECT * FROM List_TireCompound WHERE TireCompoundID = ?1", geti(&tyre_row, "TireCompoundID"))?.as_object().cloned() {
        let mut affect = Map::new();
        for (k, v) in &c {
            if let (Some(name), Some(cid)) = (k.strip_prefix("AffectCurve").and_then(|n| n.strip_suffix("ID")), v.as_i64()) {
                affect.insert(name.to_owned(), crate::cars::affect_curve(db, cid)?);
            }
        }
        p["tires"] = json!({
            "compound": c,
            "friction_lateral": crate::cars::multi_curve(db, c.get("FrictionMultiCurveLateralID").and_then(Value::as_i64))?,
            "friction_longitudinal": crate::cars::multi_curve(db, c.get("FrictionMultiCurveLongitudinalID").and_then(Value::as_i64))?,
            "affect_curves": affect,
        });
    }
    let aero_q = "SELECT * FROM List_AeroPhysics WHERE AeroPhysicsID = ?1";
    for (key, table) in [("front_bumper", "List_UpgradeFrontBumper"), ("rear_wing", "List_UpgradeRearWing")] {
        let a = one(aero_q, geti(&stock_row(db, table, "Ordinal", id)?, "AeroPhysicsID"))?;
        if !a.is_null() {
            p["aero"][key] = a;
        }
    }
    for (table, key) in [
        ("List_UpgradeBrakes", "List_UpgradeBrakes"),
        ("List_UpgradeDifferential", "List_UpgradeDrivetrainDifferential"),
        ("List_UpgradeClutch", "List_UpgradeDrivetrainClutch"),
        ("List_UpgradeDriveline", "List_UpgradeDrivetrainDriveline"),
        ("List_UpgradeFlywheel", "List_UpgradeEngineFlywheel"),
    ] {
        if let Some(r) = stock_row(db, table, "Ordinal", id)? {
            p["stock_parts"][key] = Value::Object(r);
        }
    }

    // MAXData (collision spheres, bounding box): FM2's, with the FM3-style sphere flags added (1 = 0.05 m corner marker, 2 = body sphere).
    let mut md = maxdata;
    if let Some(sph) = md["Collision"]["CollSpheres"].as_object_mut() {
        for (k, v) in sph.iter_mut() {
            if k.starts_with("Sphere") && v.get("Flags").is_none() {
                let small = v["Radius"].as_f64().unwrap_or(1.0) <= 0.06;
                v["Flags"] = json!(if small { 1.0 } else { 2.0 });
            }
        }
    }
    if !md.is_null() {
        p["maxdata"] = md;
    }
    Ok(Built { physics: p })
}

// ---------------------------------------------------------------------------------------------------------------
// parts

/// The car's stock FM2 wheel (`List_Wheels.MediaName` of `StockWheelID`, `media/wheels/<name>.zip`): the single `wheel` section
/// (axle along x, tyre + rim, material renamed `wheel_tire`) and its `wheel.xds` atlas.
struct Wheel {
    part: Part,
    radius: f32,
    width: f32,
    png: (u32, u32, Vec<u8>),
    /// The shared tread / sidewall texture (`tirea0.xds`) when it decoded.
    tyre_png: Option<(u32, u32, Vec<u8>)>,
    atlas_png: Option<(u32, u32, Vec<u8>)>,
}

fn load_wheel(db: &Connection, disc: &Path, car: &Row) -> Option<Wheel> {
    let sid = car.get("StockWheelID")?.as_i64()?;
    let media = crate::cars::rows(db, "SELECT MediaName FROM List_Wheels WHERE ID = ?1", [sid]).ok()?.into_iter().next()?["MediaName"].as_str()?.to_owned();
    let mut ar = Archive::open(disc.join("media/wheels").join(format!("{media}.zip"))).ok()?;
    let mut read = |suffix: &str| -> Option<Vec<u8>> {
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(suffix) && !e.name.contains('/'))?.clone();
        ar.read(&e).ok()
    };
    let cb = carbin_fm2::parse(&read(".carbin")?).ok()?;
    let mut part = cb.parts.into_iter().find(|p| p.runs.iter().any(|r| r.lod == 1 && !r.indices.is_empty()))?;
    // `wheel0.xds` is the face-on rim picture the mesh's planar uv expects (VERIFIED in game); `wheel.xds` is a strip atlas the mesh does not use.
    let tex = fh1_formats::xds::decode_base(&read("wheel0.xds").or_else(|| read("wheel.xds"))?).ok()?;
    let rgba = fh1_formats::xds::to_rgba8(&tex.1).ok()?;
    // The first index run of each LOD is the tyre (r 0.22..0.32 m, u runs 0..25 around the circumference: needs a repeating sampler); the rest is the rim.
    // Texture family by the stock compound (INFERRED, the gamedb has no link): street / classic = A (grooved tread), L1-L2 dry sport = B (hatched
    // lettering), L3 dry and race = C (smooth slick); wear steps 0..4 exist, 0 is used.
    let compound = car.get("Id").and_then(Value::as_i64).and_then(|id| stock_row(db, "List_UpgradeTireCompound", "Ordinal", id).ok().flatten()).and_then(|r| r.get("TireCompoundID")?.as_i64());
    let family = match compound {
        Some(5 | 6) => "tireB0.xds",
        Some(7 | 8) => "tireC0.xds",
        _ => "tirea0.xds",
    };
    let tyre_png = std::fs::read(disc.join("media/cars/shared").join(family)).ok().and_then(|d| {
        let (_, img) = fh1_formats::xds::decode_base(&d).ok()?;
        Some((img.width, img.height, fh1_formats::xds::to_rgba8(&img).ok()?))
    });
    // Wheel materials (names verified on all 400+ wheel carbins): `tire` = tread + sidewall (the shared tyre texture, uv u repeats ~25x around),
    // `rim` / `inner_rim` = the 3D wheel, uv into the `wheel.xds` atlas (fitted: not planar), `blur_rim` = a flat disc with exactly planar uv for the
    // motion-blur pictures wheel0 (sharp) .. wheel3 (fast), which would cover the 3D rim: not drawn.
    let atlas_png = read("wheel.xds").and_then(|d| {
        let (_, img) = fh1_formats::xds::decode_base(&d).ok()?;
        Some((img.width, img.height, fh1_formats::xds::to_rgba8(&img).ok()?))
    });
    let names = std::mem::take(&mut part.materials);
    let mut out: Vec<String> = Vec::new();
    let mut map: Vec<Option<usize>> = Vec::new();
    for n in &names {
        let l = n.to_ascii_lowercase();
        let new = if l.contains("blur") {
            None
        } else if l.contains("tire") || l.contains("tyre") {
            Some(if tyre_png.is_some() { "wheel_tyre" } else { "wheel_tire" })
        } else if (l.contains("rim") || l.contains("lip")) && atlas_png.is_some() {
            Some("wheel_atlas")
        } else {
            Some("wheel_tire")
        };
        map.push(new.map(|nm| match out.iter().position(|o| o == nm) {
            Some(i) => i,
            None => {
                out.push(nm.to_owned());
                out.len() - 1
            }
        }));
    }
    part.runs.retain_mut(|r| match map.get(r.material).copied().flatten() {
        Some(i) => {
            r.material = i;
            true
        }
        None => false,
    });
    part.materials = out;
    let (mut radius, mut lo, mut hi) = (0.0f32, f32::MAX, f32::MIN);
    for v in &part.vertices {
        radius = radius.max((v.position[1] * v.position[1] + v.position[2] * v.position[2]).sqrt());
        lo = lo.min(v.position[0]);
        hi = hi.max(v.position[0]);
    }
    (radius > 0.1 && hi > lo).then_some(Wheel { part, radius, width: hi - lo, png: (tex.1.width, tex.1.height, rgba), tyre_png, atlas_png })
}

/// The stock parts: the first (letter `a`) of each kit family and none of the `race` / roll-cage extras.
fn stock_parts<'a>(parts: &'a [Part], kit: &[(&'static str, char)]) -> Vec<&'a Part> {
    const FAMILIES: [&str; 6] = ["bumperf", "bumperr", "hood", "wing", "skirtl", "skirtr"];
    parts
        .iter()
        .filter(|p| {
            let n = p.name.to_ascii_lowercase();
            // `Original_damage_body`, `exclude_*`: damage-model duplicates that sit on top of the intact parts.
            if n.ends_with("race") || n.starts_with("cage") || n.contains("damage") || n.starts_with("exclude") {
                return false;
            }
            match FAMILIES.iter().find(|f| n.starts_with(**f)) {
                Some(f) => {
                    let letter = kit.iter().find(|(k, _)| k == f).map_or('a', |(_, l)| *l);
                    n[f.len()..] == letter.to_string()
                }
                None => true,
            }
        })
        .collect()
}

pub(crate) fn signed_volume(parts: &[&Part]) -> f64 {
    let mut v = 0.0;
    for p in parts {
        for r in p.runs.iter().filter(|r| r.lod == 1) {
            for t in r.indices.chunks_exact(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| p.vertices[i as usize].position.map(f64::from));
                v += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
            }
        }
    }
    v
}

pub(crate) type Colour = [f32; 3];
pub(crate) const PALETTE: [Colour; 8] = [
    [0.55, 0.03, 0.04], [0.04, 0.12, 0.45], [0.65, 0.65, 0.68], [0.02, 0.02, 0.025],
    [0.85, 0.82, 0.78], [0.05, 0.30, 0.12], [0.80, 0.45, 0.05], [0.30, 0.30, 0.33],
];

pub(crate) fn paint_rgb(c: Colour) -> u32 {
    let b = |x: f32| (x.powf(1.0 / 2.2) * 255.0).round() as u32;
    (b(c[0]) << 16) | (b(c[1]) << 8) | b(c[2])
}

/// (base colour, metallic, roughness) for an FM2 material name.
fn material_look(name: &str, paint: Colour, metallic: bool) -> ([f32; 4], f32, f32) {
    let paint_look = ([paint[0], paint[1], paint[2], 1.0], if metallic { 0.7 } else { 0.15 }, if metallic { 0.3 } else { 0.4 });
    match name {
        "body" | "body_2" | "badge" | "race_kit_red" => paint_look,
        "matte_colors" => ([paint[0], paint[1], paint[2], 1.0], 0.0, 0.8),
        "window" | "window_2" | "glass" | "detail_glass_clear" | "lights_gls_head" => ([0.04, 0.06, 0.08, 0.4], 0.0, 0.05),
        "black_glass" => ([0.01, 0.01, 0.015, 0.7], 0.0, 0.05),
        "detail_glass_red" | "red_glass" => ([0.6, 0.02, 0.02, 0.6], 0.0, 0.05),
        "detail_glass_amber" => ([0.8, 0.4, 0.02, 0.6], 0.0, 0.05),
        "chrome" | "emblem" => ([0.85, 0.85, 0.88, 1.0], 1.0, 0.15),
        "textured_reflector" | "reflector" | "head_light" => ([0.8, 0.8, 0.78, 1.0], 0.9, 0.1),
        "tail_light" | "tail_light_on" => ([0.7, 0.03, 0.03, 1.0], 0.0, 0.3),
        "reverse_light" => ([0.8, 0.8, 0.8, 1.0], 0.0, 0.3),
        "interior" => ([0.13, 0.12, 0.11, 1.0], 0.0, 0.8),
        "carbon_fiber" => ([0.03, 0.03, 0.035, 1.0], 0.2, 0.4),
        "rubber_trim" | "weatherstrip" => ([0.02, 0.02, 0.02, 1.0], 0.0, 0.8),
        "black" | "bumper_frame" | "frame" | "plastic2" | "dullplastic" | "misc" | "grille1" => ([0.02, 0.02, 0.02, 1.0], 0.0, 0.6),
        _ => ([0.4, 0.4, 0.4, 1.0], 0.0, 0.7),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// glTF writer

#[derive(Default)]
pub(crate) struct Gltf {
    pub(crate) bin: Vec<u8>,
    pub(crate) views: Vec<Value>,
    pub(crate) accessors: Vec<Value>,
    pub(crate) meshes: Vec<Value>,
    pub(crate) nodes: Vec<Value>,
    pub(crate) materials: Vec<Value>,
    pub(crate) textures: Vec<Value>,
    pub(crate) images: Vec<Value>,
    pub(crate) samplers: Vec<Value>,
    pub(crate) mat_cache: HashMap<String, usize>,
    /// Material name -> texture index (`add_texture`).
    pub(crate) mat_texture: HashMap<String, usize>,
    pub(crate) paint_metallic: bool,
    /// Materials drawn alpha-masked (cut-out textures).
    pub(crate) cutout: std::collections::HashSet<String>,
    pub(crate) triangles: usize,
}

impl Gltf {
    pub(crate) fn view(&mut self, data: &[u8], target: Option<u64>, stride: Option<u64>) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": data.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        if let Some(s) = stride {
            v["byteStride"] = json!(s);
        }
        self.bin.extend_from_slice(data);
        self.views.push(v);
        self.views.len() - 1
    }

    pub(crate) fn accessor(&mut self, view: usize, comp: u64, count: usize, ty: &str, minmax: Option<([f32; 3], [f32; 3])>) -> usize {
        let mut a = json!({"bufferView": view, "componentType": comp, "count": count, "type": ty});
        if let Some((mn, mx)) = minmax {
            a["min"] = json!(mn);
            a["max"] = json!(mx);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    pub(crate) fn add_texture(&mut self, uri: &str) -> usize {
        self.images.push(json!({"uri": uri}));
        // An explicit sampler: linear magnification and trilinear minification, repeating (tread and decal uv leave 0..1). Without one the
        // engine samples the atlas with the default, which blocks up thin livery lines when magnified.
        if self.samplers.is_empty() {
            self.samplers.push(json!({"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}));
        }
        self.textures.push(json!({"source": self.images.len() - 1, "sampler": 0}));
        self.textures.len() - 1
    }

    pub(crate) fn node(&mut self, n: Value) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    pub(crate) fn material(&mut self, name: &str, paint: Colour) -> usize {
        if let Some(&i) = self.mat_cache.get(name) {
            return i;
        }
        let (c, metal, rough) = material_look(name, paint, self.paint_metallic);
        let mut m = json!({"name": name, "pbrMetallicRoughness": {"baseColorFactor": c, "metallicFactor": metal, "roughnessFactor": rough}, "doubleSided": true});
        if let Some(&t) = self.mat_texture.get(&name.to_ascii_lowercase()) {
            m["pbrMetallicRoughness"]["baseColorFactor"] = json!([1.0, 1.0, 1.0, 1.0]);
            m["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": t});
            m["pbrMetallicRoughness"]["metallicFactor"] = json!(0.0);
            m["pbrMetallicRoughness"]["roughnessFactor"] = json!(0.35);
            m.as_object_mut().unwrap().remove("alphaMode");
            if self.cutout.contains(&name.to_ascii_lowercase()) {
                m["alphaMode"] = json!("MASK");
                m["alphaCutoff"] = json!(0.5);
            }
        }
        if c[3] < 1.0 {
            m["alphaMode"] = json!("BLEND");
        }
        self.materials.push(m);
        self.mat_cache.insert(name.to_owned(), self.materials.len() - 1);
        self.materials.len() - 1
    }

    /// One body part -> a mesh + node in world space (centre added), LOD1 only, normals from the triangles.
    pub(crate) fn part(&mut self, p: &Part, c: [f32; 3], flip: bool, paint: Colour, bmin: &mut [f32; 3], bmax: &mut [f32; 3]) -> Option<usize> {
        let mesh = self.part_mesh(p, c, flip, paint, bmin, bmax)?;
        Some(self.node(json!({"name": p.name, "mesh": mesh})))
    }

    pub(crate) fn part_mesh(&mut self, p: &Part, c: [f32; 3], flip: bool, paint: Colour, bmin: &mut [f32; 3], bmax: &mut [f32; 3]) -> Option<usize> {
        let runs: Vec<_> = p.runs.iter().filter(|r| r.lod == 1 && !r.indices.is_empty()).collect();
        if runs.is_empty() {
            return None;
        }
        let pos: Vec<[f32; 3]> = p.vertices.iter().map(|v| [v.position[0] + c[0], v.position[1] + c[1], v.position[2] + c[2]]).collect();
        let mut nrm = vec![[0.0f32; 3]; pos.len()];
        for r in &runs {
            for t in r.indices.chunks_exact(3) {
                let [a, b, cc] = [t[0], t[1], t[2]].map(|i| pos[i as usize]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]]);
                let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                if flip {
                    n = n.map(|x| -x);
                }
                for i in t {
                    for k in 0..3 {
                        nrm[*i as usize][k] += n[k];
                    }
                }
            }
        }
        for n in &mut nrm {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            *n = n.map(|x| x / l);
        }
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for q in &pos {
            for k in 0..3 {
                mn[k] = mn[k].min(q[k]);
                mx[k] = mx[k].max(q[k]);
                bmin[k] = bmin[k].min(q[k]);
                bmax[k] = bmax[k].max(q[k]);
            }
        }
        let bytes = |v: &[[f32; 3]]| -> Vec<u8> { v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect() };
        let uv: Vec<u8> = p.vertices.iter().flat_map(|v| v.uv).flat_map(|x| x.to_le_bytes()).collect();
        let (vp, vn, vt) = (self.view(&bytes(&pos), Some(34962), None), self.view(&bytes(&nrm), Some(34962), None), self.view(&uv, Some(34962), None));
        let pa = self.accessor(vp, 5126, pos.len(), "VEC3", Some((mn, mx)));
        let na = self.accessor(vn, 5126, pos.len(), "VEC3", None);
        let ta = self.accessor(vt, 5126, pos.len(), "VEC2", None);
        let mut prims = Vec::new();
        for r in runs {
            let mut name = p.materials.get(r.material).cloned().unwrap_or_default();
            // Headlight units: the engine's remaster car styling lights materials by name (`head_light` = an emissive lamp, `lights_gls_head`
            // = a cover over one), so the headlight parts' reflector and glass take those names (the tail lamps already are `detail_glass_red`).
            let lower = p.name.to_ascii_lowercase();
            if lower.starts_with("headlight") && (name == "textured_reflector" || name == "reflector") {
                name = "head_light".into();
            } else if lower.starts_with("glass") && lower.ends_with("hl") && (name == "glass" || name == "detail_glass_clear") {
                name = "lights_gls_head".into();
            }
            let idx: Vec<u8> = r
                .indices
                .chunks_exact(3)
                .flat_map(|t| if flip { [t[0], t[2], t[1]] } else { [t[0], t[1], t[2]] })
                .flat_map(|i| i.to_le_bytes())
                .collect();
            let vi = self.view(&idx, Some(34963), None);
            let ia = self.accessor(vi, 5125, r.indices.len(), "SCALAR", None);
            self.triangles += r.indices.len() / 3;
            let m = self.material(&name, paint);
            prims.push(json!({"attributes": {"POSITION": pa, "NORMAL": na, "TEXCOORD_0": ta}, "indices": ia, "material": m}));
        }
        self.meshes.push(json!({"name": p.name, "primitives": prims}));
        Some(self.meshes.len() - 1)
    }

    pub(crate) fn write(mut self, dir: &Path, root: usize) -> Result<()> {
        let mut g = json!({
            "asset": {"version": "2.0", "generator": "fh1setup import-fm2"},
            "scene": 0,
            "scenes": [{"nodes": [root]}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "accessors": self.accessors,
            "bufferViews": self.views,
            "buffers": [{"uri": "model.bin", "byteLength": self.bin.len()}],
        });
        for (k, v) in [("textures", &self.textures), ("images", &self.images), ("samplers", &self.samplers)] {
            if !v.is_empty() {
                g[k] = Value::Array(v.clone());
            }
        }
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        g["buffers"][0]["byteLength"] = json!(self.bin.len());
        std::fs::write(dir.join("model.gltf"), serde_json::to_vec(&g)?)?;
        std::fs::write(dir.join("model.bin"), &self.bin)?;
        Ok(())
    }
}

/// Copies meshes (with their accessors, materials, textures and image files) out of a donor glTF.
pub(crate) struct Copier {
    from: PathBuf,
    to: PathBuf,
    views: HashMap<usize, usize>,
    accessors: HashMap<usize, usize>,
    materials: HashMap<usize, usize>,
    textures: HashMap<usize, usize>,
    images: HashMap<String, usize>,
}

impl Copier {
    pub(crate) fn new(from: &Path, to: &Path) -> Self {
        Self { from: from.to_owned(), to: to.to_owned(), views: HashMap::new(), accessors: HashMap::new(), materials: HashMap::new(), textures: HashMap::new(), images: HashMap::new() }
    }

    fn accessor(&mut self, g: &mut Gltf, d: &Value, bin: &[u8], i: usize) -> Result<usize> {
        if let Some(&n) = self.accessors.get(&i) {
            return Ok(n);
        }
        let mut a = d["accessors"][i].clone();
        let vi = a["bufferView"].as_u64().context("accessor without a view")? as usize;
        let nv = match self.views.get(&vi) {
            Some(&n) => n,
            None => {
                let v = &d["bufferViews"][vi];
                let (off, len) = (v["byteOffset"].as_u64().unwrap_or(0) as usize, v["byteLength"].as_u64().context("view length")? as usize);
                let n = g.view(bin.get(off..off + len).context("donor view out of range")?, v["target"].as_u64(), v["byteStride"].as_u64());
                self.views.insert(vi, n);
                n
            }
        };
        a["bufferView"] = json!(nv);
        g.accessors.push(a);
        self.accessors.insert(i, g.accessors.len() - 1);
        Ok(g.accessors.len() - 1)
    }

    fn texture(&mut self, g: &mut Gltf, d: &Value, i: usize) -> Result<usize> {
        if let Some(&n) = self.textures.get(&i) {
            return Ok(n);
        }
        let t = &d["textures"][i];
        let uri = d["images"][t["source"].as_u64().unwrap_or(0) as usize]["uri"].as_str().context("donor image uri")?.to_owned();
        let img = match self.images.get(&uri) {
            Some(&n) => n,
            None => {
                std::fs::copy(self.from.join(&uri), self.to.join(&uri)).with_context(|| format!("donor image {uri}"))?;
                g.images.push(json!({"uri": uri}));
                self.images.insert(uri, g.images.len() - 1);
                g.images.len() - 1
            }
        };
        let mut nt = json!({"source": img});
        if let Some(s) = t["sampler"].as_u64() {
            g.samplers.push(d["samplers"][s as usize].clone());
            nt["sampler"] = json!(g.samplers.len() - 1);
        }
        g.textures.push(nt);
        self.textures.insert(i, g.textures.len() - 1);
        Ok(g.textures.len() - 1)
    }

    fn material(&mut self, g: &mut Gltf, d: &Value, i: usize) -> Result<usize> {
        if let Some(&n) = self.materials.get(&i) {
            return Ok(n);
        }
        let mut m = d["materials"][i].clone();
        // Every `{"index": n}` under a *Texture key is a texture reference.
        fn walk(c: &mut Copier, g: &mut Gltf, d: &Value, v: &mut Value) -> Result<()> {
            if let Some(o) = v.as_object_mut() {
                for (k, x) in o.iter_mut() {
                    if k.ends_with("Texture") {
                        if let Some(ix) = x["index"].as_u64() {
                            x["index"] = json!(c.texture(g, d, ix as usize)?);
                        }
                    } else {
                        walk(c, g, d, x)?;
                    }
                }
            }
            Ok(())
        }
        walk(self, g, d, &mut m)?;
        g.materials.push(m);
        self.materials.insert(i, g.materials.len() - 1);
        Ok(g.materials.len() - 1)
    }

    pub(crate) fn mesh(&mut self, g: &mut Gltf, d: &Value, bin: &[u8], i: usize) -> Result<usize> {
        let mut m = d["meshes"][i].clone();
        for p in m["primitives"].as_array_mut().into_iter().flatten() {
            for (_, a) in p["attributes"].as_object_mut().into_iter().flatten() {
                *a = json!(self.accessor(g, d, bin, a.as_u64().context("attribute")? as usize)?);
            }
            if let Some(ix) = p["indices"].as_u64() {
                p["indices"] = json!(self.accessor(g, d, bin, ix as usize)?);
            }
            if let Some(mi) = p["material"].as_u64() {
                p["material"] = json!(self.material(g, d, mi as usize)?);
            }
        }
        g.meshes.push(m);
        Ok(g.meshes.len() - 1)
    }
}
