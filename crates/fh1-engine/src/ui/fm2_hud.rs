//! FM2's own in-race HUD on FM2 circuits: the LAPS panel, the RACE / Lap timer rows and the third-person tach cluster's plate, speed,
//! gear, unit and assist icons, laid out from the game's scenes (`imported/fm2/ui/hud.json`, written by `fh1setup import-fm2 --only ui`
//! from `I_HUD_All_Divided_PartOne / PartTwo.xgf`) with the game's own textures and vector fonts.
//!
//! Positions are the scene's own units (1280 x 720, centred, y up), measured against the Xenia shots: the gear digit, the RACE box and
//! the LAPS panel land on the pixels the shots show. What the game's Lua does with the data (which row shows what, how the needle
//! moves) is read from the compiled scripts' string constants only; the rows and the number formats here are the engine's reading of
//! that, not a port. NOT built yet: the tach needle, hatch marks and rpm numbers (they are meshes in the scene's model data), the
//! boost dial's contents, the minimap, the place panel, the countdown lights.
//!
//! Switches: `FH1_FM2_HUD=0` keeps FH1's HUD on FM2 maps. Log lines start `fm2 hud:`.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use fh1_ui::vfont::{Font as VFont, FontMap};
use serde_json::Value;

use super::{fm2_dial, fm2_minimap, launch::raster_in, Menu, Settings};
use crate::camera::CameraRig;
use crate::race::{Events, RacePhase, RaceState};
use crate::track::Track;
use crate::{Car, Garage};

pub struct Fm2HudPlugin;

impl Plugin for Fm2HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load).add_systems(Update, (dial_update, minimap_update, drive).chain());
    }
}

/// Present (and true) when FM2's HUD replaces FH1's on this map.
#[derive(Resource)]
pub struct Fm2HudOn(pub bool);

/// Design space of the scenes.
const W: f32 = 1280.0;
const H: f32 = 720.0;
/// Text is drawn at this many times its on-screen size and shown scaled down.
const SS: f32 = 2.0;
/// Scene point size -> pixels (96 dpi).
const PT: f32 = 96.0 / 72.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bind {
    Static,
    CurLap,
    NumLaps,
    RaceTitle(u8),
    RaceTime(u8),
    Speed,
    Gear,
    Unit,
    /// The assist icon pair: shown while `on` (selected / unselected variant).
    Assist(AssistKind, bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssistKind {
    Abs,
    Tcs,
    Stm,
}

#[derive(Component)]
struct HudItem {
    bind: Bind,
    /// Text: font file stem (`c`, `b`, ...), em in design px, scene scale, horizontal align (0 left, 1 centre, 2 right), anchor (design, y up).
    font: Option<String>,
    em: f32,
    scale: Vec2,
    halign: u8,
    anchor: Vec2,
    shown: String,
}

#[derive(Resource)]
struct Fm2Hud {
    font_map: FontMap,
    fonts: HashMap<String, VFont>,
    labels: HashMap<String, String>,
    cache: HashMap<(String, String, u32), (Handle<Image>, UVec2, Vec2)>,
}

fn nums(v: &Value, k: &str) -> Vec<f32> {
    v[k].as_str().map(|s| s.split_whitespace().filter_map(|t| t.parse().ok()).collect()).unwrap_or_default()
}

/// Dark HUD panels (black or near-black pixels with alpha, the LAPS / RACE bars and the tach plate) come out about 0.38 x the picture
/// behind them in the Xenia shots, on every track measured (ratios .37 .38 .38 .38 on four tracks), where the textures' own alpha (138
/// of 255) would give 0.46 on the console's gamma-space blend. The root cause is not known; this reproduces the measured result: alpha
/// x 1.146 as a gamma-space coverage, converted to the linear-space coverage Bevy's sRGB target needs. Light pixels (the white time box)
/// keep their alpha. `FH1_FM2_HUD_ALPHA=0` leaves the textures as they are.
fn dark_panel_alpha(img: &mut Image, opacity: f32) {
    let off = std::env::var("FH1_FM2_HUD_ALPHA").is_ok_and(|v| v == "0");
    let Some(data) = img.data.as_mut() else { return };
    for px in data.chunks_exact_mut(4) {
        // The item's own opacity (the Lap row's bar is 54 %) multiplies the texture's alpha first.
        let a = px[3] as f32 / 255.0 * opacity;
        let dark = (px[0] as u32 + px[1] as u32 + px[2] as u32) <= 3 * 64;
        let a = if dark && !off && a > 0.0 && a < 1.0 + 1e-3 { 1.0 - (1.0 - (a * 1.146).min(1.0)).powf(2.2) } else { a };
        px[3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

fn png(path: &Path, srgb: bool, opacity: f32, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let bytes = std::fs::read(path).ok()?;
    match Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, srgb, ImageSampler::linear(), RenderAssetUsages::default()) {
        Ok(mut i) => {
            dark_panel_alpha(&mut i, opacity);
            Some(images.add(i))
        }
        Err(e) => {
            warn!("fm2 hud: {}: {e}", path.display());
            None
        }
    }
}

/// The font file stem for a scene font name: the longest `fontmap.xml` name the scene's name starts with.
fn font_target(map: &FontMap, name: &str) -> Option<String> {
    let n = name.trim().to_lowercase();
    map.mappings.iter().filter(|(k, _)| n.starts_with(k.as_str())).max_by_key(|(k, _)| k.len()).map(|(_, t)| t.to_lowercase())
}

fn load(mut commands: Commands, garage: Res<Garage>, track: Res<Track>, mut images: ResMut<Assets<Image>>) {
    if !track.id.starts_with("fm2/") || std::env::var("FH1_FM2_HUD").is_ok_and(|v| v == "0") {
        return;
    }
    let dir = garage.assets.join("imported/fm2/ui");
    let Ok(doc) = std::fs::read(dir.join("hud.json")).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string())) else {
        info!("fm2 hud: no imported FM2 HUD (run `fh1setup import-fm2 <disc> --only ui`); FH1's HUD stays");
        return;
    };
    let map = FontMap::parse(&std::fs::read_to_string(dir.join("fonts/fontmap.xml")).unwrap_or_default());
    let mut fonts = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("fonts")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if let Some(stem) = name.strip_suffix("_vector_aa.dt") {
                match std::fs::read(e.path()).map_err(|e| e.to_string()).and_then(|b| VFont::parse(&b).map_err(|e| e.to_string())) {
                    Ok(f) => {
                        fonts.insert(stem.to_owned(), f);
                    }
                    Err(err) => warn!("fm2 hud: font {name}: {err}"),
                }
            }
        }
    }
    let labels: HashMap<String, String> = doc["labels"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect()).unwrap_or_default();
    let textures = &doc["textures"];
    let tex_lookup = |name: &str| -> Option<(String, bool)> {
        textures.as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| (v["file"].as_str().unwrap_or_default().to_owned(), v["gamma"].as_bool().unwrap_or(true)))
    };
    let root = commands
        .spawn((
            Name::new("fm2 hud"),
            GlobalZIndex(5),
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        ))
        .id();
    let assist = |a: AssistKind, on: bool| Bind::Assist(a, on);
    let (mut quads, mut texts) = (0, 0);
    let mut tex_cache: HashMap<String, Option<Handle<Image>>> = HashMap::new();
    for (scene, items) in doc["scenes"].as_object().into_iter().flatten() {
        for it in items.as_array().into_iter().flatten() {
            let kind = it["kind"].as_str().unwrap_or("");
            let name = it["name"].as_str().unwrap_or("");
            let index = it["index"].as_u64().unwrap_or(0);
            let path: Vec<&str> = it["path"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let in_path = |p: &str| path.contains(&p);
            let part_one = scene.ends_with("PartOne");
            // Which pieces this module draws (see the module note for what is left out).
            let bind = match (kind, name) {
                ("model", _) if part_one && in_path("I_HUD_Lap") => Some(Bind::Static),
                ("text", "Laps") if part_one && in_path("I_HUD_Lap") => Some(Bind::Static),
                ("text", "NumLaps") if part_one && in_path("I_HUD_Lap") => Some(Bind::NumLaps),
                ("text", "CurLap") if part_one && in_path("I_HUD_Lap") => Some(Bind::CurLap),
                ("model", _) if !part_one && in_path("I_HUD_RACE_PANE") && (in_path("I_HUD_race_row0") || in_path("I_HUD_race_row1")) => Some(Bind::Static),
                ("text", "Row_Title") if !part_one && in_path("I_HUD_race_row0") => Some(Bind::RaceTitle(0)),
                ("text", "Row_Title") if !part_one && in_path("I_HUD_race_row1") => Some(Bind::RaceTitle(1)),
                ("text", "Row_Time") if !part_one && in_path("I_HUD_race_row0") => Some(Bind::RaceTime(0)),
                ("text", "Row_Time") if !part_one && in_path("I_HUD_race_row1") => Some(Bind::RaceTime(1)),
                _ if part_one || !(path.contains(&"Tach_Speed_Combined_3P") || name == "Tach_Speed_Combined_3P") => None,
                ("model", _) if matches!(index, 421 | 459) => Some(Bind::Static),
                ("model", _) if matches!(index, 488) => Some(assist(AssistKind::Abs, true)),
                ("model", _) if matches!(index, 492) => Some(assist(AssistKind::Abs, false)),
                ("model", _) if matches!(index, 502) => Some(assist(AssistKind::Tcs, true)),
                ("model", _) if matches!(index, 506) => Some(assist(AssistKind::Tcs, false)),
                ("model", _) if matches!(index, 474) => Some(assist(AssistKind::Stm, true)),
                ("model", _) if matches!(index, 478) => Some(assist(AssistKind::Stm, false)),
                ("text", "Speed_Text") => Some(Bind::Speed),
                ("text", "Gear_Text") => Some(Bind::Gear),
                ("text", "MPH_Label") => Some(Bind::Unit),
                _ => None,
            };
            let Some(bind) = bind else { continue };
            let (x, y, z) = (it["x"].as_f64().unwrap_or(0.0) as f32, it["y"].as_f64().unwrap_or(0.0) as f32, it["z"].as_f64().unwrap_or(0.0) as f32);
            let (sx, sy) = (it["sx"].as_f64().unwrap_or(1.0) as f32, it["sy"].as_f64().unwrap_or(1.0) as f32);
            let alpha = it["opacity"].as_str().and_then(|o| o.parse::<f32>().ok()).unwrap_or(100.0) / 100.0;
            // Nearer (lower z) draws on top; pictures stay below every text and gauge part (the boost plate sits nearer than its numbers
            // in the scene but is drawn under them).
            let z_index = if kind == "model" { ZIndex(-(z * 10.0).round() as i32) } else { ZIndex(-(z * 100.0).round() as i32 + 100) };
            if kind == "model" {
                let Some(tex) = it["texture"].as_str() else { continue };
                let Some((file, gamma)) = tex_lookup(tex) else { continue };
                // Always read as sRGB: the console blends in its gamma-encoded framebuffer, so the texture bytes are what the screen shows (the
        // `gamma` flag only says how the GPU decoded them while sampling).
        let _ = gamma;
        let handle = tex_cache.entry(format!("{tex}@{alpha:.2}")).or_insert_with(|| png(&dir.join(&file), true, alpha, &mut images)).clone();
                let Some(handle) = handle else { continue };
                let (w, h) = if it["rect"].as_bool().unwrap_or(false) {
                    (100.0 * sx, 100.0 * sy)
                } else {
                    // A "64x64" / "32_x_32" plane: its name is its size in scene units.
                    let b = it["buffer"].as_str().unwrap_or("");
                    let dims: Vec<f32> = b.split(|c: char| !c.is_ascii_digit()).filter_map(|t| t.parse().ok()).collect();
                    match dims.as_slice() {
                        [a, b] => (a * sx, b * sy),
                        _ => continue,
                    }
                };
                commands.spawn((
                    ChildOf(root),
                    HudItem { bind, font: None, em: 0.0, scale: Vec2::ONE, halign: 1, anchor: Vec2::new(x, y), shown: String::new() },
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent((W / 2.0 + x - w / 2.0) / W * 100.0),
                        top: Val::Percent((H / 2.0 - y - h / 2.0) / H * 100.0),
                        width: Val::Percent(w / W * 100.0),
                        height: Val::Percent(h / H * 100.0),
                        ..default()
                    },
                    ImageNode { image: handle, image_mode: NodeImageMode::Stretch, ..default() },
                    z_index,
                    if matches!(bind, Bind::Assist(..)) { Visibility::Hidden } else { Visibility::Inherited },
                ));
                quads += 1;
            } else {
                let font = it["font"].as_str().and_then(|f| font_target(&map, f));
                let size = it["size"].as_str().and_then(|s| s.parse::<f32>().ok()).unwrap_or(18.0);
                let colour = nums(it, "textcolor");
                let colour = if colour.len() >= 3 { Color::srgba(colour[0] / 255.0, colour[1] / 255.0, colour[2] / 255.0, alpha) } else { Color::srgba(1.0, 1.0, 1.0, alpha) };
                let halign = it["horzalign"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
                let initial = it["text"].as_str().unwrap_or("").to_owned();
                commands.spawn((
                    ChildOf(root),
                    HudItem { bind, font, em: size * PT, scale: Vec2::new(sx, sy), halign, anchor: Vec2::new(x, y), shown: String::new() },
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode { color: colour, image_mode: NodeImageMode::Stretch, ..default() },
                    z_index,
                    // The text's initial string is kept for statics (the unit label is replaced by the bound value).
                    Name::new(initial),
                ));
                texts += 1;
            }
        }
    }
    info!("fm2 hud: {quads} pictures, {texts} texts, {} fonts, {} labels", fonts.len(), labels.len());
    commands.insert_resource(Fm2HudOn(true));
    let mut dials = Vec::new();
    for kind in [DialKind::Rpm, DialKind::Boost] {
        let Some(spec) = fm2_dial::DialSpec::parse(&doc, kind == DialKind::Boost) else {
            warn!("fm2 hud: a gauge's pieces are not in hud.json (re-run `fh1setup import-fm2 <disc> --only ui`)");
            continue;
        };
        let needle = images.add(spec.needle_image());
        commands.spawn((
            DialNeedle(kind),
            ChildOf(root),
            picture_node(spec.centre),
            ImageNode { image: needle, image_mode: NodeImageMode::Stretch, ..default() },
            UiTransform::IDENTITY,
            ZIndex(spec.z.needle * 100),
        ));
        dials.push(DialState { kind, spec, key: None, parts: Vec::new() });
    }
    commands.insert_resource(Dials(dials));
    // The minimap: the track's centre line in the map rectangle of the scene (the game draws it natively; see fm2_minimap.rs).
    match fm2_minimap::Minimap::load(&garage.assets.join("imported").join(&track.id).join("ai/line.json"), &doc, &mut images) {
        Some(map) => {
            let (top_left, size) = map.rect();
            commands.spawn((
                MiniMapPicture,
                ChildOf(root),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent((W / 2.0 + top_left.x) / W * 100.0),
                    top: Val::Percent((H / 2.0 - top_left.y) / H * 100.0),
                    width: Val::Percent(size.x / W * 100.0),
                    height: Val::Percent(size.y / H * 100.0),
                    ..default()
                },
                ImageNode { image: map.handle.clone(), image_mode: NodeImageMode::Stretch, ..default() },
                ZIndex(50),
            ));
            commands.insert_resource(MiniMapState(map));
        }
        None => info!("fm2 hud: no track line for the minimap ({})", track.id),
    }
    commands.insert_resource(root_res(root));
    commands.insert_resource(Fm2Hud { font_map: map, fonts, labels, cache: HashMap::new() });
}

#[derive(Resource)]
struct HudRoot(Entity);

#[derive(Resource)]
struct MiniMapState(fm2_minimap::Minimap);

#[derive(Component)]
struct MiniMapPicture;

fn minimap_update(
    map: Option<Res<MiniMapState>>,
    mut picture: Query<&mut Visibility, With<MiniMapPicture>>,
    mut images: ResMut<Assets<Image>>,
    cars: Query<&Car>,
    settings: Res<Settings>,
    menu: Res<Menu>,
    rig: Res<CameraRig>,
) {
    let (Some(map), Ok(car)) = (map, cars.single()) else { return };
    let on = settings.hud && !menu.open && !rig.photo && !crate::cutscene::hide_hud();
    for mut v in &mut picture {
        *v = if on { Visibility::Inherited } else { Visibility::Hidden };
    }
    if !on {
        return;
    }
    let v = &car.0;
    let f = v.rotation * Vec3::NEG_Z;
    let fwd = Vec2::new(f.x, f.z).normalize_or(Vec2::NEG_Y);
    map.0.draw(&mut images, Vec2::new(v.position.x, v.position.z), fwd);
}

fn root_res(e: Entity) -> HudRoot {
    HudRoot(e)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialKind {
    Rpm,
    Boost,
}

struct DialState {
    kind: DialKind,
    spec: fm2_dial::DialSpec,
    /// What the parts were built for: (value range top, redline), rounded.
    key: Option<(i32, i32)>,
    parts: Vec<Entity>,
}

/// The third-person gauges (the tach and the boost gauge), built per car.
#[derive(Resource)]
struct Dials(Vec<DialState>);

#[derive(Component)]
struct DialNeedle(DialKind);

/// Design-space rectangle (left, top, width, height in percent of the window) of a SIZE x SIZE picture centred on `c`.
fn picture_node(c: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent((W / 2.0 + c.x - 120.0) / W * 100.0),
        top: Val::Percent((H / 2.0 - c.y - 120.0) / H * 100.0),
        width: Val::Percent(240.0 / W * 100.0),
        height: Val::Percent(240.0 / H * 100.0),
        ..default()
    }
}

/// What the dial shows for this car: (minimum, maximum, current value, redline). The tach reads rpm; the boost gauge reads atmospheres
/// (GUESS: the range of a turbo car is -1 .. 1 as in the Xenia shot, an engine without boost is -1 .. 0, and the needle is manifold
/// vacuum at closed throttle plus the boost the sim reports; FM2's `compressorMinBoostATM` / `PeakBoostATM` are not in the files read).
fn dial_values(kind: DialKind, v: &crate::vehicle::Vehicle, stock_boost_bar: f32) -> (f32, f32, f32, f32) {
    match kind {
        DialKind::Rpm => (0.0, v.data.torque_curve_max_rpm, v.rpm, v.data.redline_rpm),
        DialKind::Boost => {
            // `engine.StockBoost-bar` of the car's own data (0.9 bar on the R34 GT-R), in atmospheres.
            let peak = stock_boost_bar * 0.986_923;
            let load = v.torque_fraction.clamp(0.0, 1.0);
            let boost = if v.boost > 1.0 { (v.boost - 1.0) * peak } else { load * peak * (v.rpm / v.data.redline_rpm.max(1.0)).clamp(0.0, 1.0) };
            (-1.0, peak, -0.75 * (1.0 - load) + boost, 0.0)
        }
    }
}

/// `engine.StockBoost-bar` from the car's `physics.json` (0 for engines without forced induction or when the file has no such field).
fn stock_boost_bar(garage: &Garage) -> f32 {
    let Some(car) = garage.cars.get(garage.current) else { return 0.0 };
    let path = garage.assets.join("cars").join(car).join("physics.json");
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|d| d["engine"]["StockBoost-bar"].as_f64()).unwrap_or(0.0) as f32
}

/// Builds each dial for the player's car (ticks, red zone, numbers) when the car changes and turns the needles.
#[allow(clippy::too_many_arguments)]
fn dial_update(
    mut commands: Commands,
    dials: Option<ResMut<Dials>>,
    hud: Option<Res<Fm2Hud>>,
    root: Option<Res<HudRoot>>,
    cars: Query<&Car>,
    mut images: ResMut<Assets<Image>>,
    mut needles: Query<(&DialNeedle, &mut UiTransform, &mut Visibility)>,
    garage: Res<Garage>,
    mut boost_bar: Local<Option<(usize, f32)>>,
    settings: Res<Settings>,
    menu: Res<Menu>,
    rig: Res<CameraRig>,
) {
    let (Some(mut dials), Some(hud), Some(root)) = (dials, hud, root) else { return };
    let Ok(car) = cars.single() else { return };
    let v = &car.0;
    let on = settings.hud && !menu.open && !rig.photo && !crate::cutscene::hide_hud();
    // One switch for the whole HUD (the children follow the root).
    commands.entity(root.0).insert(if on { Visibility::Inherited } else { Visibility::Hidden });
    if boost_bar.is_none_or(|(c, _)| c != garage.current) {
        *boost_bar = Some((garage.current, stock_boost_bar(&garage)));
    }
    let bar = boost_bar.map_or(0.0, |b| b.1);
    for dial in &mut dials.0 {
        let (min, max, value, redline) = dial_values(dial.kind, v, bar);
        let layout = fm2_dial::layout(&dial.spec.params, min, max);
        let key = (max.round() as i32, redline.round() as i32);
        if dial.key != Some(key) {
            for e in dial.parts.drain(..) {
                commands.entity(e).despawn();
            }
            dial.key = Some(key);
            let img = images.add(dial.spec.dial_image(&layout, redline));
            let ticks = commands
                .spawn((
                    ChildOf(root.0),
                    picture_node(dial.spec.centre),
                    ImageNode { image: img, image_mode: NodeImageMode::Stretch, ..default() },
                    ZIndex(dial.spec.z.ticks * 100),
                ))
                .id();
            dial.parts.push(ticks);
            // Numbers: the script turns a copy of the number group by (`mStupidParameter` - the number's angle), pushes it half the text's
            // size out along (sin a, cos a) first (`alignRadial`), and keeps the text upright.
            let target = font_target(&hud.font_map, &dial.spec.number_font);
            let em = dial.spec.number_size * PT;
            let col = dial.spec.number_colour;
            for (text, a) in &layout.numbers {
                let (w, h) = match target.as_ref().and_then(|t| hud.fonts.get(t)) {
                    Some(font) => (font.text_width(text, 0.0) * em, font.line_height() * em),
                    None => (em * 0.6 * text.len() as f32, em),
                };
                let nudge = Vec2::new(a.sin() * w / 2.0, a.cos() * h / 2.0);
                let ang = dial.spec.params.stupid.to_radians() - a;
                let (s, c) = ang.sin_cos();
                let o = dial.spec.number_offset;
                let at = dial.spec.number_centre + nudge + Vec2::new(o.x * c - o.y * s, o.x * s + o.y * c);
                debug!("fm2 hud: dial number {text:?} angle {:.1} deg at ({:.1}, {:.1}) font {:?}", a.to_degrees(), at.x, at.y, target);
                let e = commands
                    .spawn((
                        ChildOf(root.0),
                        HudItem { bind: Bind::Static, font: target.clone(), em, scale: Vec2::ONE, halign: 1, anchor: at, shown: String::new() },
                        Node { position_type: PositionType::Absolute, ..default() },
                        ImageNode { color: Color::srgb(col[0], col[1], col[2]), image_mode: NodeImageMode::Stretch, ..default() },
                        ZIndex(dial.spec.z.numbers * 100),
                        Name::new(text.clone()),
                    ))
                    .id();
                dial.parts.push(e);
            }
            info!(
                "fm2 hud: {} dial {:.1} .. {:.1} (redline {:.0}): needle range {:.1} .. {:.1}, {} numbers, {} major / {} mid / {} minor ticks",
                if dial.kind == DialKind::Rpm { "tach" } else { "boost" },
                min,
                max,
                redline,
                layout.range.0,
                layout.range.1,
                layout.numbers.len(),
                layout.major.len(),
                layout.mid.len(),
                layout.minor.len()
            );
        }
        for (n, mut tf, mut vis) in &mut needles {
            if n.0 != dial.kind {
                continue;
            }
            *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
            let t = (value - layout.range.0) / (layout.range.1 - layout.range.0).max(1e-3);
            // Bevy's UI rotation turns clockwise on screen; the scene's z rotation is counter-clockwise.
            tf.rotation = Rot2::radians(-fm2_dial::needle_angle(&dial.spec.params, t));
        }
    }
}

fn mmss(t: f32) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u32;
    format!("{:02}:{:02}.{:03}", ms / 60000, ms / 1000 % 60, ms % 1000)
}

#[allow(clippy::too_many_arguments)]
fn drive(
    hud: Option<ResMut<Fm2Hud>>,
    mut items: Query<(&mut HudItem, &Name, &mut Node, &mut ImageNode, &mut Visibility)>,
    mut images: ResMut<Assets<Image>>,
    cars: Query<&Car>,
    rs: Res<RaceState>,
    events: Res<Events>,
    settings: Res<Settings>,
    menu: Res<Menu>,
    rig: Res<CameraRig>,
    frontend: Option<Res<super::fh1_launch::Fh1LaunchOn>>,
) {
    let Some(mut hud) = hud else { return };
    let hud = &mut *hud;
    let on = settings.hud && !menu.open && !rig.photo && !crate::cutscene::hide_hud() && !frontend.as_deref().is_some_and(|f| f.0);
    let car = cars.single().ok();
    let racing = matches!(rs.phase, RacePhase::Racing);
    let (race_t, lap_t) = if racing { (rs.clock_s, rs.clock_s - rs.lap_start_s) } else { (0.0, 0.0) };
    let total = rs.race.and_then(|r| events.races.get(r)).map(|r| r.laps);
    let speed = car.map_or(0.0, |c| if settings.metric { c.0.speed() * 3.6 } else { c.0.speed() * 2.236_94 });
    let gear = car.map_or(1, |c| c.0.gear);
    for (mut item, name, mut node, mut img, mut vis) in &mut items {
        if let Bind::Assist(kind, selected) = item.bind {
            let (enabled, active) = car.map_or((false, false), |c| {
                let v = &c.0;
                match kind {
                    AssistKind::Abs => (settings.abs, v.abs_active()),
                    AssistKind::Tcs => (settings.tcs, v.tcs_cut > 0.0),
                    AssistKind::Stm => (settings.stm, v.stm_active),
                }
            });
            *vis = if on && enabled && active == selected { Visibility::Inherited } else { Visibility::Hidden };
            continue;
        }
        let Some(font_name) = item.font.clone() else {
            *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
            continue;
        };
        *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
        if !on {
            continue;
        }
        let s = match item.bind {
            Bind::Static => name.as_str().to_owned(),
            Bind::CurLap => format!("{}", (rs.laps.len() + 1).min(total.map_or(usize::MAX, |t| t as usize).max(1))),
            Bind::NumLaps => total.map_or("-".to_owned(), |t| t.to_string()),
            Bind::RaceTitle(0) => hud.labels.get("race").cloned().unwrap_or_default(),
            Bind::RaceTitle(_) => hud.labels.get("lap").cloned().unwrap_or_default(),
            Bind::RaceTime(0) => mmss(race_t),
            Bind::RaceTime(_) => mmss(lap_t),
            Bind::Speed => format!("{}", speed.abs().round() as u32),
            Bind::Gear => if gear == 0 { "R".to_owned() } else { gear.to_string() },
            Bind::Unit => hud.labels.get(if settings.metric { "kmh" } else { "mph" }).cloned().unwrap_or_default(),
            Bind::Assist(..) => unreachable!(),
        };
        if item.shown == s {
            continue;
        }
        item.shown = s.clone();
        let Some(font) = hud.fonts.get(&font_name) else { continue };
        let em_px = (item.em * item.scale.y * SS).round().max(4.0) as u32;
        let key = (font_name.clone(), s.clone(), em_px);
        if !hud.cache.contains_key(&key) {
            let Some((image, size, origin)) = raster_in(font, &s, em_px as f32, true) else {
                img.image = Handle::default();
                continue;
            };
            let _ = origin;
            hud.cache.insert(key.clone(), (images.add(image), size, origin));
        }
        let (handle, size, origin) = hud.cache[&key].clone();
        // Design-space size and the pen origin inside the picture.
        let k = 1.0 / SS * (item.scale.x / item.scale.y);
        let (w, h) = (size.x as f32 * k, size.y as f32 / SS);
        let adv = font.text_width(&s, 0.0) * item.em * item.scale.x;
        let left = match item.halign {
            1 => item.anchor.x - adv / 2.0 - origin.x * k,
            2 => item.anchor.x - adv - origin.x * k,
            _ => item.anchor.x - origin.x * k,
        };
        // The anchor is the middle of the line box (ascent + descent).
        let top = item.anchor.y + h / 2.0;
        node.left = Val::Percent((W / 2.0 + left) / W * 100.0);
        node.top = Val::Percent((H / 2.0 - top) / H * 100.0);
        node.width = Val::Percent(w / W * 100.0);
        node.height = Val::Percent(h / H * 100.0);
        img.image = handle;
    }
}
