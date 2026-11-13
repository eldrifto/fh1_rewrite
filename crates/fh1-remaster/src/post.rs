//! Remaster grade (W3): FH1's colour grading on top of Bevy's tonemapping. One full-screen pass after
//! `tonemapping`: display-referred colour -> sRGB -> the game's 16³ grading LUT -> linear, plus a soft vignette.
//!
//! The LUT is fh1-render's [`FxPost::lut_image`]: postfx.rs keeps blending it on the CPU exactly as the game does
//! (zone ColorGradingMap/Night by DayColorGradeAmount/NightColorGradeAmount, track defaults outside zones), so the
//! remaster gets every zone's and the night's grade for free. The faithful chain's other stages (FH1 bloom,
//! adaptation, Hable filmic) are replaced by Bevy bloom, light.rs's exposure and AgX.
//!
//! Default tone map = the game's (FH1_RM_TONEMAP unset/game): Bevy's tonemapper is off and this pass runs FH1's Hable
//! curve with the track's `<filmicTone>` (day/night blended), the LUT on its output and the game's sqrt encoding, so the
//! contrast and black level match the faithful renderer; FH1_RM_TONEMAP=tony|agx|aces = Bevy's curve, LUT after it.
//! Env: FH1_RM_LUT=strength (default 1; 0 = pass off, Bevy tonemapper only), FH1_RM_VIGNETTE=amount (default 0.22),
//! FH1_RM_FILMIC_EV=ev (the curve's exposure offset, default -2.2), FH1_RM_FRAME_EV=ev (whole-frame offset incl. game-shader
//! output, default +0.4), FH1_RM_DUSK_EV / FH1_RM_NIGHT_EV (extra after sunset -1.4 / at night -1.0).
//!
//! Camera motion blur (post/motion_blur.rs, [`MotionBlurSettings`]): on the HDR frame before bloom, independent of this
//! grade (registered even with FH1_RM_LUT=0).

mod motion_blur;

pub use motion_blur::{forced_level as motion_blur_forced_level, MotionBlurSettings};

use bevy::core_pipeline::schedule::Core3d;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::Core3dSystems;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_3d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntry, BindGroupLayoutDescriptor, BindingResource, Buffer, BufferDescriptor, BufferUsages, CachedRenderPipelineId, ColorTargetState,
    ColorWrites, FragmentState, LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, StoreOp, TextureFormat, TextureSampleType, VertexState,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::light::{env_f32, RemasterView};

const GRADE_WGSL: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_s: sampler;
@group(0) @binding(2) var lut: texture_3d<f32>;
@group(0) @binding(3) var lut_s: sampler;
struct Params { a: vec4<f32>, v: vec4<f32>, f1: vec4<f32>, f2: vec4<f32>, m: vec4<f32>, lum: vec4<f32>, tint: vec4<f32>, lite: vec4<f32>, dark: vec4<f32> };
@group(0) @binding(4) var<uniform> p: Params;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vertex(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

// FH1_RM_POST_CODED=1 (MODLOG 480): the FM2 combine's input is read as ALREADY gamma-coded (no sRGB encode on the way
// in). The game's scene buffer is gamma-coded, and FM2 scenery outputs its display-coded product, so without this the
// combine encodes it a second time -- which lifts shadows and crushes highlights.
const RM_POST_CODED: f32 = RM_POST_CODED_V;
// T3 (MODLOG 493): apply the combine's tint in linear light rather than on the codes.
const RM_TINT_LINEAR: f32 = RM_TINT_LINEAR_V;

fn to_code(c: vec3<f32>) -> vec3<f32> {
    if (RM_POST_CODED > 0.5) {
        return clamp(c, vec3<f32>(0.0), vec3<f32>(8.0));
    }
    return to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(8.0)));
}

fn from_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// FH1's Hable curve: f1 = (W, A, B, C), f2 = (D, E, F, exposure EV) (docs/SHADERS.md "FinalCombine").
fn hable(x: vec3<f32>) -> vec3<f32> {
    let A = p.f1.y; let B = p.f1.z; let C = p.f1.w; let D = p.f2.x; let E = p.f2.y; let F = p.f2.z;
    return (x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F) - E / F;
}

@fragment
fn fragment(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(src, src_s, in.uv, 0.0);
    if (p.m.x > 0.5) {
        // FM2's own FinalCombineBloomTonemap (docs: STATUS.md, read from the game's post shader): desaturate toward the
        // luminance, tint, then a luminance tone curve blending the dark and light parameter sets (xyzw = black point,
        // white point, gamma, boost).
        // The game's scene buffer holds gamma-coded values (a 360 render target written with the console's gamma), and the whole combine
        // (desaturate, tint, curve, the average that picks the curve) works on those codes: convert in, and back out for the display.
        var x = to_code(c.rgb);
        let l = dot(x, p.lum.xyz);
        // `p.m.w` (FH1_FM2_SAT, INFERRED): extra saturation about the luminance; the Xenia shots are about 1.4x as colourful as the plain combine.
        x = vec3<f32>(l) + (x - vec3<f32>(l)) * p.m.y;
        // The track's `DesatPostMod` tint. `FH1_FM2_TINT_LINEAR=1` (T3, MODLOG 493) applies it in linear light -- i.e. as
        // tint^2.2 on the codes -- instead of a plain multiply on the codes, which is the other way the game could use it.
        if RM_TINT_LINEAR > 0.5 {
            x = to_code(from_srgb(clamp(x, vec3<f32>(0.0), vec3<f32>(1.0))) * p.tint.xyz);
        } else {
            x = x * p.tint.xyz;
        }
        // The extra saturation turns about the true luminance, so it leaves the brightness the curve sees alone.
        let y0 = dot(x, vec3<f32>(0.2125, 0.7154, 0.0721));
        x = max(vec3<f32>(y0) + (x - vec3<f32>(y0)) * p.m.w, vec3<f32>(0.0));
        let y = dot(x, vec3<f32>(0.2125, 0.7154, 0.0721));
        // The game's blend (vertex shader of the technique): t = saturate((average scene luminance - min) / (max - min)) with the track's
        // TrackLumLimits; its average comes from a small adaptation texture, here a grid of samples of the frame. m.z >= 0 overrides t.
        var t = p.m.z;
        var avg = 0.0;
        if (t < 0.0) {
            var sum = 0.0;
            for (var gy = 0; gy < 6; gy = gy + 1) {
                for (var gx = 0; gx < 8; gx = gx + 1) {
                    let sc = textureSampleLevel(src, src_s, vec2<f32>((f32(gx) + 0.5) / 8.0, (f32(gy) + 0.5) / 6.0), 0.0).rgb;
                    sum = sum + dot(to_code(sc), vec3<f32>(0.2125, 0.7154, 0.0721));
                }
            }
            avg = sum / 48.0;
            t = clamp((avg - p.lum.w) * p.tint.w, 0.0, 1.0);
        }
        // FH1_FM2_CONTRAST=2: diagnostic -- R = the frame average the blend uses (coded luminance), G = the resulting t.
        // Read back from a normal screenshot (the flat colour encodes the value as its sRGB code).
        if (p.a.z > 1.5) {
            return vec4<f32>(from_srgb(clamp(vec3<f32>(avg * 2.0, t, 0.0), vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
        }
        let q = mix(p.dark, p.lite, t);
        let y2 = pow(max(y - q.x, 0.0) / max(q.y - q.x, 1e-4), 1.0 / max(q.z, 0.1)) + q.w;
        var o = clamp(x * (y2 / max(y, 1e-6)), vec3<f32>(0.0), vec3<f32>(1.0));
        // `p.lum`-independent global contrast (INFERRED, FH1_FM2_CONTRAST=1): a piecewise-linear map of the coded value that matches the luminance
        // percentiles of the twelve Xenia shots (5 / 25 / 50 / 75 / 95 % : 33 / 75 / 103 / 141 / 179 in ours -> 13 / 51 / 90 / 150 / 210, mean of the twelve; the new Xenia shots of 2026-10-27).
        if (p.a.z > 0.5) {
            let xs = array<f32, 7>(0.0, 33.0, 75.0, 103.0, 141.0, 179.0, 255.0);
            let ys = array<f32, 7>(0.0, 13.0, 51.0, 90.0, 150.0, 210.0, 255.0);
            let v = o * 255.0;
            var r = v;
            for (var k = 0; k < 6; k = k + 1) {
                let t = clamp((v - vec3<f32>(xs[k])) / (xs[k + 1] - xs[k]), vec3<f32>(0.0), vec3<f32>(1.0));
                let seg = vec3<f32>(ys[k]) + (ys[k + 1] - ys[k]) * t;
                r = select(r, seg, v >= vec3<f32>(xs[k]));
            }
            o = r / 255.0;
        }
        // `FH1_FM2_RAMP` (OPT-IN, MODLOG 135/136): the display gamma ramp FM2's own Direct3D code builds for display type 2 (what Xenia reports): the BT.709 OETF,
        // 4.5 L below 0.018, else 1.099 L^0.45 - 0.099 (constants read from default.xex). 1 = applied to the light the sRGB code o stands for (the Xenia shots then show
        // this curve instead of sRGB), 2 = applied to o itself as if o were the linear framebuffer.
        if (p.a.w > 0.5) {
            let l = select(o, from_srgb(o), p.a.w < 1.5);
            let d = select(1.099 * pow(l, vec3<f32>(0.45)) - 0.099, l * 4.5, l < vec3<f32>(0.018));
            return vec4<f32>(from_srgb(clamp(d, vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
        }
        return vec4<f32>(from_srgb(o), c.a);
    }
    var s: vec3<f32>;
    var g: vec3<f32>;
    if (p.a.y > 0.5) {
        // Game tone map (camera Tonemapping::None): exposure, Hable / Hable(W), the LUT on the curve's output, then
        // the game's sqrt output encoding read as the display's sRGB code value.
        let x = max(c.rgb, vec3<f32>(0.0)) * exp2(p.f2.w);
        let h = clamp(hable(x) / hable(vec3<f32>(p.f1.x)), vec3<f32>(0.0), vec3<f32>(1.0));
        let l = textureSampleLevel(lut, lut_s, h * (15.0 / 16.0) + 0.5 / 16.0, 0.0).rgb;
        s = sqrt(h);
        g = sqrt(clamp(l, vec3<f32>(0.0), vec3<f32>(1.0)));
    } else {
        // After Bevy's tonemapper: grade the display-referred colour in sRGB.
        s = to_srgb(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
        // 16^3 LUT: texel centres at (i + 0.5) / 16.
        g = textureSampleLevel(lut, lut_s, s * (15.0 / 16.0) + 0.5 / 16.0, 0.0).rgb;
    }
    var o = mix(s, g, p.a.x);
    // Vignette: smooth darkening towards the corners (aspect-corrected radius, 1 at the corner).
    let d = (in.uv - 0.5) * vec2<f32>(p.v.z, 1.0);
    let r = length(d) / length(vec2<f32>(0.5 * p.v.z, 0.5));
    o = o * (1.0 - p.v.x * pow(clamp(r, 0.0, 1.0), p.v.y));
    return vec4<f32>(from_srgb(o), c.a);
}
"#;

/// Main-world grade inputs, extracted every frame.
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct GradeSettings {
    pub lut: Option<Handle<Image>>,
    pub strength: f32,
    pub vignette: f32,
    /// The game's filmic curve replaces Bevy's tonemapper (light.rs sets the camera to Tonemapping::None).
    pub game_tonemap: bool,
    /// (W, A, B, C, D, E, F, exposure EV), day/night blended.
    pub filmic: [f32; 8],
    /// FM2's own final combine (`FH1_FM2_POST=1`, set up by the engine for FM2 tracks); replaces the whole grade.
    pub fm2: Option<Fm2Post>,
}

/// FM2's `FinalCombineBloomTonemap` inputs from a track's `TrackSettings.xml` (`DesaturateAmount`, `DesatLuminosity`, `DesatPostMod`,
/// `ToneMappingLevels`, `TrackLumLimits`). VERIFIED in the game's code (`0x82284bf0`): black points, white points, boosts and the luminance limits
/// are scaled by 1/255 (the constant `0x82002e4c`), gammas are not. The dark / light curve blend `t` is
/// `saturate((average frame luminance - min) * 1 / (max - min))` with the limits (the vertex shader of the technique, whose sign convention for
/// the subtract is read from the intent, not verified); the engine estimates the average from a grid of samples of the frame. `FH1_FM2_POST_T`
/// (0..1) forces `t`.
#[derive(Resource, Clone, Debug, Default)]
pub struct Fm2Post {
    pub desat: f32,
    pub lum: [f32; 3],
    pub tint: [f32; 3],
    pub light: [f32; 4],
    pub dark: [f32; 4],
    /// Forced blend (`FH1_FM2_POST_T`); negative = computed from the frame.
    pub t: f32,
    /// `TrackLumLimits` min / 255 and 1 / ((max - min) / 255).
    pub lum_min: f32,
    pub lum_inv_range: f32,
}

impl Fm2Post {
    pub fn from_xml(xml: &str) -> Option<Self> {
        let attr = |tag: &str, a: &str| -> Option<f32> {
            let i = xml.find(&format!("<{tag} "))?;
            let end = xml[i..].find('>')? + i;
            let t = &xml[i..end];
            let j = t.find(&format!("{a}=\""))? + a.len() + 2;
            t[j..].split('"').next()?.parse().ok()
        };
        let curve = |k: &str| [attr("ToneMappingLevels", &format!("{k}CurveBlackPoint")), attr("ToneMappingLevels", &format!("{k}CurveWhitePoint")), attr("ToneMappingLevels", &format!("{k}CurveGamma")), attr("ToneMappingLevels", &format!("{k}CurveBoost"))];
        let pack = |c: [Option<f32>; 4]| Some([c[0]? / 255.0, c[1]? / 255.0, c[2]?, c[3]? / 255.0]);
        Some(Self {
            desat: attr("DesaturateAmount", "value")?,
            lum: [attr("DesatLuminosity", "r")?, attr("DesatLuminosity", "g")?, attr("DesatLuminosity", "b")?],
            tint: [attr("DesatPostMod", "r")?, attr("DesatPostMod", "g")?, attr("DesatPostMod", "b")?],
            light: pack(curve("Light"))?,
            dark: pack(curve("Dark"))?,
            t: env_f32("FH1_FM2_POST_T", -1.0),
            lum_min: attr("TrackLumLimits", "min")? / 255.0,
            lum_inv_range: 255.0 / (attr("TrackLumLimits", "max")? - attr("TrackLumLimits", "min")?).max(1.0),
        })
    }
}

#[derive(Resource)]
struct GradePipeline {
    shader: Handle<Shader>,
    layout: BindGroupLayoutDescriptor,
    ids: Vec<(TextureFormat, CachedRenderPipelineId)>,
}

#[derive(Default)]
struct GradeCache {
    sampler: Option<Sampler>,
    uniform: Option<Buffer>,
}

pub(crate) fn plugin(app: &mut App) {
    motion_blur::plugin(app);
    let strength = env_f32("FH1_RM_LUT", 1.0);
    let fm2 = app.world().get_resource::<Fm2Post>().cloned();
    if strength <= 0.0 && fm2.is_none() {
        return;
    }
    app.insert_resource(GradeSettings {
        lut: None,
        strength,
        vignette: env_f32("FH1_RM_VIGNETTE", 0.22),
        game_tonemap: game_tonemap(),
        filmic: [1.0, 0.7, 0.038, 0.634, 0.22, 0.003, 0.107, 0.31],
        fm2,
    })
        .add_plugins(ExtractResourcePlugin::<GradeSettings>::default())
        .add_systems(PostUpdate, follow_lut);
    // RM_POST_CODED_V is filled from the environment at start-up (see RM_POST_CODED in the shader).
    let coded = if std::env::var("FH1_RM_POST_CODED").is_ok_and(|v| v == "1") { "1.0" } else { "0.0" };
    let tint_lin = if std::env::var("FH1_FM2_TINT_LINEAR").is_ok_and(|v| v == "1") { "1.0" } else { "0.0" };
    let src = GRADE_WGSL.replace("RM_POST_CODED_V", coded).replace("RM_TINT_LINEAR_V", tint_lin);
    let shader = app.world_mut().resource_mut::<Assets<Shader>>().add(Shader::from_wgsl(src, "fh1_remaster/grade.wgsl"));
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
    let entries = [
        texture_2d(TextureSampleType::Float { filterable: true }).build(0, ShaderStages::FRAGMENT),
        sampler(SamplerBindingType::Filtering).build(1, ShaderStages::FRAGMENT),
        texture_3d(TextureSampleType::Float { filterable: true }).build(2, ShaderStages::FRAGMENT),
        sampler(SamplerBindingType::Filtering).build(3, ShaderStages::FRAGMENT),
        uniform_buffer_sized(false, None).build(4, ShaderStages::FRAGMENT),
    ];
    render_app
        .insert_resource(GradePipeline { shader, layout: BindGroupLayoutDescriptor::new("fh1_remaster_grade_layout", &entries), ids: Vec::new() })
        .add_systems(Render, prepare_grade.in_set(RenderSystems::Prepare))
        // Before FXAA / SMAA (Options > Graphics > Anti-aliasing, fh1-engine ui/graphics.rs): they expect the final colour.
        .add_systems(
            Core3d,
            grade_system
                .in_set(Core3dSystems::PostProcess)
                .after(tonemapping)
                .before(bevy::anti_alias::fxaa::fxaa)
                .before(bevy::anti_alias::smaa::smaa),
        );
}

/// Calibration of light.rs's physical exposure to the game curve's input scale: the festival at 16:00 matched the
/// faithful frame's ground, car and black-level luma (2.2-2.5 EV apart) at this offset (2026-10-06, c3).
const FILMIC_EV: f32 = -2.2;
/// Whole-frame exposure offset on the curve input (game-shader output included: the sky dome, clouds and glows don't go
/// through the camera exposure). 2026-10-07 same-pose A/B vs faithful at the festival 16:00 with the TOD sun: median luma
/// matched at -0.4 EV camera bias, but sky and lit banners stayed ~0.72x; +0.4 here lifts both. FH1_RM_FRAME_EV=0 = old.
const FRAME_EV: f32 = 0.4;
/// Extra whole-frame EV once the sun has sunk ([`crate::light::twilight`] = 1) and at night (SunObjectMoon = 1). Same A/B:
/// with only [`FRAME_EV`] the frame was 2.6x the faithful luma at 20:00 (sky 3-4x) and 2.0x at 00:00; 08:00-19:00 matched
/// (0.94-1.02x). FH1_RM_DUSK_EV / FH1_RM_NIGHT_EV (0 = old).
const DUSK_EV: f32 = -1.4;
const NIGHT_EV: f32 = -1.0;

/// The whole-frame EV offset at this time of day.
fn frame_ev(l: &crate::light::RemasterLighting) -> f32 {
    env_f32("FH1_RM_FRAME_EV", FRAME_EV) + env_f32("FH1_RM_DUSK_EV", DUSK_EV) * l.twilight + env_f32("FH1_RM_NIGHT_EV", NIGHT_EV) * l.night
}

/// Multiply exposure-independent emission (emissive_exposure_weight 0) given in the game's display-linear units by this,
/// so it lands where the faithful renderer puts it: the game curve's input carries [`FILMIC_EV`] (and its env override).
/// 1 with a Bevy tonemapper.
pub fn game_unit_scale() -> f32 {
    if game_tonemap() {
        (-env_f32("FH1_RM_FILMIC_EV", FILMIC_EV)).exp2()
    } else {
        1.0
    }
}

/// `FH1_RM_TONEMAP` unset or `game`: FH1's own filmic curve (TrackSettings `<filmicTone>`) in the grade pass.
pub fn game_tonemap() -> bool {
    std::env::var("FH1_RM_TONEMAP").map_or(true, |v| v.eq_ignore_ascii_case("game")) && env_f32("FH1_RM_LUT", 1.0) > 0.0
}

/// The LUT handle follows FxPost (rebuilt on an in-process map change); the filmic curve follows the track's base
/// post settings, blended to the night variant by DayNightPostProcess like the game (postfx.rs update_post).
fn follow_lut(
    post: Option<Res<fh1_render::postfx::FxPost>>,
    tod: Option<Res<fh1_render::lighting::FxTimeOfDay>>,
    lighting: Option<Res<crate::light::RemasterLighting>>,
    mut s: ResMut<GradeSettings>,
) {
    let want = post.as_ref().map(|p| p.lut_image.clone());
    if s.lut != want {
        s.lut = want;
    }
    if let Some(p) = post {
        let k = tod.map_or(0.0, |t| t.tod.scalar_or("DayNightPostProcess", t.minutes(), 0.0));
        let f = p.base.filmic.at_night_amount(k);
        let want = [f.white, f.shoulder_strength, f.linear_strength, f.linear_angle, f.toe_strength, f.toe_numerator, f.toe_denominator, f.exposure + p.base.exposure_bias + env_f32("FH1_RM_FILMIC_EV", FILMIC_EV) + lighting.as_deref().map_or(FRAME_EV, frame_ev)];
        if s.filmic != want {
            s.filmic = want;
        }
    }
}

fn prepare_grade(mut pipe: ResMut<GradePipeline>, cache: Res<PipelineCache>, views: Query<&ExtractedView, With<RemasterView>>) {
    for v in &views {
        let format = v.target_format;
        if pipe.ids.iter().any(|(f, _)| *f == format) {
            continue;
        }
        let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("fh1_remaster_grade".into()),
            layout: vec![pipe.layout.clone()],
            vertex: VertexState { shader: pipe.shader.clone(), shader_defs: vec![], entry_point: Some("vertex".into()), buffers: vec![] },
            fragment: Some(FragmentState {
                shader: pipe.shader.clone(),
                shader_defs: vec![],
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
            }),
            ..default()
        });
        pipe.ids.push((format, id));
    }
}

#[allow(clippy::too_many_arguments)]
fn grade_system(
    view: ViewQuery<(&ViewTarget, &ExtractedView), With<RemasterView>>,
    settings: Option<Res<GradeSettings>>,
    pipe: Res<GradePipeline>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    images: Res<RenderAssets<GpuImage>>,
    mut cache: Local<GradeCache>,
    mut ctx: RenderContext,
) {
    let (view_target, extracted) = view.into_inner();
    let Some(settings) = settings else { return };
    let Some(lut) = settings.lut.as_ref().and_then(|h| images.get(h)) else { return };
    let Some((_, id)) = pipe.ids.iter().find(|(f, _)| *f == extracted.target_format) else { return };
    let Some(pipeline) = pipeline_cache.get_render_pipeline(*id) else { return };
    let size = extracted.viewport.zw().as_vec2();
    let aspect = size.x / size.y.max(1.0);
    let f = settings.filmic;
    let params: [f32; 36] = [
        settings.strength.min(1.0),
        settings.game_tonemap as u32 as f32,
        env_f32("FH1_FM2_CONTRAST", 0.0),
        env_f32("FH1_FM2_RAMP", 0.0),
        settings.vignette,
        2.2,
        aspect,
        0.0,
        f[0], f[1], f[2], f[3], f[4], f[5], f[6], f[7],
        settings.fm2.is_some() as u32 as f32, settings.fm2.as_ref().map_or(1.0, |m| m.desat), settings.fm2.as_ref().map_or(1.0, |m| m.t), env_f32("FH1_FM2_SAT", 1.0),
        settings.fm2.as_ref().map_or(0.0, |m| m.lum[0]), settings.fm2.as_ref().map_or(0.0, |m| m.lum[1]), settings.fm2.as_ref().map_or(0.0, |m| m.lum[2]), settings.fm2.as_ref().map_or(0.0, |m| m.lum_min),
        settings.fm2.as_ref().map_or(1.0, |m| m.tint[0]), settings.fm2.as_ref().map_or(1.0, |m| m.tint[1]), settings.fm2.as_ref().map_or(1.0, |m| m.tint[2]), settings.fm2.as_ref().map_or(1.0, |m| m.lum_inv_range),
        settings.fm2.as_ref().map_or(0.0, |m| m.light[0]), settings.fm2.as_ref().map_or(1.0, |m| m.light[1]), settings.fm2.as_ref().map_or(1.0, |m| m.light[2]), settings.fm2.as_ref().map_or(0.0, |m| m.light[3]),
        settings.fm2.as_ref().map_or(0.0, |m| m.dark[0]), settings.fm2.as_ref().map_or(1.0, |m| m.dark[1]), settings.fm2.as_ref().map_or(1.0, |m| m.dark[2]), settings.fm2.as_ref().map_or(0.0, |m| m.dark[3]),
    ];
    let bytes: Vec<u8> = params.iter().flat_map(|f| f.to_le_bytes()).collect();
    let uniform = cache
        .uniform
        .get_or_insert_with(|| {
            render_device.create_buffer(&BufferDescriptor {
                label: Some("fh1_remaster_grade_params"),
                size: bytes.len() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .clone();
    render_queue.write_buffer(&uniform, 0, &bytes);
    let sampler = cache
        .sampler
        .get_or_insert_with(|| {
            render_device.create_sampler(&SamplerDescriptor {
                mag_filter: bevy::render::render_resource::FilterMode::Linear,
                min_filter: bevy::render::render_resource::FilterMode::Linear,
                ..default()
            })
        })
        .clone();
    let post = view_target.post_process_write();
    let bind_group = render_device.create_bind_group(
        "fh1_remaster_grade",
        &pipeline_cache.get_bind_group_layout(&pipe.layout),
        &[
            BindGroupEntry { binding: 0, resource: BindingResource::TextureView(post.source) },
            BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&sampler) },
            BindGroupEntry { binding: 2, resource: BindingResource::TextureView(&lut.texture_view) },
            BindGroupEntry { binding: 3, resource: BindingResource::Sampler(&lut.sampler) },
            BindGroupEntry { binding: 4, resource: uniform.as_entire_binding() },
        ],
    );
    let mut rp = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("fh1_remaster_grade"),
        // Every pixel is written (full-screen triangle, no blending): no clear (P15-C; Operations::default() cleared it).
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::DontCare(Default::default()), store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    rp.set_pipeline(pipeline);
    rp.set_bind_group(0, &bind_group, &[]);
    rp.draw(0..3, 0..1);
}
