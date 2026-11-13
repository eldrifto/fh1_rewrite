//! FM2 props: the per-track objects the `.rmb` scenery does not contain -- cones, poles and brake markers.
//!
//! The scenery models (`<track>out.NNNNN.rmb.bin`) are the static world. Everything else the game puts on the track
//! comes from `<track>/<ribbon>/CollObjs.xml`, one `ObjN` per placement, each naming its own model file at the track
//! root (`co_Orgcone.bin`, `co_Cone.bin`, `co_BrakeMarker01.bin`, ...). Those models are `CAFF` containers, the same
//! format the scenery reader already parses.
//!
//! Output, in the format the engine's prop system already reads:
//!   * `props/templates/<n>.bin` -- one tile-format mesh per distinct model, in LOCAL space;
//!   * `props/tiles/<x>_<z>.bin` -- `b"FH1PROP3"`, u32 count, then per placement
//!     `u32 template, f32x16 column-major matrix, f32x3 normal, u32 tint, u32x2 lightmaps` (92 bytes);
//!   * the `props` section of `index.json` lists both.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use fh1_formats::fm2::{self, CollObj};
use serde_json::{json, Value};

use crate::scenery::{write_tile, Batch};

/// Tile size the engine buckets placements by (`props.tile_size` in index.json).
const PROP_TILE: f32 = 256.0;

/// Every `CollObjs.xml` under a track folder (one per ribbon), parsed into one list.
pub fn read(track_dir: &Path) -> Vec<CollObj> {
    let Ok(rd) = std::fs::read_dir(track_dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(xml) = std::fs::read_to_string(e.path().join("CollObjs.xml")) else { continue };
        out.extend(fh1_formats::fm2::parse_collobjs(&xml));
    }
    out
}

/// One line for the import log: object count, distinct models, and the kinds with their counts.
pub fn summary(objs: &[CollObj]) -> String {
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut models: BTreeSet<String> = BTreeSet::new();
    for o in objs {
        *kinds.entry(o.kind.as_str()).or_default() += 1;
        models.insert(o.model.to_ascii_lowercase());
    }
    let k: Vec<String> = kinds.iter().map(|(k, n)| format!("{k} x{n}")).collect();
    format!("{} objects, {} models ({})", objs.len(), models.len(), k.join(", "))
}

/// The model file on disk, tolerating the XML's capitalisation (`co_Orgcone.bin` vs `co_orgcone.bin`).
fn find_model(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let direct = dir.join(name);
    if let Ok(b) = std::fs::read(&direct) {
        return Some(b);
    }
    let want = name.to_ascii_lowercase();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if e.file_name().to_string_lossy().to_ascii_lowercase() == want {
            if let Ok(b) = std::fs::read(e.path()) {
                return Some(b);
            }
        }
    }
    None
}

/// Per-vertex normals: the area-weighted average of the faces that use each vertex.
fn smooth_normals(pos: &[[f32; 3]], idx: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; pos.len()];
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &k in t {
            let nn = &mut n[k as usize];
            nn[0] += f[0];
            nn[1] += f[1];
            nn[2] += f[2];
        }
    }
    for v in &mut n {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 1e-9 {
            *v = [v[0] / l, v[1] / l, v[2] / l];
        } else {
            *v = [0.0, 1.0, 0.0];
        }
    }
    n
}

/// The material to draw a prop draw with: the first record that samples the draw's first texture anywhere.
/// Falls back to record 0, which every track has.
fn material_for(materials: &[Value], tex: Option<u32>) -> u32 {
    let Some(id) = tex else { return 0 };
    for (i, m) in materials.iter().enumerate() {
        if m["textures"].as_array().into_iter().flatten().any(|t| t["id"].as_u64() == Some(id as u64)) {
            return i as u32;
        }
    }
    0
}

/// Write the templates and the placement tiles. Returns (`props.templates`, `props.tiles`) for index.json.
pub fn write_collobj_props(out: &Path, disc_track: &Path, objs: &[CollObj], pvs: &fm2::Pvs, materials: &[Value]) -> Result<(Vec<u16>, Vec<Value>)> {
    if objs.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    // The disc lists the same object more than once on most tracks (every ribbon's `CollObjs.xml` carries it, or
    // the file repeats it), with an IDENTICAL transform: measured on the installed maps, Sebring 556 entries ->
    // 196 distinct, TT 578 -> 139, Silverstone 496 -> 203, Sunset 196 -> 61, Road Atlanta 80 -> 42, Tsukuba 50 ->
    // 36, Suzuka 38 -> 21; Nurburgring and Laguna have none. Two objects with the same model at the same position
    // and axes are one object: drawing both costs a second draw and a second shadow, and doubles the opacity of a
    // blended prop. Deduplicated here, in the importer, so the engine and the tile files agree.
    let mut seen = std::collections::HashSet::new();
    let listed = objs.len();
    let objs: Vec<&CollObj> = objs
        .iter()
        .filter(|o| {
            let key = (
                o.model.to_ascii_lowercase(),
                o.pos.map(f32::to_bits),
                o.axes.map(|a| a.map(f32::to_bits)),
            );
            seen.insert(key)
        })
        .collect();
    if objs.len() != listed {
        println!("[fm2] props: {listed} entries -> {} after deduplicating identical placements", objs.len());
    }
    let objs = &objs;
    // Template number per distinct model, in a stable order so a re-import gives the same files.
    let models: BTreeSet<String> = objs.iter().map(|o| o.model.to_ascii_lowercase()).collect();
    let mut number: BTreeMap<String, u16> = BTreeMap::new();
    for (i, m) in models.iter().enumerate() {
        number.insert(m.clone(), i as u16);
    }
    let mut templates = Vec::new();
    for (name, n) in &number {
        let Some(raw) = find_model(disc_track, name) else {
            println!("[fm2] props: model {name} not found on the disc");
            continue;
        };
        let Ok(model) = fm2::parse_model(&raw) else {
            println!("[fm2] props: model {name} did not parse");
            continue;
        };
        let mut batches: BTreeMap<(u32, u32, u32), Batch> = BTreeMap::new();
        for d in &model.draws {
            let tex = d.items.first().and_then(|&i| pvs.textures.get(i as usize)).map(|t| t.file_id);
            let material = material_for(materials, tex);
            let (uv0, _) = d.uv_offsets();
            let uv = d.uv_at(uv0);
            // T1 diagnostic (MODLOG 495): give the template the same vertex layout the scenery uses (uv0/uv1/uv2 +
            // colour) -- the last untested difference between a prop template and a scenery mesh that does rasterise.
            let full = std::env::var_os("FM2_PROP_ATTRS").is_some();
            let mask = if full {
                crate::scenery::ATTR_UV0 | crate::scenery::ATTR_UV1 | crate::scenery::ATTR_UV2 | crate::scenery::ATTR_COLOR
            } else {
                crate::scenery::ATTR_UV0
            };
            let b = batches.entry((material, 0, mask)).or_default();
            let base = b.positions.len() as u32;
            b.positions.extend_from_slice(&d.positions);
            b.normals.extend(smooth_normals(&d.positions, &d.indices));
            b.uvs[0].extend(uv.iter().copied());
            if full {
                b.uvs[1].extend(uv.iter().copied());
                b.uvs[2].extend(uv.iter().copied());
                b.colours.extend(std::iter::repeat([255, 255, 255, 255]).take(d.positions.len()));
            }
            // FM2 prop models may wind the opposite way to the engine's templates; `FM2_PROP_WIND=1` swaps each
            // triangle to test that (the cone currently draws nothing either way, see MODLOG 488).
            let flip = std::env::var("FM2_PROP_WIND").as_deref() == Ok("1");
            b.indices.extend(d.indices.chunks_exact(3).flat_map(|t| if flip { [t[0] + base, t[2] + base, t[1] + base] } else { [t[0] + base, t[1] + base, t[2] + base] }));
        }
        if batches.is_empty() {
            println!("[fm2] props: model {name} has no draws");
            continue;
        }
        write_tile(&out.join(format!("props/templates/{n}.bin")), &batches)?;
        templates.push(*n);
    }

    // Placements, bucketed by the engine's tile size.
    let mut tiles: BTreeMap<(i32, i32), Vec<u8>> = BTreeMap::new();
    let mut count = 0usize;
    for o in objs {
        let Some(&n) = number.get(&o.model.to_ascii_lowercase()) else { continue };
        let [ax, ay, az] = o.axes;
        // Engine space is right-handed with Z negated relative to the disc's world coordinates (fh1-formats
        // `Placement::engine_matrix`): the camera at the Nordschleife grid sits at z -1918 while the spawn is +1915, so a
        // disc-space +z placement lands 3.8 km away. Axes map by (x, y, -z), the Z axis then flips again, position
        // (x, y, -z) -- exactly FH1's rule.
        let f = |v: [f32; 3]| [v[0], v[1], -v[2]];
        let (ax, ay, az, p) = (f(ax), f(ay), f(az), f(o.pos));
        let az = [-az[0], -az[1], -az[2]];
        // The tile key must come from the ENGINE-space position: bucketing by the disc's +z put every placement in a tile
        // 3.8 km from its own geometry, so no tile was ever streamed (MODLOG 488).
        let (x, z) = ((p[0] / PROP_TILE).floor() as i32, (p[2] / PROP_TILE).floor() as i32);
        // Column-major: X, Y, Z axes then the translation.
        let mut m = [0.0f32; 16];
        for k in 0..3 {
            m[k] = ax[k];
            m[4 + k] = ay[k];
            m[8 + k] = az[k];
            m[12 + k] = p[k];
        }
        m[15] = 1.0;
        let e = tiles.entry((x, z)).or_insert_with(|| {
            let mut v = Vec::new();
            v.extend_from_slice(b"FH1PROP3");
            v.extend_from_slice(&0u32.to_le_bytes());
            v
        });
        e.extend_from_slice(&(n as u32).to_le_bytes());
        for f in m {
            e.extend_from_slice(&f.to_le_bytes());
        }
        for f in ay {
            e.extend_from_slice(&f.to_le_bytes());
        }
        e.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // tint: none
        e.extend_from_slice(&u32::MAX.to_le_bytes()); // LOD0 night lightmap
        e.extend_from_slice(&u32::MAX.to_le_bytes()); // LOD1 night lightmap
        count += 1;
    }
    let mut tile_index = Vec::new();
    for ((x, z), mut bytes) in tiles {
        let n = (bytes.len() - 12) / 92;
        bytes[8..12].copy_from_slice(&(n as u32).to_le_bytes());
        let file = format!("props/tiles/{x}_{z}.bin");
        std::fs::write(out.join(&file), &bytes)?;
        tile_index.push(json!({"x": x, "z": z, "file": file, "placements": n}));
    }
    println!("[fm2] props: {} templates, {} placements in {} tiles", templates.len(), count, tile_index.len());
    Ok((templates, tile_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(kind: &str, model: &str) -> CollObj {
        CollObj { kind: kind.into(), model: model.into(), pos: [0.0; 3], axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] }
    }

    #[test]
    fn summary_counts_kinds_and_models() {
        let v = vec![obj("Cone", "co_Orgcone.bin"), obj("Cone", "co_ORGcone.bin"), obj("Pole", "co_Pole.bin")];
        let s = summary(&v);
        assert_eq!(s, "3 objects, 2 models (Cone x2, Pole x1)");
    }

    #[test]
    fn normals_point_out_of_a_triangle() {
        let n = smooth_normals(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], &[0, 1, 2]);
        assert!(n[0][1].abs() > 0.9, "{n:?}");
    }

    /// Counts straight off the extracted disc, when `FM2_DISC_TREE` points at it (`data/work_fm2/disc`).
    /// Skipped otherwise, so a checkout without the disc still passes.
    #[test]
    fn counts_on_the_disc_when_asked() {
        let Ok(root) = std::env::var("FM2_DISC_TREE") else { return };
        for (tr, n) in [
            ("nurburgring", 70),
            ("silverstone", 496),
            ("sebring", 556),
            ("tsukuba", 50),
            ("mugello", 6),
            ("lagunaseca", 27),
            ("roadatlanta", 80),
            ("suzuka", 38),
            ("sunsetinfield", 196),
            ("testtrackinfield", 578),
            ("maplevalley", 0),
            ("newyork", 0),
        ] {
            let v = read(Path::new(&format!("{root}/Media/tracks/{tr}")));
            assert_eq!(v.len(), n, "{tr}");
        }
    }
}
