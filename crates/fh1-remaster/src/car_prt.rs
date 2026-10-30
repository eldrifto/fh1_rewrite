//! FM2 car body light from the game's own data (MODLOG 76): the car file's vertices carry 16 precomputed-radiance-transfer numbers (the body's
//! self-shadowing baked in), the game lights the car with D3DX spherical-harmonic lights (a hemisphere from the track's `SHTop` / `SHBottom`
//! and one directional light, `SHDir`), and the vertex shader dots the two. This does the same on the CPU: per car mesh a lit colour per vertex
//! is written into the mesh's COLOR_0 whenever the car turns against the sun, and the paint shader (car_paint.rs) uses it as its diffuse light.
//!
//! Off unless `FH1_FM2_CAR_PRT=1` (`FH1_FM2_CAR_PRT_GAIN` scales it, default 1: the paint's overall level is INFERRED, not read).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use bevy::gltf::GltfMeshName;
use bevy::light::DirectionalLight;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use fh1_render::car::FxCarBody;

/// The track's `<SHLighting>` colours (linear game values), set once by the engine for FM2 circuits.
#[derive(Clone, Copy, Debug)]
pub struct TrackSh {
    pub top: [f32; 3],
    pub bottom: [f32; 3],
    pub dir: [f32; 3],
    /// `SHEnvMapTop` / `SHEnvMapBottom`: the hemisphere the body's environment reflection is occluded against (the VS `shEnv` block).
    pub env_top: f32,
    pub env_bottom: f32,
}

static TRACK_SH: OnceLock<TrackSh> = OnceLock::new();

pub fn set_track_sh(sh: TrackSh) {
    let _ = TRACK_SH.set(sh);
}

/// Paint gain for the lit colour (0 = the feature is off): `FH1_FM2_CAR_PRT=1` turns it on, `FH1_FM2_CAR_PRT_GAIN` scales it (default 1).
pub fn gain() -> f32 {
    if TRACK_SH.get().is_none() || !std::env::var("FH1_FM2_CAR_PRT").is_ok_and(|v| v != "0") {
        return 0.0;
    }
    std::env::var("FH1_FM2_CAR_PRT_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)
}

/// The multiplier the game puts on a car pixel the sun does not reach (the body shader's `lerp(ShadowColour, 1, visibility)`): the track's
/// ShadowColor (green, `FH1_RM_FM2_SHADOW_COL`, set by the engine) raised to the display gamma, because the game multiplies display-coded colours.
/// `FH1_FM2_CAR_SHADE=0` leaves it out; `FH1_FM2_CAR_SHADE_GAMMA` (default 2.2) is the exponent.
pub fn shade() -> f32 {
    if gain() <= 0.0 || std::env::var("FH1_FM2_CAR_SHADE").is_ok_and(|v| v == "0") {
        return 0.0;
    }
    let sc: f32 = std::env::var("FH1_RM_FM2_SHADOW_COL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    let g: f32 = std::env::var("FH1_FM2_CAR_SHADE_GAMMA").ok().and_then(|v| v.parse().ok()).unwrap_or(2.2);
    sc.clamp(0.0, 1.0).powf(g).max(1e-4)
}

type Parts = HashMap<String, Arc<Vec<[f32; 16]>>>;

#[derive(Resource, Default)]
struct PrtLibrary(HashMap<std::path::PathBuf, Option<Arc<Parts>>>);

/// On a car mesh entity: its transfer numbers (scaled to light units) and the light the colours were last made for.
#[derive(Component)]
struct PrtMesh {
    coeffs: Arc<Vec<[f32; 16]>>,
    normals: Vec<Vec3>,
    sun: Vec3,
    up: Vec3,
    made: bool,
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<PrtLibrary>().add_systems(Update, (attach, update).chain());
}

/// The numbers are signed 16 bit = (transfer / pi) x sqrt(pi) x 32768 (MODLOG 76: an unoccluded surface under a uniform white light of 1 reads
/// 1 after dividing by sqrt(pi)), so the factor to light units is 1 / (32768 x sqrt(pi)).
const SCALE: f32 = 1.0 / (32768.0 * 1.772_453_9);

fn load(path: &std::path::Path) -> Option<Arc<Parts>> {
    let b = std::fs::read(path).ok()?;
    if b.get(0..4)? != b"PRT1" {
        return None;
    }
    let n = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
    let mut p = 8;
    let mut out = Parts::new();
    for _ in 0..n {
        let nl = u16::from_le_bytes(b.get(p..p + 2)?.try_into().ok()?) as usize;
        let name = String::from_utf8_lossy(b.get(p + 2..p + 2 + nl)?).into_owned();
        p += 2 + nl;
        let vc = u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?) as usize;
        p += 4;
        let raw = b.get(p..p + vc * 32)?;
        p += vc * 32;
        let v: Vec<[f32; 16]> = raw.chunks_exact(32).map(|c| std::array::from_fn(|k| i16::from_le_bytes([c[2 * k], c[2 * k + 1]]) as f32 * SCALE)).collect();
        out.insert(name, Arc::new(v));
    }
    Some(Arc::new(out))
}

fn attach(
    mut commands: Commands,
    new: Query<(Entity, &GltfMeshName, &Mesh3d), Without<PrtMesh>>,
    parents: Query<&ChildOf>,
    bodies: Query<&FxCarBody>,
    mut lib: ResMut<PrtLibrary>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if gain() <= 0.0 {
        return;
    }
    for (e, name, mesh) in &new {
        let Some(body) = parents.iter_ancestors(e).find_map(|a| bodies.get(a).ok()) else { continue };
        let dir = body.assets.join("cars").join(&body.car);
        let parts = lib.0.entry(dir.clone()).or_insert_with(|| {
            let p = load(&dir.join("prt.bin"));
            if p.is_none() {
                warn!("{}: no prt.bin (run fh1setup import-fm2); the car keeps the engine's lighting", body.car);
            }
            p
        });
        let Some(coeffs) = parts.as_ref().and_then(|p| p.get(&name.0)).cloned() else { continue };
        let Some(src) = meshes.get(&mesh.0) else { continue };
        if src.count_vertices() != coeffs.len() {
            continue;
        }
        // A private copy: cars of one model share their mesh assets, but not their light.
        let mut copy = src.clone();
        let normals: Vec<Vec3> = match copy.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(n)) => n.iter().map(|v| Vec3::from(*v)).collect(),
            _ => continue,
        };
        // COLOR_0 = the lit colour; UV_1 = (sun visibility, environment visibility) for the shade and the reflection (car_paint.rs).
        copy.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32, 0.0, 0.0, 1.0]; coeffs.len()]);
        copy.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[1.0f32, 1.0]; coeffs.len()]);
        let handle = meshes.add(copy);
        commands.entity(e).insert((Mesh3d(handle), PrtMesh { coeffs, normals, sun: Vec3::ZERO, up: Vec3::ZERO, made: false }));
    }
}

/// D3DX real SH basis, 16 terms (l = 0..3), for a unit direction in the car file's frame (x right, y up, z back).
fn basis(d: Vec3) -> [f32; 16] {
    let (x, y, z) = (d.x, d.y, d.z);
    [
        0.282_095,
        -0.488_603 * y,
        0.488_603 * z,
        -0.488_603 * x,
        1.092_548 * x * y,
        -1.092_548 * y * z,
        0.315_392 * (3.0 * z * z - 1.0),
        -1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
        -0.590_044 * y * (3.0 * x * x - y * y),
        2.890_611 * x * y * z,
        -0.457_046 * y * (5.0 * z * z - 1.0),
        0.373_176 * z * (5.0 * z * z - 3.0),
        -0.457_046 * x * (5.0 * z * z - 1.0),
        1.445_306 * z * (x * x - y * y),
        -0.590_044 * x * (x * x - 3.0 * y * y),
    ]
}

/// SH (R, G, B x 16) of the track light in the car's frame: the hemisphere (top over `up`, bottom under it: radiance = bottom + (top - bottom) x
/// (0.5 + 0.5 cos)) plus one directional light from `sun` (the direction towards it).
fn light_sh(sh: &TrackSh, up: Vec3, sun: Vec3) -> [[f32; 16]; 3] {
    let pi = std::f32::consts::PI;
    let bu = basis(up);
    let bs = basis(sun);
    let mut out = [[0.0f32; 16]; 3];
    for k in 0..3 {
        let (t, b) = (sh.top[k], sh.bottom[k]);
        out[k][0] = 2.0 * pi * 0.282_095 * (t + b);
        for i in 1..4 {
            out[k][i] = (2.0 * pi / 3.0) * (t - b) * bu[i];
        }
        for i in 0..16 {
            out[k][i] += sh.dir[k] * bs[i];
        }
    }
    out
}

fn update(
    sun: Query<(&DirectionalLight, &GlobalTransform)>,
    mut parts: Query<(&GlobalTransform, &mut PrtMesh, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(track) = TRACK_SH.get() else { return };
    let Some((_, sun_t)) = sun.iter().max_by(|a, b| a.0.illuminance.total_cmp(&b.0.illuminance)) else { return };
    let mut to_sun = -Vec3::from(sun_t.forward());
    // `FH1_RM_FM2_PRT_DIR=x,y,z` (engine space, towards the sun; the engine sets it from the sky model's sun disc under `FH1_FM2_PRT_SKY=1`).
    if let Some(v) = std::env::var("FH1_RM_FM2_PRT_DIR").ok().map(|s| s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 3) {
        to_sun = Vec3::new(v[0], v[1], v[2]).normalize_or(to_sun);
    }
    for (gt, mut prt, mesh) in &mut parts {
        let inv = gt.to_scale_rotation_translation().1.inverse();
        let (up, sun_l) = (inv * Vec3::Y, inv * to_sun);
        if prt.made && prt.up.dot(up) > 0.9995 && prt.sun.dot(sun_l) > 0.9995 {
            continue;
        }
        let l = light_sh(track, up, sun_l);
        // The sun alone, for the visibility: a delta light of colour 1 reads (visibility x cos / pi) on a vertex (the numbers are transfer / pi).
        let bs = basis(sun_l);
        // The environment hemisphere (SHEnvMapTop / Bottom) for the reflection's occlusion: 1 on an unoccluded vertex under a uniform 1.
        let env = light_sh(&TrackSh { top: [track.env_top; 3], bottom: [track.env_bottom; 3], dir: [0.0; 3], env_top: 0.0, env_bottom: 0.0 }, up, sun_l)[0];
        let Some(mut m) = meshes.get_mut(&mesh.0) else { continue };
        let mut colours: Vec<[f32; 4]> = Vec::with_capacity(prt.coeffs.len());
        let mut vis: Vec<[f32; 2]> = Vec::with_capacity(prt.coeffs.len());
        for (t, n) in prt.coeffs.iter().zip(&prt.normals) {
            let mut c = [0.0f32; 4];
            for k in 0..3 {
                c[k] = (0..16).map(|i| t[i] * l[k][i]).sum::<f32>().max(0.0);
            }
            c[3] = 1.0;
            colours.push(c);
            let sun_term: f32 = (0..16).map(|i| t[i] * bs[i]).sum();
            let cos = n.dot(sun_l);
            let sun_vis = if cos > 0.1 { (sun_term * std::f32::consts::PI / cos).clamp(0.0, 1.0) } else { 0.0 };
            let env_vis = (0..16).map(|i| t[i] * env[i]).sum::<f32>().clamp(0.0, 1.0);
            vis.push([sun_vis, env_vis]);
        }
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, VertexAttributeValues::Float32x4(colours));
        m.insert_attribute(Mesh::ATTRIBUTE_UV_1, VertexAttributeValues::Float32x2(vis));
        prt.up = up;
        prt.sun = sun_l;
        prt.made = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(top: f32, bottom: f32, dir: f32) -> TrackSh {
        TrackSh { top: [top; 3], bottom: [bottom; 3], dir: [dir; 3], env_top: 1.0, env_bottom: 1.0 }
    }

    #[test]
    fn uniform_light_is_the_dc_term_only() {
        let l = light_sh(&sh(1.0, 1.0, 0.0), Vec3::Y, Vec3::Y);
        // The SH of a uniform radiance of 1 is sqrt(4 pi) x Y00 = 1 -> coefficient sqrt(4 pi).
        assert!((l[0][0] - (4.0 * std::f32::consts::PI).sqrt()).abs() < 1e-3, "{}", l[0][0]);
        assert!(l[0][1..].iter().all(|v| v.abs() < 1e-5));
    }

    #[test]
    fn hemisphere_light_leans_towards_its_up_vector() {
        // Top 1 over `up`, bottom 0: the l = 1 terms follow `up` with the D3DX signs (-y, +z, -x).
        let l = light_sh(&sh(1.0, 0.0, 0.0), Vec3::Y, Vec3::Y)[0];
        assert!(l[1] < -0.1 && l[2].abs() < 1e-5 && l[3].abs() < 1e-5, "{l:?}");
        let l = light_sh(&sh(1.0, 0.0, 0.0), Vec3::Z, Vec3::Y)[0];
        assert!(l[2] > 0.1 && l[1].abs() < 1e-5, "{l:?}");
    }

    #[test]
    fn an_unoccluded_vertex_reads_one_under_a_uniform_light() {
        // Stored transfer of an open surface: DC = 0.5 of full scale (MODLOG 76) -> 0.5 x sqrt(4 pi) / sqrt(pi) = 1.
        let dc = 16_384.0 * SCALE;
        let l = light_sh(&sh(1.0, 1.0, 0.0), Vec3::Y, Vec3::Y)[0];
        assert!((dc * l[0] - 1.0).abs() < 0.01, "{}", dc * l[0]);
    }

    #[test]
    fn prt_file_round_trip() {
        let mut b = b"PRT1".to_vec();
        b.extend(1u32.to_le_bytes());
        b.extend(4u16.to_le_bytes());
        b.extend(b"body");
        b.extend(2u32.to_le_bytes());
        for v in 0..32i16 {
            b.extend((v * 100).to_le_bytes());
        }
        let dir = std::env::temp_dir().join("fh1_prt_test.bin");
        std::fs::write(&dir, &b).unwrap();
        let parts = load(&dir).unwrap();
        let body = &parts["body"];
        assert_eq!(body.len(), 2);
        assert!((body[1][0] - 1600.0 * SCALE).abs() < 1e-9);
        let _ = std::fs::remove_file(&dir);
        // A truncated file is refused rather than half read.
        std::fs::write(&dir, &b[..b.len() - 3]).unwrap();
        assert!(load(&dir).is_none());
        let _ = std::fs::remove_file(&dir);
    }
}
