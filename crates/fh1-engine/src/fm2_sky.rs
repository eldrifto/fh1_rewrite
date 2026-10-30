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
    /// The sun disc (a sphere mesh with no texture): drawn additive in a flat yellow.
    sun: bool,
}

/// The sun glow card: a quad at the sun with a grey flare picture (importer `sky/glow.png`), drawn additive.
struct GlowCard {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// PNG bytes of the grey flare picture.
    png: Vec<u8>,
}

#[derive(Resource)]
pub struct Fm2Sky {
    draws: Vec<SkyDraw>,
    glow: Option<GlowCard>,
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
        // `FH1_FM2_SKY_TEXDIR` = a folder of replacement textures (diagnostics: e.g. the alpha channel as a picture).
        let dir = std::env::var("FH1_FM2_SKY_TEXDIR").map(std::path::PathBuf::from).unwrap_or_else(|_| native_dir.join("scenery/sky/textures"));
        let b = std::fs::read(dir.join(format!("{id:08x}.dds"))).ok()?;
        let u = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
        let (h, w) = (u(12)?, u(16)?);
        // BC4 (a mask) is not a colour image.
        (u(128)? != 80).then_some((b, w, h))
    };
    let mut draws = Vec::new();
    let mut glow = None;
    for (di, d) in v["draws"].as_array()?.iter().enumerate() {
        let positions: Vec<[f32; 3]> = d["positions"].as_array()?.iter().filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32])).collect();
        let first = d["textures"].as_array().and_then(|t| t.first()).and_then(Value::as_u64);
        if positions.len() < 12 {
            // The sun glow card (opt-in: `FH1_FM2_GLOW=1`; no Xenia shot shows the sun, so its level is not verified). Its corners come in the order (1,1) (1,0) (0,1) (0,0) of the card's picture on
            // every track checked, so that is the uv; the flare is symmetric, so a wrong turn would not show.
            if positions.len() == 4 && glow.is_none() && std::env::var("FH1_FM2_GLOW").is_ok_and(|v| v == "1") {
                if let (Some(f), Some(png)) = (d["glow"].as_str(), d["glow"].as_str().and_then(|f| std::fs::read(native_dir.join("scenery").join(f)).ok())) {
                    let _ = f;
                    let indices: Vec<u32> = d["indices"].as_array()?.iter().filter_map(|i| i.as_u64().map(|i| i as u32)).collect();
                    glow = Some(GlowCard { positions, uv: vec![[1.0, 1.0], [1.0, 0.0], [0.0, 1.0], [0.0, 0.0]], indices, png });
                }
            }
            continue;
        }
        let Some(file) = first else {
            // No texture: the sun disc (`FH1_FM2_SUN=0` leaves it out).
            if std::env::var("FH1_FM2_SUN_DISC").is_ok_and(|v| v == "0") || di == 0 {
                continue;
            }
            let uv: Vec<[f32; 2]> = vec![[0.0; 2]; positions.len()];
            let indices: Vec<u32> = d["indices"].as_array()?.iter().filter_map(|i| i.as_u64().map(|i| i as u32)).collect();
            draws.push(SkyDraw { positions, uv, indices, dds: Vec::new(), cap: false, sun: true });
            continue;
        };
        let Some((dds, w, h)) = tex(file) else { continue };
        // The draws are sectors of one dome, each with its own panorama texture (square textures are the zenith cap, alpha-blended).
        // The sky model lists `shader_sky_cap` first, then the four sector passes (`shader_sky1..4`): the cap is draw 0. (Square textures do not
        // tell: Sunset Infield's sectors are 1024 x 1024 too. `FH1_FM2_SKY_SQUARE_CAP=1` = the old rule.)
        let cap = if std::env::var_os("FH1_FM2_SKY_SQUARE_CAP").is_some() { w == h } else { di == 0 };
        let mut uv: Vec<[f32; 2]> = d["uv"].as_array()?.iter().filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32])).collect();
        // Some tracks' vertex shaders scale the 16-bit uv by half: a uv that tops out at 0.5 spans the whole texture.
        let top = uv.iter().fold(0.0f32, |m, t| m.max(t[0]).max(t[1]));
        if top > 0.0 && top < 0.55 {
            for t in &mut uv {
                *t = [t[0] * 2.0, t[1] * 2.0];
            }
        }
        let indices: Vec<u32> = d["indices"].as_array()?.iter().filter_map(|i| i.as_u64().map(|i| i as u32)).collect();
        draws.push(SkyDraw { positions, uv, indices, dds, cap, sun: false });
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
    Some(Fm2Sky { draws, glow, offset_y })
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(PostUpdate, follow.before(bevy::transform::TransformSystems::Propagate));
}

fn spawn(sky: Option<Res<Fm2Sky>>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let Some(sky) = sky else { return };
    info!("fm2 sky: {} draws ({} caps)", sky.draws.len(), sky.draws.iter().filter(|d| d.cap).count());
    let layered = std::env::var("FH1_FM2_SKY_LAYERS").is_ok_and(|v| v == "1");
    let only: Option<usize> = std::env::var("FH1_FM2_SKY_ONLY").ok().and_then(|v| v.parse().ok());
    for (order, d) in sky.draws.iter().enumerate() {
        if only.is_some_and(|o| o != order) {
            continue;
        }
        if d.sun {
            let positions: Vec<[f32; 3]> = d.positions.iter().map(|p| [p[0], p[1], -p[2]]).collect();
            let mut idx = Vec::with_capacity(d.indices.len());
            for t in d.indices.chunks_exact(3) {
                idx.extend_from_slice(&[t[0], t[2], t[1]]);
            }
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            let normals: Vec<[f32; 3]> = positions.iter().map(|p| (-Vec3::from(*p)).normalize_or(Vec3::Y).to_array()).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            mesh.insert_indices(Indices::U32(idx));
            // INFERRED colour (the game's c0 of the flare shader is not read): the Xenia disc is a warm yellow.
            let material = StandardMaterial { base_color: Color::srgb(1.0, 0.85, 0.35), unlit: true, cull_mode: None, fog_enabled: false, alpha_mode: AlphaMode::Add, ..default() };
            commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.add(material)), Transform::default(), NoFrustumCulling, bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver, SkyDome));
            continue;
        }
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
        let flip = std::env::var("FH1_FM2_SKY_FLIPV").is_ok();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, d.uv.iter().map(|t| if flip { [t[0], 1.0 - t[1]] } else { *t }).collect::<Vec<[f32; 2]>>());
        mesh.insert_indices(Indices::U32(idx));
        let sky_gain: f32 = std::env::var("FH1_FM2_SKY_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let material = StandardMaterial {
            base_color: Color::linear_rgb(sky_gain, sky_gain, sky_gain),
            base_color_texture: Some(images.add(img)),
            unlit: true,
            cull_mode: None,
            fog_enabled: false,
            // The panoramas of one dome are passes over the same sphere (Tsukuba's five share one bounding box): sky and clouds with their own
            // alpha, drawn in file order, each over the last. `FH1_FM2_SKY_LAYERS=0` = the old opaque panoramas.
            alpha_mode: if d.cap || layered { AlphaMode::Blend } else { AlphaMode::Opaque },
            depth_bias: order as f32,
            ..default()
        };
        // `FH1_FM2_SKY_PASSES=n` draws each layer n times (alpha ~ n x): the textures' alpha tops out at 0.44.
        let passes = std::env::var("FH1_FM2_SKY_PASSES").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 6);
        let mesh_h = meshes.add(mesh);
        for k in 0..passes {
            let mut m = material.clone();
            m.depth_bias = (order * 8 + k) as f32;
            commands.spawn((Mesh3d(mesh_h.clone()), MeshMaterial3d(materials.add(m)), Transform::default(), NoFrustumCulling, bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver, SkyDome));
        }
    }
    // The sun glow card: the flare picture added over the sky around the sun disc. INFERRED level: the draw's own pixel-shader constants are
    // neutral on most tracks (white / 2.0) and its shader is not read, so `FH1_FM2_GLOW_GAIN` (default 1) scales a white flare.
    if only.is_none() {
        if let Some(g) = &sky.glow {
            let decoded = Image::from_buffer(&g.png, bevy::image::ImageType::Extension("png"), bevy::image::CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::RENDER_WORLD);
            if let Ok(img) = decoded {
                let positions: Vec<[f32; 3]> = g.positions.iter().map(|p| [p[0], p[1], -p[2]]).collect();
                let mut idx = Vec::with_capacity(g.indices.len());
                for t in g.indices.chunks_exact(3) {
                    idx.extend_from_slice(&[t[0], t[2], t[1]]);
                }
                let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
                let normals: Vec<[f32; 3]> = positions.iter().map(|p| (-Vec3::from(*p)).normalize_or(Vec3::Y).to_array()).collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, g.uv.clone());
                mesh.insert_indices(Indices::U32(idx));
                let gain: f32 = std::env::var("FH1_FM2_GLOW_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
                let material = StandardMaterial {
                    base_color: Color::linear_rgb(gain, gain, gain),
                    base_color_texture: Some(images.add(img)),
                    unlit: true,
                    cull_mode: None,
                    fog_enabled: false,
                    alpha_mode: AlphaMode::Add,
                    depth_bias: 100.0,
                    ..default()
                };
                commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.add(material)), Transform::default(), NoFrustumCulling, bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver, SkyDome));
                info!("fm2 sky: sun glow card drawn (gain {gain})");
            } else {
                warn!("fm2 sky: the glow picture failed to load");
            }
        }
    }
}

/// The sun disc's / glow card's centre, in engine axes relative to the dome centre (log only).
impl Fm2Sky {
    pub fn sun_offset(&self) -> Option<Vec3> {
        let d = self.draws.iter().find(|d| d.sun)?;
        let n = d.positions.len() as f32;
        let c = d.positions.iter().fold(Vec3::ZERO, |a, p| a + Vec3::new(p[0], p[1], -p[2]));
        Some(c / n + Vec3::new(0.0, self.offset_y, 0.0))
    }

    pub fn glow_offset(&self) -> Option<Vec3> {
        let g = self.glow.as_ref()?;
        let c = g.positions.iter().fold(Vec3::ZERO, |a, p| a + Vec3::new(p[0], p[1], -p[2]));
        Some(c / g.positions.len() as f32 + Vec3::new(0.0, self.offset_y, 0.0))
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
        29 => {
            // Uncompressed RGBA8 (sRGB), written by diagnostics only.
            let len = (w * h * 4) as usize;
            return Some(Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, b.get(148..148 + len)?.to_vec(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD));
        }
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
