//! FM2 sky: the painted dome the importer wrote to `<map>/scenery/sky.json` + `scenery/sky/textures` (`fh1setup`'s fm2_sky): the sky model's draws are sectors of one dome, each with its own panorama texture; a square one is the zenith cap) drawn unlit around the
//! camera in place of Colorado's procedural sky. The dome follows the camera and sits `SkyboxHeightOffset` below it like the game's. `FH1_FM2_SKY=0` keeps Colorado's sky.

use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::ImageSampler;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use serde_json::Value;

struct SkyDraw {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
    dds: Vec<u8>,
    cap: bool,
}

#[derive(Resource)]
pub struct Fm2Sky {
    draws: Vec<SkyDraw>,
    /// TrackSettings `SkyboxHeightOffset`: the dome's centre relative to the camera, in metres.
    offset_y: f32,
}

#[derive(Component)]
struct SkyDome;

/// Whether this map's sky will be drawn by [`load`]'s result (the caller then switches Colorado's sky off).
pub fn available(native_dir: &Path) -> bool {
    std::env::var("FH1_FM2_SKY").map_or(true, |v| v != "0") && native_dir.join("scenery/sky.json").is_file()
}

pub fn load(native_dir: &Path) -> Option<Fm2Sky> {
    if !available(native_dir) {
        return None;
    }
    let v: Value = serde_json::from_slice(&std::fs::read(native_dir.join("scenery/sky.json")).ok()?).ok()?;
    let tex = |id: u64| -> Option<(Vec<u8>, u32, u32)> {
        let b = std::fs::read(native_dir.join(format!("scenery/sky/textures/{id:08x}.dds"))).ok()?;
        let u = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
        let (h, w) = (u(12)?, u(16)?);
        // BC4 (a mask) is not a colour image.
        (u(128)? != 80).then_some((b, w, h))
    };
    let mut draws = Vec::new();
    for d in v["draws"].as_array()? {
        let positions: Vec<[f32; 3]> = d["positions"].as_array()?.iter().filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32])).collect();
        let Some(file) = d["textures"].as_array().and_then(|t| t.first()).and_then(Value::as_u64) else { continue };
        if positions.len() < 12 {
            continue; // sun glow cards
        }
        let Some((dds, w, h)) = tex(file) else { continue };
        // The draws are sectors of one dome, each with its own panorama texture (square textures are the zenith cap, alpha-blended).
        let cap = w == h;
        let mut uv: Vec<[f32; 2]> = d["uv"].as_array()?.iter().filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32])).collect();
        // Some tracks' vertex shaders scale the 16-bit uv by half: a uv that tops out at 0.5 spans the whole texture.
        let top = uv.iter().fold(0.0f32, |m, t| m.max(t[0]).max(t[1]));
        if top > 0.0 && top < 0.55 {
            for t in &mut uv {
                *t = [t[0] * 2.0, t[1] * 2.0];
            }
        }
        let indices: Vec<u32> = d["indices"].as_array()?.iter().filter_map(|i| i.as_u64().map(|i| i as u32)).collect();
        draws.push(SkyDraw { positions, uv, indices, dds, cap });
    }
    if draws.is_empty() {
        return None;
    }
    // Zenith cap first (behind), the panorama over it.
    draws.sort_by_key(|d| !d.cap);
    let offset_y = std::fs::read_to_string(native_dir.join("tracks/TrackSettings.xml"))
        .ok()
        .and_then(|x| {
            let s = x.find("<SkyboxHeightOffset")?;
            let r = &x[s..];
            let p = r.find("value=\"")? + 7;
            r[p..].split('"').next()?.parse().ok()
        })
        .unwrap_or(0.0);
    Some(Fm2Sky { draws, offset_y })
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(PostUpdate, follow.before(bevy::transform::TransformSystems::Propagate));
}

fn spawn(sky: Option<Res<Fm2Sky>>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let Some(sky) = sky else { return };
    for d in &sky.draws {
        let Some(mut img) = dds_image(&d.dds) else {
            warn!("fm2 sky: a texture failed to load");
            continue;
        };
        img.sampler = ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
            address_mode_u: bevy::image::ImageAddressMode::Repeat,
            address_mode_v: bevy::image::ImageAddressMode::ClampToEdge,
            ..bevy::image::ImageSamplerDescriptor::linear()
        });
        // FM2's world is mirrored in z against the engine's; mirror the dome and flip the winding with it.
        let positions: Vec<[f32; 3]> = d.positions.iter().map(|p| [p[0], p[1], -p[2]]).collect();
        let mut idx = Vec::with_capacity(d.indices.len());
        for t in d.indices.chunks_exact(3) {
            idx.extend_from_slice(&[t[0], t[2], t[1]]);
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        let normals: Vec<[f32; 3]> = positions.iter().map(|p| { let v = Vec3::from(*p); (-v).normalize_or(Vec3::Y).to_array() }).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, d.uv.clone());
        mesh.insert_indices(Indices::U32(idx));
        let material = StandardMaterial {
            base_color_texture: Some(images.add(img)),
            unlit: true,
            cull_mode: None,
            fog_enabled: false,
            alpha_mode: if d.cap { AlphaMode::Blend } else { AlphaMode::Opaque },
            ..default()
        };
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.add(material)), Transform::default(), NoFrustumCulling, SkyDome));
    }
}

fn follow(sky: Option<Res<Fm2Sky>>, cam: Query<&GlobalTransform, With<fh1_render::post::FxPostCamera>>, mut dome: Query<&mut Transform, With<SkyDome>>) {
    let (Some(sky), Some(c)) = (sky, cam.iter().next()) else { return };
    let p = c.translation();
    for mut t in &mut dome {
        t.translation = Vec3::new(p.x, p.y + sky.offset_y, p.z);
    }
}

/// The importer's BC1 / BC2 / BC3 DDS (DX10 header) as a GPU image with its stored mip chain.
fn dds_image(b: &[u8]) -> Option<Image> {
    let u = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()));
    let (h, w, mips, fmt) = (u(12)?, u(16)?, u(28)?.max(1), u(128)?);
    let format = match fmt {
        71 => TextureFormat::Bc1RgbaUnormSrgb,
        74 => TextureFormat::Bc2RgbaUnormSrgb,
        77 => TextureFormat::Bc3RgbaUnormSrgb,
        _ => return None,
    };
    // Level 0 only (the dome is viewed near 1:1).
    let block = if fmt == 71 { 8 } else { 16 };
    let len0 = ((w as usize).div_ceil(4)) * ((h as usize).div_ceil(4)) * block;
    let _ = mips;
    let img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, b.get(148..148 + len0)?.to_vec(), format, RenderAssetUsages::RENDER_WORLD);
    Some(img)
}
