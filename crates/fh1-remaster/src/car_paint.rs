//! Remaster car paint (W2): `StandardMaterial` + a small bindless extension.
//!
//! - Base colour = lerp(paint, atlas.rgb, atlas.a): the exterior atlas (nodamage) is white with alpha 0 where the body is
//!   painted and carries badges / lettering / trim with alpha 1 (seen on the Corrado atlas; the game's body technique
//!   reads PaintColor, docs/SHADERS.md "Car materials").
//! - Clear coat from StandardMaterial (`clearcoat`, `clearcoat_perceptual_roughness`); the coat normal stays the smooth
//!   geometric one.
//! - Metallic flake: per-texel-cell random normal jitter on the base layer only, faded out where a cell gets smaller
//!   than a pixel (no sparkle aliasing at distance), and off on the decals (atlas alpha).
//! - Fill (2026-10-07): the game lights cars with a sky/ground hemisphere (TOD HLTopColour / HLBottomColour as an SH,
//!   docs/SHADERS.md "Car globals") on top of the sun; the remaster's env map alone left shade-side panels at ~0.2x the
//!   faithful luma. The extension adds diffuse x lerp(bottom, top, n.y) in the game's display units x game_unit_scale
//!   (like the game-shader sky), so it lands where the faithful car's hemisphere term does. car.rs `update_car_fill`.
//! - Car sun: the game lights cars with sunColor x IBLDirectScaleCar where scenery gets x IBLDirectScaleEnvironment
//!   (VERIFIED, docs/SHADERS.md; Colorado day 0.63 vs 1.3). One Bevy sun lights both, so the extension takes
//!   (1 - fill_top.w) of the directional lights' Lambert diffuse back off the paint (fill_top.w = the ratio; 1 = no change).
//! - Car sun matched (P16-B 2026-10-09, user: single black pixels on the paint, densest on the roof, probe on or off;
//!   FH1_RM_PAINT_SUN_MATCH=0 = old): the removal above re-derived the sun with the shadow MAP only, then clamped at 0.
//!   Bevy's direct term also multiplies by the screen-space contact shadow (light.rs `ContactShadows`), a jittered
//!   per-pixel ray march (blue noise / interleaved gradient noise, 8 steps). Every pixel the march called shadowed but the
//!   map called lit lost its sun in Bevy and then had the full unshadowed diffuse subtracted: pure black, scattered by the
//!   noise, densest where the sun term is largest (the roof). Now the removal takes the same contact shadow (same inputs
//!   = same noise), and the result never goes below `keep` x the lit colour ([`sun_match_on`]). Guard (always on): the lit
//!   colour is made finite and >= 0 before fog / post (`finite_rgb`). Residual (not fixable here): Bevy's contact shadow
//!   itself self-hits on the car under MSAA (the main pass marches from the pixel-centre depth, the multisampled depth
//!   prepass is read at sample 0, a fraction of a pixel away: raymarch.wgsl `depth_texel_clamped`; a tiny penetration
//!   returns FULL shadow, pbr_functions.wgsl `calculate_contact_shadow`), so some pixels lose their sun (darker, no longer
//!   black dots; also on lamps / StandardMaterial parts). FH1_RM_CONTACT_SHADOWS=0 or an AA mode without MSAA removes it.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub type CarPaintMaterial = ExtendedMaterial<StandardMaterial, CarPaint>;

/// FH1_RM_PAINT_SUN_MATCH (module doc "Car sun matched"), default on.
pub(crate) fn sun_match_on() -> bool {
    std::env::var("FH1_RM_PAINT_SUN_MATCH").map_or(true, |v| v != "0")
}

/// The paint WGSL with the build-time switches applied.
pub(crate) fn wgsl() -> String {
    let sh = std::env::var("FH1_RM_PAINT_SPEC_SH").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.3183);
    WGSL.replace("const SUN_MATCH: bool = true;", &format!("const SUN_MATCH: bool = {};", sun_match_on()))
        .replace("const RM_PAINT_SPEC_SH: f32 = 0.3183;", &format!("const RM_PAINT_SPEC_SH: f32 = {sh:.5};"))
}

/// Paint parameters on top of the StandardMaterial base (whose base colour must be white, its texture the atlas).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[data(50, CarPaintUniform, binding_array(101))]
#[bindless(index_table(range(50..53), binding(100)))]
pub struct CarPaint {
    /// Paint colour, linear (Combo_Colors RGB sRGB-degammed).
    pub colour: LinearRgba,
    /// Flake normal jitter (0 = solid paint).
    pub flake: f32,
    /// Flake cells per UV unit.
    pub flake_scale: f32,
    /// Hemisphere fill above / below (post-exposure units, see the module doc; 0 = none). `fill_top.alpha` = the share of
    /// the directional lights' diffuse kept on the paint (car sun, module doc).
    pub fill_top: LinearRgba,
    pub fill_bottom: LinearRgba,
    /// Gain of the per-vertex lit colour in COLOR_0 (car_prt.rs; 0 = off: the engine's own lighting).
    pub prt: f32,
    /// With `prt`: the multiplier on pixels the sun does not reach (car_prt.rs `shade`; 0 = none).
    pub shade: f32,
    /// The game's own hemisphere colours **before** the inferred fill scale (`TrackSettings` HLTopColour /
    /// HLBottomColour). The body shader's specular lobes multiply the SH term directly (MODLOG 331), so a
    /// game-sourced lobe must use these rather than the scaled `fill_top` / `fill_bottom`.
    pub fill_raw_top: LinearRgba,
    pub fill_raw_bottom: LinearRgba,
    /// The game's second specular lobe (`body`: `SpecularAmount2`, `SpecularPower2`); `x` = amount, `y` = power.
    /// Zero unless `FH1_RM_PAINT_SPEC2=1`, so it cannot change anything before it has been measured.
    pub spec2: Vec3,
    /// The game's first specular lobe (ody: SpecularAmount, SpecularPower) -- x = amount, y = power.
    /// Zero unless the game-specular mode is on.
    pub spec1: Vec4,
    /// The track's own car cube map (FM2 circuits), the game's coded colour (the shader squares the fetch); `None` = no reflection.
    #[texture(51, dimension = "cube")]
    #[sampler(52)]
    pub cube: Option<Handle<Image>>,
}

/// The track's car cube map as a cube texture (set once by the engine for FM2 circuits; `params.z` = reflection strength).
static CUBE: std::sync::OnceLock<Handle<Image>> = std::sync::OnceLock::new();

/// The cube's source bytes (`car_cube_full.bin`, see [`cube_image`]), handed over before the app runs; [`load_cube`] turns them into the image.
static CUBE_BIN: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

/// Sets the reflection cube's source bytes (the engine reads them from the imported FM2 circuit).
pub fn set_cube(bin: Vec<u8>) {
    let _ = CUBE_BIN.set(bin);
}

/// Startup system: builds the cube image from the bytes of [`set_cube`] (a 6-layer image viewed as a cube, 8-bit coded colour, with its mip chain).
pub(crate) fn load_cube(mut images: ResMut<Assets<Image>>) {
    if let Some(image) = CUBE_BIN.get().and_then(|b| cube_image(b)) {
        let _ = CUBE.set(images.add(image));
    }
}

/// The reflection cube, if one was set: new paint materials take it.
pub fn cube() -> Option<Handle<Image>> {
    CUBE.get().cloned()
}

/// Builds the cube image from `car_cube_full.bin` (importer: u32 LE width, u32 LE height, 6 faces of RGBA8, D3D face order +X -X +Y -Y +Z -Z):
/// the faces get a box-filtered mip chain so the reflection does not shimmer at a distance.
pub fn cube_image(bin: &[u8]) -> Option<Image> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
    let w = u32::from_le_bytes(bin.get(0..4)?.try_into().ok()?) as usize;
    let h = u32::from_le_bytes(bin.get(4..8)?.try_into().ok()?) as usize;
    if w == 0 || w != h || !w.is_power_of_two() || bin.len() != 8 + 6 * w * h * 4 {
        return None;
    }
    let levels = w.trailing_zeros() as usize + 1;
    let mut data = Vec::new();
    for face in 0..6 {
        let mut cur: Vec<u8> = bin[8 + face * w * h * 4..8 + (face + 1) * w * h * 4].to_vec();
        let mut size = w;
        for level in 0..levels {
            data.extend_from_slice(&cur);
            if level + 1 == levels {
                break;
            }
            let half = size / 2;
            let mut next = vec![0u8; half * half * 4];
            for y in 0..half {
                for x in 0..half {
                    for c in 0..4 {
                        let at = |dx: usize, dy: usize| cur[((2 * y + dy) * size + 2 * x + dx) * 4 + c] as u32;
                        next[(y * half + x) * 4 + c] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
                    }
                }
            }
            cur = next;
            size = half;
        }
    }
    let mut image = Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 6 }, TextureDimension::D2, data, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    image.texture_descriptor.mip_level_count = levels as u32;
    image.data_order = TextureDataOrder::LayerMajor;
    image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..Default::default() });
    Some(image)
}

#[derive(Clone, ShaderType)]
pub struct CarPaintUniform {
    pub colour: Vec4,
    /// x flake strength, y flake cells per UV unit, z cube reflection strength (0 = none).
    pub params: Vec4,
    pub fill_top: Vec4,
    pub fill_bottom: Vec4,
    /// x gain of the per-vertex lit colour (0 = off).
    pub prt: Vec4,
    /// The game's own hemisphere colours, before the inferred fill scale (MODLOG 331).
    pub fill_raw_top: Vec4,
    pub fill_raw_bottom: Vec4,
    /// x = the second specular lobe's amount, y = its power, z = SpecularColor (0 = none).
    pub spec: Vec4,
    /// x = the first specular lobe's amount, y = its power.
    pub spec1: Vec4,
}

impl Default for CarPaintUniform {
    fn default() -> Self {
        Self { colour: Vec4::ZERO, params: Vec4::ZERO, fill_top: Vec4::ZERO, fill_bottom: Vec4::ZERO, prt: Vec4::ZERO, fill_raw_top: Vec4::ZERO, fill_raw_bottom: Vec4::ZERO, spec: Vec4::ZERO, spec1: Vec4::ZERO }
    }
}

impl From<&CarPaint> for CarPaintUniform {
    fn from(p: &CarPaint) -> Self {
        let strength = if p.cube.is_some() { crate::light::env_f32("FH1_FM2_CAR_REFL", 1.0) } else { 0.0 };
        Self { colour: p.colour.to_vec4(), params: Vec4::new(p.flake, p.flake_scale, strength, crate::light::env_f32("FH1_FM2_CUBE_FLIP", 0.0)), fill_top: p.fill_top.to_vec4(), fill_bottom: p.fill_bottom.to_vec4(), prt: Vec4::new(p.prt, p.shade, crate::light::env_f32("FH1_FM2_CUBE_MUL", if p.prt > 0.0 { 12.0 } else { 0.0 }), 0.0), fill_raw_top: p.fill_raw_top.to_vec4(), fill_raw_bottom: p.fill_raw_bottom.to_vec4(), spec: Vec4::new(p.spec2.x, p.spec2.y, p.spec2.z, 0.0), spec1: p.spec1 }
    }
}

pub(crate) const SHADER: Handle<Shader> = bevy::asset::uuid_handle!("6f1c2a8e-93d4-4b7a-a51e-2c0d9e7b3f12");

pub(crate) const WGSL: &str = r#"
#import bevy_render::maths::PI
#ifdef BINDLESS
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_cube}
#endif
#import bevy_pbr::{
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    shadows,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_functions as pbr_fns,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_bindings::mesh,
}

struct CarPaint { colour: vec4<f32>, params: vec4<f32>, fill_top: vec4<f32>, fill_bottom: vec4<f32>, prt: vec4<f32>, fill_raw_top: vec4<f32>, fill_raw_bottom: vec4<f32>, spec: vec4<f32>, spec1: vec4<f32> }

// Car sun removal matched to Bevy's shadowing (module doc; replaced at registration from FH1_RM_PAINT_SUN_MATCH).
const SUN_MATCH: bool = true;

// The 1/pi the vertex transfer numbers carry (car_prt.rs SCALE: "transfer / pi"); `FH1_RM_PAINT_SPEC_SH` overrides.
const RM_PAINT_SPEC_SH: f32 = 0.3183;


#ifdef BINDLESS
struct CarPaintIndices { material: u32, cube: u32, cube_sampler: u32 }
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage> car_paint_indices: array<CarPaintIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage> car_paint: array<CarPaint>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(50) var<uniform> car_paint: CarPaint;
@group(#{MATERIAL_BIND_GROUP}) @binding(51) var car_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(52) var car_cube_sampler: sampler;
#endif

// Non-finite (NaN / inf) or negative channels -> 0, by bit pattern (safe under fast-math). One bad sample would turn a
// whole pixel black in the MSAA resolve and poison bloom / SMAA neighbourhoods (module doc "Car sun matched").
fn finite_rgb(c: vec3<f32>) -> vec3<f32> {
    let bad = (bitcast<vec3<u32>>(c) & vec3<u32>(0x7f800000u)) == vec3<u32>(0x7f800000u);
    return max(select(c, vec3<f32>(0.0), bad), vec3<f32>(0.0));
}

fn hash3(p: vec2<f32>) -> vec3<f32> {
    var q = vec3<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)), dot(p, vec2<f32>(419.2, 371.9)));
    return fract(sin(q) * 43758.5453);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let paint = car_paint[car_paint_indices[slot].material];
    let car_cube = bindless_textures_cube[car_paint_indices[slot].cube];
    let car_cube_sampler = bindless_samplers_filtering[car_paint_indices[slot].cube_sampler];
#else
    let paint = car_paint;
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let atlas = pbr_input.material.base_color;
    let decal = atlas.a;
    // The game's own light (car_prt.rs): the mesh carries a lit colour per vertex in COLOR_0, which Bevy has already multiplied into the atlas.
#ifdef VERTEX_COLORS
    let prt_on = paint.prt.x > 0.0;
    let prt_lit = in.color.rgb;
#else
    let prt_on = false;
    let prt_lit = vec3<f32>(1.0);
#endif
    pbr_input.material.base_color = vec4<f32>(mix(paint.colour.rgb * select(vec3<f32>(1.0), prt_lit, prt_on), atlas.rgb, decal), 1.0);
    // `FH1_RM_PAINT_GAME_DIFFUSE` (MODLOG 336): the game's body shader *is* the SH lighting -- its diffuse is the SH
    // term, not Bevy's ambient/sun. `paint.spec1.z` is the flag and `.w` the scale on the SH term. With the flag on,
    // Bevy's diffuse is removed here and the SH term is added below, so the two are not summed.
    var game_albedo = vec3<f32>(0.0);
    if paint.spec1.z > 0.5 {
        game_albedo = pbr_input.material.base_color.rgb;
        pbr_input.material.base_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // The diffuse light replaces Bevy's: the paint is lit by the vertex colour and Bevy only adds the specular (black albedo).
    let prt_diffuse = pbr_input.material.base_color.rgb * paint.prt.x;
    if prt_on {
        pbr_input.material.base_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        pbr_input.material.metallic = 0.0;
    }
#ifdef VERTEX_UVS_A
    if paint.params.x > 0.0 {
        let cells = in.uv * paint.params.y;
        let px = max(length(fwidth(cells)), 1e-4);
        let fade = (1.0 - smoothstep(0.35, 1.0, px)) * (1.0 - decal);
        let j = hash3(floor(cells)) * 2.0 - 1.0;
        pbr_input.N = normalize(pbr_input.N + j * (paint.params.x * fade));
    }
#endif
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Game hemisphere fill (module doc): diffuse albedo x lerp(ground, sky, n.y), already in post-exposure units.
    let diffuse = pbr_input.material.base_color.rgb * (1.0 - pbr_input.material.metallic);
    let hemi = mix(paint.fill_bottom.rgb, paint.fill_top.rgb, clamp(pbr_input.N.y * 0.5 + 0.5, 0.0, 1.0));
    out.color = vec4<f32>(out.color.rgb + diffuse * hemi * pbr_input.diffuse_occlusion + select(vec3<f32>(0.0), prt_diffuse, prt_on), out.color.a);
    // The track's own car cube (FM2): a Fresnel-weighted reflection of the surroundings (module doc; `params.z` = strength). The cube is stored
    // in the game's coded colour (the shaders square the fetch) and indexed in the game's left-handed frame (engine z is mirrored).
    // The fetch sits outside the branch: an implicit-derivative texture sample needs uniform control flow.
    let v = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);
    let rr = reflect(-v, pbr_input.N);
    // params.w (FH1_FM2_CUBE_FLIP): 0 = z mirrored (the game frame), 1 = not mirrored, 2 = x mirrored, 3 = both.
    var d = vec3<f32>(rr.x, rr.y, -rr.z);
    if paint.params.w > 2.5 { d = vec3<f32>(-rr.x, rr.y, rr.z); } else if paint.params.w > 1.5 { d = vec3<f32>(-rr.x, rr.y, -rr.z); } else if paint.params.w > 0.5 { d = rr; }
    // A real cube texture (level 0 is the game's 256 x 256 face, with a mip chain); the game squares the fetch.
    // The game reads the cube's alpha as a brightness boost: reflection = rgb^2 x (1 + alpha x CubeMultiplier) (body pixel shader, CubeMultiplier 12 in
    // cars/ShaderSettings Normal.xml). Only with the game's own car light (`FH1_FM2_CAR_PRT=1`, default 12); without it the old plain reflection
    // (`FH1_FM2_CUBE_MUL=n` forces a multiplier either way: alone it made the cars much too bright, MODLOG 80).
    let coded4 = textureSample(car_cube, car_cube_sampler, d);
#ifdef VERTEX_UVS_B
    // The body's environment visibility from its transfer numbers (the VS `shEnv` dot product), with the light-on car.
    // Only when the FM2 car rule is active (`prt_on`, or the game's shadow rule via `prt.y`, both FM2-only): other
    // games' cars have their own UV_1, which is not a visibility and must not be read as one (MODLOG 314).
    let env_vis = select(1.0, in.uv_b.y, prt_on || paint.prt.y > 0.0);
#else
    let env_vis = 1.0;
#endif
    let env = coded4.rgb * coded4.rgb * (1.0 + coded4.a * paint.prt.z) * env_vis;
    if paint.params.z > 0.0 {
        let ndv = clamp(dot(pbr_input.N, v), 0.0, 1.0);
        // FresnelMin 0.05 / Max 0.7 / Scale 1.75 / Power 5 (cars/ShaderSettings Normal.xml).
        let fres = clamp(0.05 + 1.75 * pow(1.0 - ndv, 5.0), 0.05, 0.7);
        out.color = vec4<f32>(out.color.rgb + env * fres * paint.params.z, out.color.a);
    }
    // Car sun (module doc): remove (1 - keep) of the directional lights' Lambert diffuse.
    let keep = paint.fill_top.w;
    if keep < 0.999 {
        let view_z = dot(vec4<f32>(
            view_bindings::view.view_from_world[0].z,
            view_bindings::view.view_from_world[1].z,
            view_bindings::view.view_from_world[2].z,
            view_bindings::view.view_from_world[3].z
        ), in.world_position);
        var direct = vec3<f32>(0.0);
        for (var i = 0u; i < view_bindings::lights.n_directional_lights; i = i + 1u) {
            let light = &view_bindings::lights.directional_lights[i];
            let n_dot_l = max(dot(pbr_input.N, (*light).direction_to_light), 0.0);
            if n_dot_l <= 0.0 {
                continue;
            }
            var shadow = 1.0;
            if ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u
                && (pbr_input.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u {
                shadow = shadows::fetch_directional_shadow(i, in.world_position, pbr_input.world_normal, view_z, in.position.xy);
            }
#ifdef CONTACT_SHADOWS
#ifdef DEPTH_PREPASS
            // SUN_MATCH (module doc): the same contact shadow Bevy's direct term got (same inputs, same per-pixel noise).
            if SUN_MATCH && view_bindings::contact_shadows_settings.linear_steps > 0u
                && (pbr_input.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u && shadow > 0.0
                && ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_CONTACT_SHADOWS_ENABLED_BIT) != 0u {
                shadow *= pbr_fns::calculate_contact_shadow(
                    pbr_input.world_position.xyz, pbr_input.frag_coord.xy, (*light).direction_to_light,
                    view_bindings::contact_shadows_settings.linear_steps);
            }
#endif
#endif
            direct = direct + (*light).color.rgb * n_dot_l * shadow;
        }
        let removed = diffuse * direct * (view_bindings::view.exposure * (1.0 - keep) / PI);
        // SUN_MATCH: never below the kept share of the lit colour (an estimate above Bevy's own diffuse, e.g. Burley /
        // clear-coat Fresnel at grazing angles, clipped pixels to black).
        let kept = select(vec3<f32>(0.0), out.color.rgb * keep, SUN_MATCH);
        out.color = vec4<f32>(max(out.color.rgb - removed, max(kept, vec3<f32>(0.0))), out.color.a);
    }
    // The game's second specular lobe (`body`: SpecularAmount2 / SpecularPower2), as the body pixel shader computes it
    // (MODLOG 331): `SpecularColor . SpecularAmount2 . pow(sat(N.H), SpecularPower2) . SH`, where SH is the game's own
    // hemisphere (`fill_raw_*`, before the inferred fill scale) evaluated the same way the SH term is -- see the
    // instructions `mul r0.yz, r3.zzzz, r6.xyy` / `mulsc r0.x, c181.x, r0.z` / `mul r6.xyz, r5.xxxx, c180.yzxx`.
    if paint.spec.x > 0.0 || paint.spec1.x > 0.0 {
        let n = normalize(pbr_input.world_normal);
        let hemi_raw = mix(paint.fill_raw_bottom.rgb, paint.fill_raw_top.rgb, clamp(n.y * 0.5 + 0.5, 0.0, 1.0));
        // `HLTopColour` / `HLBottomColour` are **radiance**, while the body shader's `r3.z` is the SH term the vertex
        // transfer numbers produce -- and `prt.bin`'s numbers are "transfer / pi" (car_prt.rs `SCALE`), so the same
        // quantity carries a 1/pi. `FH1_RM_PAINT_SPEC_SH` scales it for the test (default 1/pi), MODLOG 335.
        let sh = dot(hemi_raw, vec3<f32>(0.2126, 0.7152, 0.0722)) * RM_PAINT_SPEC_SH;
        var sheen = vec3<f32>(0.0);
        for (var i = 0u; i < view_bindings::lights.n_directional_lights; i = i + 1u) {
            let light = &view_bindings::lights.directional_lights[i];
            let h = normalize((*light).direction_to_light + v);
            let nh = clamp(dot(n, h), 0.0, 1.0);
            // c190/c191: SpecularAmount / SpecularPower (Metallic.xml sets the amount to 0, so a metallic paint has none).
            sheen += pow(nh, max(paint.spec1.y, 1.0)) * paint.spec1.x;
            // c181/c182: SpecularAmount2 / SpecularPower2 -- the broad sheen a metallic paint carries.
            sheen += pow(nh, max(paint.spec.y, 1.0)) * paint.spec.x;
        }
        out.color = vec4<f32>(out.color.rgb + sheen * sh * vec3<f32>(paint.spec.z), out.color.a);
    }
    if paint.spec1.z > 0.5 {
        // The game's diffuse: albedo x SH (the same term the lobes multiply), scaled by `spec1.w` for the experiment.
        let n2 = normalize(pbr_input.world_normal);
        let hemi2 = mix(paint.fill_raw_bottom.rgb, paint.fill_raw_top.rgb, clamp(n2.y * 0.5 + 0.5, 0.0, 1.0));
        out.color = vec4<f32>(out.color.rgb + game_albedo * hemi2 * paint.spec1.w, out.color.a);
    }
    // The game's shadow term on the whole lit car (car_prt.rs `shade`): lerp(shade, 1, sun visibility). The visibility
    // is the body's own self-shadowing from its transfer numbers (UV_1.x), 0 on sides facing away from the sun.
    // Independent of the additive PRT light (MODLOG 314).
#ifdef VERTEX_UVS_B
    if paint.prt.y > 0.0 {
        out.color = vec4<f32>(out.color.rgb * mix(paint.prt.y, 1.0, in.uv_b.x), out.color.a);
    }
#endif
    out.color = vec4<f32>(finite_rgb(out.color.rgb), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
"#;

impl MaterialExtension for CarPaint {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}
