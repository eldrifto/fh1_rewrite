//! FM2's pause screen on FM2 circuits: the game's own `I_PRE_All_Modes` layout (a column of 257 x 34 list buttons on the left, a
//! results table on the right, the A / B help bar along the bottom), drawn over the engine's pause menu in place of FH1's pause scene.
//! The menu's rows and input are the engine's (`ui::Menu`, main page); only the look is FM2's.
//!
//! From the scene (`imported/fm2/ui/hud.json`, scene `I_PRE_All_Modes`): button positions, textures (`X_TRN_ListButton_idle / focused /
//! highlite`), label font / size / colour with their black shadow, the title, table header and row geometry (747 x 60 rows,
//! `I_MEN_PlayerRow`), help bar texts. The labels are FM2's own strings (`Forza_Flow` Resume / Restart / Take Photo ...). Not from the
//! files: the dimming of the world behind it (a guess), the highlight texture's blend, and the table's contents (this engine's race
//! standings; best laps only for the player). `FH1_FM2_PAUSE=0` keeps FH1's pause scene.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use fh1_ui::vfont::{Font as VFont, FontMap};
use serde_json::Value;

use super::launch::raster_in;
use super::{Menu, Page, Settings};
use crate::race::RaceState;
use crate::track::Track;
use crate::Garage;

pub struct Fm2PausePlugin;

impl Plugin for Fm2PausePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load).add_systems(Update, draw);
    }
}

/// True while FM2's pause screen replaces FH1's pause scene on this map.
#[derive(Resource)]
pub struct Fm2PauseOn;

const W: f32 = 1280.0;
const H: f32 = 720.0;
const SS: f32 = 2.0;
const PT: f32 = 96.0 / 72.0;
/// Rows of the pause list and of the table.
const BUTTONS: usize = 8;
/// Centre x of the table: the header model's (139); the row component's own x is 0 in the file, the header and rows line up on the
/// same columns (header "Best Lap" ends at 502, a row's time at 362 + 139).
const TABLE_X: f32 = 139.0;

struct Layout {
    buttons: Vec<Vec2>,
    button_size: Vec2,
    button_font: String,
    button_em: f32,
    title: (Vec2, f32),
    header_y: f32,
    header_x: [f32; 3],
    row_y: Vec<f32>,
    row_size: Vec2,
    help: Vec<(Vec2, String)>,
}

#[derive(Resource)]
struct Pause {
    root: Entity,
    layout: Layout,
    fonts: HashMap<String, VFont>,
    map: FontMap,
    labels: HashMap<String, String>,
    cache: HashMap<(String, String, u32), (Handle<Image>, UVec2, Vec2)>,
    idle: Handle<Image>,
    focused: Handle<Image>,
    highlite: Handle<Image>,
    row: Handle<Image>,
    head: Handle<Image>,
    /// Entities that change: (button picture, button label, label shadow) per row, the table's text slots.
    buttons: Vec<(Entity, Entity, Entity)>,
    /// Focus overlays per button: the orange `focused` picture and the yellow `highlite` line.
    overlays: Vec<(Entity, Entity)>,
    table: Vec<Entity>,
    shown: Vec<String>,
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn png(path: &std::path::Path, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let b = std::fs::read(path).ok()?;
    Image::from_buffer(&b, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::default()).ok().map(|i| images.add(i))
}

fn load(mut commands: Commands, garage: Res<Garage>, track: Res<Track>, mut images: ResMut<Assets<Image>>) {
    if !track.id.starts_with("fm2/") || std::env::var("FH1_FM2_PAUSE").is_ok_and(|v| v == "0") || std::env::var("FH1_FM2_HUD").is_ok_and(|v| v == "0") {
        return;
    }
    let dir = garage.assets.join("imported/fm2/ui");
    let Ok(doc) = std::fs::read(dir.join("hud.json")).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string())) else { return };
    let Some(items) = doc["scenes"]["I_PRE_All_Modes"].as_array() else { return };
    let tex = |name: &str, images: &mut Assets<Image>| -> Option<Handle<Image>> {
        let file = doc["textures"].as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))?.1["file"].as_str()?.to_owned();
        png(&dir.join(file), images)
    };
    let (Some(idle), Some(focused), Some(highlite), Some(row), Some(head)) =
        (tex("X_TRN_ListButton_idle", &mut images), tex("X_TRN_ListButton_focused", &mut images), tex("X_TRN_ListButton_highlite", &mut images), tex("I_MEN_PlayerRow", &mut images), tex("I_MEN_BlueHeading", &mut images))
    else {
        info!("fm2 pause: textures missing (re-run `fh1setup import-fm2 <disc> --only ui`)");
        return;
    };
    // Button rows: the `ButtonGroup` groups under `ScrollingList_Buttons`, in file order.
    let buttons: Vec<Vec2> = items.iter().filter(|it| it["kind"] == "group" && it["name"] == "ButtonGroup").take(BUTTONS).map(|it| Vec2::new(f(&it["x"]), f(&it["y"]))).collect();
    let label = items.iter().find(|it| it["kind"] == "text" && it["name"] == "ButtonText");
    let title = items.iter().find(|it| it["kind"] == "text" && it["name"] == "Title");
    let by = |n: &str| items.iter().find(|it| it["kind"] == "text" && it["name"] == n).map_or(0.0, |it| f(&it["x"]));
    let header_y = items.iter().find(|it| it["kind"] == "text" && it["name"] == "Place Title").map_or(200.0, |it| f(&it["y"]));
    let mut row_y: Vec<f32> = items.iter().filter(|it| it["kind"] == "model" && it["name"] == "I_MEN_PlayerRow").map(|it| f(&it["y"])).collect();
    row_y.dedup();
    let help: Vec<(Vec2, String)> = ["A_Label", "B_Label"].iter().filter_map(|n| items.iter().find(|it| it["kind"] == "text" && it["name"] == *n)).map(|it| (Vec2::new(f(&it["x"]), f(&it["y"])), it["text"].as_str().unwrap_or("").to_owned())).collect();
    if buttons.len() < 7 || label.is_none() {
        return;
    }
    let label = label.unwrap();
    let map = FontMap::parse(&std::fs::read_to_string(dir.join("fonts/fontmap.xml")).unwrap_or_default());
    let mut fonts = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("fonts")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if let Some(stem) = name.strip_suffix("_vector_aa.dt") {
                if let Ok(font) = std::fs::read(e.path()).map_err(|e| e.to_string()).and_then(|b| VFont::parse(&b).map_err(|e| e.to_string())) {
                    fonts.insert(stem.to_owned(), font);
                }
            }
        }
    }
    let labels: HashMap<String, String> = doc["labels"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect()).unwrap_or_default();
    let layout = Layout {
        buttons,
        button_size: Vec2::new(257.0, 34.0),
        button_font: label["font"].as_str().unwrap_or("helveticaneuelt std med").to_owned(),
        button_em: label["size"].as_str().and_then(|s| s.parse::<f32>().ok()).unwrap_or(20.0) * PT,
        title: (title.map_or(Vec2::new(-521.0, 28.0), |it| Vec2::new(f(&it["x"]), f(&it["y"]))), title.and_then(|it| it["size"].as_str()?.parse::<f32>().ok()).unwrap_or(24.0) * PT),
        header_y,
        header_x: [by("Place Title"), by("Driver Title"), by("Race Points Title")],
        row_y,
        row_size: Vec2::new(747.0, 60.0),
        help,
    };
    let root = commands.spawn((Name::new("fm2 pause"), GlobalZIndex(20), Visibility::Hidden, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)))).id();
    let mut pause = Pause { root, layout, fonts, map, labels, cache: HashMap::new(), idle, focused, highlite, row, head, buttons: Vec::new(), overlays: Vec::new(), table: Vec::new(), shown: Vec::new() };
    // Buttons: picture, label, shadow.
    let n = pause.layout.buttons.len();
    for i in 0..n {
        let c = pause.layout.buttons[i];
        let pic = commands.spawn((ChildOf(root), rect(c, pause.layout.button_size), ImageNode { image: pause.idle.clone(), image_mode: NodeImageMode::Stretch, ..default() }, ZIndex(1), Visibility::Hidden)).id();
        let shadow = commands.spawn((ChildOf(root), Node { position_type: PositionType::Absolute, ..default() }, ImageNode { color: Color::BLACK, ..default() }, ZIndex(2), Visibility::Hidden)).id();
        let text = commands.spawn((ChildOf(root), Node { position_type: PositionType::Absolute, ..default() }, ImageNode { color: Color::srgb(224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0), ..default() }, ZIndex(3), Visibility::Hidden)).id();
        let foc = commands.spawn((ChildOf(root), rect(c, pause.layout.button_size), ImageNode { image: pause.focused.clone(), image_mode: NodeImageMode::Stretch, ..default() }, ZIndex(1), Visibility::Hidden)).id();
        let hi = commands.spawn((ChildOf(root), rect(c, pause.layout.button_size), ImageNode { image: pause.highlite.clone(), image_mode: NodeImageMode::Stretch, ..default() }, ZIndex(1), Visibility::Hidden)).id();
        pause.overlays.push((foc, hi));
        pause.buttons.push((pic, text, shadow));
    }
    // Table: header picture and rows.
    commands.spawn((ChildOf(root), rect(Vec2::new(TABLE_X, 203.0), Vec2::new(747.0, 57.0)), ImageNode { image: pause.head.clone(), image_mode: NodeImageMode::Stretch, ..default() }, ZIndex(1)));
    // The row picture is a frame with a cut-out middle (148 x 58): stretched as a nine-slice.
    let slicer = TextureSlicer { border: BorderRect { min_inset: Vec2::new(40.0, 12.0), max_inset: Vec2::new(40.0, 12.0) }, center_scale_mode: SliceScaleMode::Stretch, sides_scale_mode: SliceScaleMode::Stretch, max_corner_scale: 1.0 };
    for &y in &pause.layout.row_y.clone() {
        commands.spawn((ChildOf(root), rect(Vec2::new(TABLE_X, y), pause.layout.row_size), ImageNode { image: pause.row.clone(), image_mode: NodeImageMode::Sliced(slicer.clone()), ..default() }, ZIndex(1)));
    }
    commands.insert_resource(Fm2PauseOn);
    commands.insert_resource(pause);
    info!("fm2 pause: {} buttons, {} table rows", n, 0);
}

/// Absolute node for a design-space rectangle centred on `c` (y up).
fn rect(c: Vec2, size: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent((W / 2.0 + c.x - size.x / 2.0) / W * 100.0),
        top: Val::Percent((H / 2.0 - c.y - size.y / 2.0) / H * 100.0),
        width: Val::Percent(size.x / W * 100.0),
        height: Val::Percent(size.y / H * 100.0),
        ..default()
    }
}

fn target(map: &FontMap, name: &str) -> Option<String> {
    let n = name.trim().to_lowercase();
    map.mappings.iter().filter(|(k, _)| n.starts_with(k.as_str())).max_by_key(|(k, _)| k.len()).map(|(_, t)| t.to_lowercase())
}

impl Pause {
    /// Puts `text` on `entity` anchored at `at` (design, y up): `align` 0 left, 1 centre, 2 right.
    fn put(&mut self, images: &mut Assets<Image>, node: &mut Node, img: &mut ImageNode, font_name: &str, em: f32, text: &str, at: Vec2, align: u8) {
        let Some(t) = target(&self.map, font_name) else { return };
        let Some(font) = self.fonts.get(&t) else { return };
        let em_px = (em * SS).round().max(4.0) as u32;
        let key = (t.clone(), text.to_owned(), em_px);
        if !self.cache.contains_key(&key) {
            let Some((image, size, origin)) = raster_in(font, text, em_px as f32, true) else {
                img.image = Handle::default();
                return;
            };
            self.cache.insert(key.clone(), (images.add(image), size, origin));
        }
        let (handle, size, origin) = self.cache[&key].clone();
        let (w, h) = (size.x as f32 / SS, size.y as f32 / SS);
        let adv = font.text_width(text, 0.0) * em;
        let left = match align {
            1 => at.x - adv / 2.0 - origin.x / SS,
            2 => at.x - adv - origin.x / SS,
            _ => at.x - origin.x / SS,
        };
        node.left = Val::Percent((W / 2.0 + left) / W * 100.0);
        node.top = Val::Percent((H / 2.0 - (at.y + h / 2.0)) / H * 100.0);
        node.width = Val::Percent(w / W * 100.0);
        node.height = Val::Percent(h / H * 100.0);
        img.image = handle;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    pause: Option<ResMut<Pause>>,
    menu: Res<Menu>,
    settings: Res<Settings>,
    garage: Res<Garage>,
    track: Res<Track>,
    maps: Res<super::MapChoices>,
    rs: Res<RaceState>,
    mut vis: Query<&mut Visibility>,
    mut parts: Query<(&mut Node, &mut ImageNode)>,
    mut images: ResMut<Assets<Image>>,
    mut spawned: Local<Vec<Entity>>,
) {
    let Some(mut pause) = pause else { return };
    let pause = &mut *pause;
    let on = menu.open && matches!(menu.page, Page::Main);
    if let Ok(mut v) = vis.get_mut(pause.root) {
        *v = if on { Visibility::Inherited } else { Visibility::Hidden };
    }
    for &(pic, text, shadow) in &pause.buttons {
        for e in [pic, text, shadow] {
            if let Ok(mut v) = vis.get_mut(e) {
                *v = if on { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
    if !on {
        // The table's per-open texts are rebuilt on the next open.
        for e in spawned.drain(..) {
            commands.entity(e).despawn();
        }
        return;
    }
    let (rows, _) = super::items(&menu, &settings, &garage, &track, &maps);
    let n = pause.layout.buttons.len();
    let em = pause.layout.button_em;
    let font = pause.layout.button_font.clone();
    for i in 0..n {
        let (pic, text, shadow) = pause.buttons[i];
        let c = pause.layout.buttons[i];
        let visible = i < rows.len();
        for e in [pic, text, shadow] {
            if let Ok(mut v) = vis.get_mut(e) {
                *v = if visible { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
        if !visible {
            continue;
        }
        let focus = i == menu.cursor;
        let (foc, hi) = pause.overlays[i];
        for e in [foc, hi] {
            if let Ok(mut v) = vis.get_mut(e) {
                *v = if focus { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
        let label = fm2_label(&pause.labels, &rows[i].label);
        // The scene puts the label 2 px below the button's centre and its shadow 1 px right and below that.
        if let Ok((mut node, mut img)) = parts.get_mut(text) {
            pause.put(&mut images, &mut node, &mut img, &font, em, &label, c + Vec2::new(0.0, -2.0), 1);
        }
        if let Ok((mut node, mut img)) = parts.get_mut(shadow) {
            pause.put(&mut images, &mut node, &mut img, &font, em, &label, c + Vec2::new(1.0, -3.0), 1);
        }
    }
    // Static texts (title, header, help bar) and the standings are respawned when the menu opens.
    if spawned.is_empty() {
        let mut add = |commands: &mut Commands, pause: &mut Pause, images: &mut Assets<Image>, font: &str, em: f32, text: &str, at: Vec2, align: u8, colour: Color| {
            let mut node = Node { position_type: PositionType::Absolute, ..default() };
            let mut img = ImageNode { color: colour, ..default() };
            pause.put(images, &mut node, &mut img, font, em, text, at, align);
            let e = commands.spawn((ChildOf(pause.root), node, img, ZIndex(4))).id();
            spawned.push(e);
        };
        let white = Color::WHITE;
        let (tp, ts) = pause.layout.title;
        // The title sits in the header group at (3, -273): bottom left, above the help bar (INFERRED from the group offsets).
        let tp = Vec2::new(tp.x + 3.0, -273.0 + tp.y);
        add(&mut commands, pause, &mut images, "handelgothicef medium", ts, "PAUSE", tp, 0, Color::srgb(235.0 / 255.0, 235.0 / 255.0, 235.0 / 255.0));
        let (hy, hx) = (pause.layout.header_y, pause.layout.header_x);
        let heads = [pause.labels.get("pl").cloned().unwrap_or("PL".into()), pause.labels.get("driver").cloned().unwrap_or("Driver".into()), pause.labels.get("best_lap").cloned().unwrap_or("Best Lap".into())];
        for (k, t) in heads.iter().enumerate() {
            add(&mut commands, pause, &mut images, &font, 20.0 * PT, t, Vec2::new(hx[k], hy), if k == 2 { 2 } else { 0 }, white);
        }
        let help = pause.layout.help.clone();
        for (k, (at, text)) in help.iter().enumerate() {
            let t = if k == 0 { pause.labels.get("select").cloned() } else { pause.labels.get("back").cloned() }.unwrap_or(text.clone());
            add(&mut commands, pause, &mut images, &font, 16.0 * PT, &t, *at, 0, Color::srgb(0.9, 0.9, 0.9));
        }
        // Standings: position, name, best lap (player only; this engine keeps no best lap for the AI).
        let mut order: Vec<&crate::race::Racer> = rs.racers.iter().collect();
        order.sort_by_key(|r| r.position);
        let best = rs.laps.iter().copied().fold(f32::INFINITY, f32::min);
        let ys = pause.layout.row_y.clone();
        for (k, r) in order.iter().take(ys.len()).enumerate() {
            let y = ys[k];
            add(&mut commands, pause, &mut images, &font, 26.0 * PT, &format!("{}", r.position), Vec2::new(-200.0, y - 2.0), 1, Color::srgb(1.0, 247.0 / 255.0, 153.0 / 255.0));
            add(&mut commands, pause, &mut images, &font, 18.0 * PT, &r.name, Vec2::new(-234.0 + TABLE_X, y + 12.0), 0, Color::srgb(0.92, 0.92, 0.92));
            let t = if r.is_player && best.is_finite() { format!("{:02}:{:06.3}", (best / 60.0) as u32, best % 60.0) } else { "--:--.---".to_owned() };
            add(&mut commands, pause, &mut images, &font, 25.0 * PT, &t, Vec2::new(362.0 + TABLE_X, y + 12.0), 2, Color::srgb(0.92, 0.92, 0.92));
        }
    }
}

/// FM2's label for one of this engine's pause rows (FM2's own strings where the game has the row).
fn fm2_label(labels: &HashMap<String, String>, ours: &str) -> String {
    let get = |k: &str, d: &str| labels.get(k).cloned().unwrap_or_else(|| d.to_owned());
    match ours {
        "Resume" => get("resume", "Resume"),
        "Restart at start" => get("restart", "Restart"),
        "Photo mode" => get("photo", "Take Photo"),
        "Options" => get("options", "Options"),
        "Quit to desktop" => get("quit_race", "Quit Race"),
        other => other.to_owned(),
    }
}
