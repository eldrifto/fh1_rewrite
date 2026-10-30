//! Probe: an unpacked FM6 car. `fm6_car_probe <car folder> <library Materials folder> [out.obj]` places every
//! LOD0 mesh by its skeleton bone, resolves every material instance through its parents, and reports what each
//! mesh would be drawn with. With `out.obj` it writes the placed meshes (to a git-ignored folder).
//! Shared-library parts (control arms, brakes, wheels) use bone 0 and are reported, not placed.
use fh1_formats::{materialbin, modelbin, tyre};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn walk(p: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).collect();
        v.sort();
        for e in v {
            walk(&e, ext, out);
        }
    } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
        out.push(p.to_path_buf());
    }
}

/// `Game:\Media\cars\_library\Materials\a\b.materialbin` or `Game:\Media\cars\<car>\Materials\...` -> a file.
fn find_material(index: &[(String, PathBuf)], game_path: &str) -> Option<PathBuf> {
    let p = game_path.replace('\\', "/").to_lowercase();
    let tail = p.rsplit_once("/materials/").map(|(_, t)| t.to_string())?;
    let want_lib = p.contains("/cars/_library/");
    index
        .iter()
        .filter(|(k, _)| k.ends_with(&tail))
        .find(|(k, _)| k.contains("/_lib/") == want_lib)
        .or_else(|| index.iter().find(|(k, _)| k.ends_with(&tail)))
        .map(|(_, f)| f.clone())
}

fn resolve(index: &[(String, PathBuf)], inst: &[u8]) -> (materialbin::Material, Vec<String>) {
    let mut m = materialbin::parse(inst).unwrap_or_default();
    let mut missing = Vec::new();
    for _ in 0..5 {
        let Some(parent) = m.parent.clone() else { break };
        match find_material(index, &parent).and_then(|f| std::fs::read(f).ok()).filter(|d| d.len() >= 20) {
            Some(d) => {
                let p = materialbin::parse(&d).unwrap_or_default();
                m = m.over(&p);
            }
            None => {
                missing.push(parent);
                break;
            }
        }
    }
    (m, missing)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let car = Path::new(&a[1]);
    let mut matfiles = Vec::new();
    walk(Path::new(&a[2]), "materialbin", &mut matfiles);
    let mut index: Vec<(String, PathBuf)> = matfiles.iter().map(|f| (format!("/_lib/{}", f.strip_prefix(&a[2]).unwrap().display()).replace('\\', "/").to_lowercase(), f.clone())).collect();
    let mut carmats = Vec::new();
    walk(&car.join("Materials"), "materialbin", &mut carmats);
    index.extend(carmats.iter().map(|f| (format!("/car/{}", f.strip_prefix(car).unwrap().display()).replace('\\', "/").to_lowercase(), f.clone())));

    let mut skel_file = Vec::new();
    walk(&car.join("scene"), "modelbin", &mut skel_file);
    let sk = skel_file.iter().find(|f| f.to_string_lossy().contains("skeleton")).expect("skeleton modelbin");
    let bones = modelbin::parse_skeleton(&std::fs::read(sk).unwrap()).unwrap();
    println!("skeleton: {} bones", bones.len());

    let mut obj = a.get(3).map(|p| std::io::BufWriter::new(std::fs::File::create(p).unwrap()));
    // tyre sizes from the game database (GameDB.slt, Data_Car): rim diameter in inches, tyre width in mm, aspect in %
    let num = |i: usize, d: f32| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    let sizes = Sizes { rim_in: num(4, 19.0), width_mm: num(5, 225.0), aspect: num(6, 40.0) };
    let locators = parse_locators(&std::fs::read_to_string(car.join("Locators.xml")).unwrap_or_default());
    println!("wheel locators: {:?}", locators.keys().collect::<Vec<_>>());
    let mut checks: BTreeMap<&str, Vec<[f32; 3]>> = ["wheel", "rotor", "caliper", "controlarm"].into_iter().map(|k| (k, Vec::new())).collect();
    let mut library_placed = 0;
    let wheel_w = skel_file
        .iter()
        .find(|f| part_kind(&f.file_stem().unwrap().to_string_lossy().to_lowercase()) == "wheel")
        .and_then(|f| wheel_weights(&modelbin::parse(&std::fs::read(f).unwrap()).ok()?, &sizes));
    println!("wheel morph weights (width, radius): {wheel_w:?}");
    let mut vbase = 1;
    let (mut placed, mut library, mut meshes_n) = (0, 0, 0);
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut shaders: BTreeMap<String, usize> = BTreeMap::new();
    let (mut inst_total, mut inst_missing) = (0, 0);
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for f in &skel_file {
        let d = std::fs::read(f).unwrap();
        let Ok(meshes) = modelbin::parse(&d) else { continue };
        let insts = modelbin::material_instances(&d).unwrap_or_default();
        let resolved: Vec<_> = insts.iter().map(|i| resolve(&index, i)).collect();
        inst_total += resolved.len();
        inst_missing += resolved.iter().filter(|r| !r.1.is_empty()).count();
        let is_library = f.to_string_lossy().contains("_library");
        let stem = f.file_stem().unwrap().to_string_lossy().to_lowercase();
        for m in meshes.iter().filter(|m| m.lod == 0 && m.material != 0xFFFF) {
            meshes_n += 1;
            let copies: Vec<Vec<[f32; 3]>> = if is_library || m.bone == 0 {
                let Some(places) = library_places(&stem, &bones, &locators, &sizes) else {
                    library += 1;
                    continue;
                };
                library_placed += 1;
                // the wheel is sized by its own morph targets (weights solved from the model, see `wheel_weights`)
                let src: Vec<[f32; 3]> = match (&wheel_w, part_kind(&stem)) {
                    (Some(w), "wheel") => m
                        .positions
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let mut q = *p;
                            for (t, wt) in w.iter().enumerate() {
                                if let Some(dv) = m.morphs.get(t).and_then(|d| d.get(i)) {
                                    for k in 0..3 {
                                        q[k] += wt * dv[k];
                                    }
                                }
                            }
                            q
                        })
                        .collect(),
                    _ => m.positions.clone(),
                };
                places.iter().map(|pl| src.iter().map(|p| pl.apply(*p)).collect()).collect()
            } else {
                vec![m.positions.iter().map(|p| bones[m.bone as usize].to_car(*p)).collect()]
            };
            placed += 1;
            for pts in &copies {
                for p in pts {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                if let Some(c) = checks.get_mut(&*part_kind(&stem)) {
                    c.extend(pts.iter().copied());
                }
            }
            let pts: Vec<[f32; 3]> = copies.concat();
            let nv = m.positions.len();
            let (mat, _) = &resolved[m.material as usize];
            let shader = mat.shader.clone().unwrap_or_else(|| "?".into());
            *shaders.entry(shader.clone()).or_default() += 1;
            let has_diff = mat.textures().any(|t| {
                let t = t.to_lowercase();
                t.contains("_diff_") && !t.contains("defaultshader") && !t.contains("defaultch")
            });
            let kind = if shader.starts_with("carpaint") {
                "car paint"
            } else if has_diff {
                "diffuse texture"
            } else if mat.get(0x1f9b6488).or(mat.get(0x938926b0)).is_some() {
                "colour parameter"
            } else {
                "no colour found (default grey)"
            };
            *kinds.entry(kind).or_default() += 1;
            if let Some(o) = obj.as_mut() {
                writeln!(o, "o {}_{}", f.file_stem().unwrap().to_string_lossy(), m.material).unwrap();
                for p in &pts {
                    writeln!(o, "v {} {} {}", p[0], p[1], p[2]).unwrap();
                }
                for c in 0..pts.len() / nv {
                    for t in m.indices.chunks_exact(3) {
                        let b = vbase + c * nv;
                        writeln!(o, "f {} {} {}", t[0] as usize + b, t[1] as usize + b, t[2] as usize + b).unwrap();
                    }
                }
                vbase += pts.len();
            }
        }
    }
    // tyres: our own geometry from the database sizes (INFERRED shape, see fh1_formats::tyre), one per wheel locator
    let tsize = tyre::TyreSize::from_code(sizes.width_mm, sizes.aspect, sizes.rim_in);
    let (tread, sides) = tyre::build(&tsize, 72);
    let mut tyre_pts = Vec::new();
    for (name, at) in &locators {
        let pl = Place { scale: [1.0; 3], half_turn: name.contains("wheelR"), at: *at };
        for (label, part) in [("tread", &tread), ("sidewall", &sides)] {
            let pts: Vec<[f32; 3]> = part.positions.iter().map(|p| pl.apply(*p)).collect();
            if let Some(o) = obj.as_mut() {
                writeln!(o, "o tyre_{label}_{name}").unwrap();
                for p in &pts {
                    writeln!(o, "v {} {} {}", p[0], p[1], p[2]).unwrap();
                }
                for t in part.indices.chunks_exact(3) {
                    writeln!(o, "f {} {} {}", t[0] as usize + vbase, t[1] as usize + vbase, t[2] as usize + vbase).unwrap();
                }
                vbase += pts.len();
            }
            tyre_pts.extend(pts);
        }
    }
    checks.insert("tyre", tyre_pts);
    println!("tyres built: {} + {} triangles each (tread + sidewalls), outer radius {:.3} m", tread.indices.len() / 3, sides.indices.len() / 3, tsize.outer_radius());
    println!("material instances: {inst_total}, unresolved parents: {inst_missing}");
    println!("LOD0 meshes: {meshes_n}; placed: {placed} (of them shared-library parts placed by name: {library_placed}); not placed: {library}");
    let rim_r = sizes.rim_in * 0.0254 / 2.0;
    let tyre_r = rim_r + sizes.width_mm / 1000.0 * sizes.aspect / 100.0;
    println!("sizes: rim radius {rim_r:.3} m, tyre radius {tyre_r:.3} m (tyre not modelled), width {:.3} m", sizes.width_mm / 1000.0);
    for (k, pts) in &checks {
        if pts.is_empty() {
            continue;
        }
        let mut mn = [f32::MAX; 3];
        let mut mx = [f32::MIN; 3];
        for p in pts {
            for i in 0..3 {
                mn[i] = mn[i].min(p[i]);
                mx[i] = mx[i].max(p[i]);
            }
        }
        println!("  {k:10}: x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}", mn[0], mx[0], mn[1], mx[1], mn[2], mx[2]);
    }
    for (name, c) in &locators {
        let hub = bones.iter().find(|b| name.ends_with(&b.name[b.name.len().saturating_sub(2)..]) && b.name.starts_with("hub"));
        if let Some(h) = hub {
            let hp = h.world[3];
            println!(
                "  {name}: centre {c:?}; hub {:?} (offset {:.3} m along the axle); ground under the tyre would be at y = {:.3}",
                [hp[0], hp[1], hp[2]],
                (c[0] - hp[0]).abs(),
                c[1] - tyre_r
            );
        }
    }
    println!("placed car box: min {lo:?} max {hi:?}  size {:?}", [0, 1, 2].map(|k| hi[k] - lo[k]));
    println!("what each placed mesh is drawn with:");
    for (k, n) in &kinds {
        println!("  {k}: {n}");
    }
    println!("shaders used by placed meshes: {}", shaders.len());
    for (s, n) in shaders.iter().take(60) {
        println!("  {n:3} {s}");
    }
}

struct Sizes {
    rim_in: f32,
    width_mm: f32,
    aspect: f32,
}

/// `carLocator_wheelLF` and friends: the translation row (_41, _42, _43) of each `SceneTransform`.
fn parse_locators(xml: &str) -> BTreeMap<String, [f32; 3]> {
    let mut out = BTreeMap::new();
    for part in xml.split("<Locator ").skip(1) {
        let Some(name) = part.split("<Name value=\"").nth(1).and_then(|s| s.split('"').next()) else { continue };
        if !name.starts_with("carLocator_wheel") {
            continue;
        }
        let val = |k: &str| part.split(&format!("value._{k}=\"")).nth(1).and_then(|s| s.split('"').next()).and_then(|v| v.parse::<f32>().ok());
        if let (Some(x), Some(y), Some(z)) = (val("41"), val("42"), val("43")) {
            out.insert(name.to_string(), [x, y, z]);
        }
    }
    out
}

/// How a shared-library part is placed: scale, then (right side) a half turn about the vertical axis, then a move.
struct Place {
    scale: [f32; 3],
    half_turn: bool,
    at: [f32; 3],
}

impl Place {
    fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let s = [p[0] * self.scale[0], p[1] * self.scale[1], p[2] * self.scale[2]];
        let r = if self.half_turn { [-s[0], s[1], -s[2]] } else { s };
        [r[0] + self.at[0], r[1] + self.at[1], r[2] + self.at[2]]
    }
}

fn part_kind(stem: &str) -> &'static str {
    if stem.contains("wheel") {
        "wheel"
    } else if stem.contains("rotor") {
        "rotor"
    } else if stem.contains("caliper") {
        "caliper"
    } else if stem.contains("controlarm") {
        "controlarm"
    } else {
        ""
    }
}

/// Placement of the shared-library parts by name (INFERRED from the part / bone / locator names, not from game code):
/// the wheel model at all four `carLocator_wheelXX`, sized beforehand by its own morph targets (`wheel_weights`). Control arms, rotors and calipers take their sideways position
/// (x) from their `controlArm_XX` / `hubXX` bone but height and fore-aft position (y, z) from the wheel locator of the
/// same corner: the body's wheel arches are centred on the locators (front z +1.21, rear -1.27) while the rear hub
/// bones sit at z -1.52, outside the arch. Right-side brakes and wheels get a half turn (the models are left-side;
/// a half turn keeps the triangle winding, a mirror would not).
fn library_places(stem: &str, bones: &[modelbin::Bone], loc: &BTreeMap<String, [f32; 3]>, sizes: &Sizes) -> Option<Vec<Place>> {
    let side = |s: &str| ["lf", "lr", "rf", "rr"].into_iter().find(|k| s.ends_with(k) || s.contains(&format!("{k}_")));
    let bone_at = |name: &str| bones.iter().find(|b| b.name.eq_ignore_ascii_case(name)).map(|b| [b.world[3][0], b.world[3][1], b.world[3][2]]);
    let corner = |bone: &str, s: &str| -> Option<[f32; 3]> {
        let b = bone_at(bone)?;
        let l = loc.get(&format!("carLocator_wheel{}", s.to_uppercase()))?;
        Some([b[0], l[1], l[2]])
    };
    match part_kind(stem) {
        "controlarm" => {
            let s = side(stem.trim_end_matches("_a"))?;
            Some(vec![Place { scale: [1.0; 3], half_turn: false, at: corner(&format!("controlArm_{}", s.to_uppercase()), s)? }])
        }
        "rotor" | "caliper" => {
            let s = side(stem.split('_').nth(1).unwrap_or(""))?;
            Some(vec![Place { scale: [1.0; 3], half_turn: s.starts_with('r'), at: corner(&format!("hub{}", s.to_uppercase()), s)? }])
        }
        "wheel" => {
            let _ = sizes; // sized by morph weights, not by scaling
            let mut v = Vec::new();
            for (name, at) in loc {
                v.push(Place { scale: [1.0; 3], half_turn: name.contains("wheelR"), at: *at });
            }
            Some(v)
        }
        _ => None,
    }
}

/// Morph weights that size the normalised wheel to the database rim: read from the model itself, no constants.
/// Radius target (last): the vertex farthest from the axle has radius r0 and moves out by dr per unit weight, so
/// weight = (rim radius - r0) / dr. Width target (first): the barrel's inner edge sits at x0 and moves by dx per unit
/// weight; the rim width is taken as the tyre width (INFERRED: the database has no rim width), weight = (width - x0) / dx.
fn wheel_weights(meshes: &[modelbin::Mesh], sizes: &Sizes) -> Option<Vec<f32>> {
    let lod0: Vec<_> = meshes.iter().filter(|m| m.lod == 0 && m.material != 0xFFFF && m.morphs.len() == 2).collect();
    let mut far = (0.0f32, 0.0f32); // (radius, radial change)
    let mut inner = (f32::MIN, 0.0f32); // (x, axle change)
    for m in &lod0 {
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
    if far.1.abs() < 1e-6 || inner.1.abs() < 1e-6 {
        return None;
    }
    let rim_r = sizes.rim_in * 0.0254 / 2.0;
    let width = sizes.width_mm / 1000.0;
    println!(
        "wheel model: base rim radius {:.3} m (+{:.3} per unit weight), barrel inner edge at {:.3} ({:+.3} per unit weight)",
        far.0, far.1, inner.0, inner.1
    );
    Some(vec![(width - inner.0) / inner.1, (rim_r - far.0) / far.1])
}
