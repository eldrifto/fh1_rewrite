//! L1/L1b main menu (launch screen): the game's logo over one of FH1's loading backdrops, "Press Start / Enter", then
//! the mode menu. The world is chosen here BEFORE it loads (ui/world_load.rs): main.rs starts on a placeholder and
//! `world_load::apply_choice` loads the pick behind the loading screen (ui/loading.rs).
//!
//! - **HORIZON** (free roam): choose the open world (Colorado = FH1, Southern Europe = FH2, other imported worlds), with
//!   the last car (settings `car`); the pause menu still changes cars.
//! - **MOTORSPORT** (circuits: FM4 `imported/fm4/maps.json`, grouped by track, then layout with length / type; greyed
//!   with its requirement when FM4 isn't imported, FM3 listed greyed as "coming soon"): track -> vehicle (X1's car
//!   browser, ui/browser.rs) ->
//!   session: Grid (pole) / Pit / Flying start from the track's `spawns.json` kinds; a lap timer when the track has
//!   `track.json` timing (ui/laptimer.rs). Racing against AI: "coming soon".
//!
//! Map lists come from `Track::maps_by_game` (X1); the maps of optional games that aren't imported are listed greyed
//! (`track::locked_maps`: FH1 is the only required disc). The choices are remembered in settings.json (`map`,
//! `motorsport_track`, `car`).
//!
//! Without main.rs's L1b patch (no `PendingWorld` inserted, the world already loading behind the screen), the same
//! menu still works: the loaded map continues, another map is switched to in-process (`world_load::switch_world`).
//!
//! `FH1_LAUNCH_SCREEN=0` = off, `=1` = on even under automation or an explicit `--track`. It is skipped automatically
//! with `--track` and when automation is set (FH1_SHOT, FH1_AUTODRIVE, FH1_RACE, FH1_AI_RACE, FH1_TELEPORT, FH1_MENU,
//! FH1_UI_SCENE, FH1_PERF_TOUR, FH1_P6_AB, any FH1_P2_*), for agent launches (CLAUDECODE), and on a map relaunch.

use std::path::Path;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use fh1_ui::vfont::{Font as VFont, Mesh as VMesh};

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;

use super::browser::{catalog_for, BrowserRow, CarBrowser, CarCatalog, MapBrowser, Pick, LOCKED, LOCKED_ON};
use super::loading::{shadow, Fade, FadeBg, Loading, LoadingRoot};
use super::world_load::{Mode, PendingWorld, WorldChoice};

// ONLINE (server browser). Declared here so ui.rs stays untouched; the file is ui/online.rs.
#[path = "online.rs"]
mod online;
use online::{OnlinePick, OnlineScreen};
use super::{Settings, SettingsPath, UiFont, ACCENT};
use crate::track::{LockedMap, Track};
use crate::Garage;

/// The main menu's widgets (children of the loading root), shown only while it is up.
#[derive(Component)]
pub struct LaunchPart;

/// The "Press Start" line (pulses).
#[derive(Component)]
pub struct PressPrompt;

/// Menu panel (hidden on the title). Also the centred logo and the backdrop dimmer, which only the menu screens use.
#[derive(Component)]
pub struct MenuBox;

/// Marks the title images that pulse like the prompt text (the rasterised "PRESS START"; not the logo).
#[derive(Component)]
pub struct PressPulse;

/// The plain-text "PRESS START / ENTER" shown until the game's own font has drawn the real one ([`fill_press_text`]).
#[derive(Component)]
pub struct PressFallback;

#[derive(Component)]
pub struct MenuTitle;

#[derive(Component)]
pub struct MenuHint;

/// Row `n`: label and value texts.
#[derive(Component)]
pub struct RowLabel(usize);

#[derive(Component)]
pub struct RowValue(usize);

const ROWS: usize = 9;

/// Env vars that mean an automated run: no launch screen.
const AUTOMATION: [&str; 10] = ["FH1_SHOT", "FH1_AUTODRIVE", "FH1_RACE", "FH1_AI_RACE", "FH1_TELEPORT", "FH1_MENU", "FH1_UI_SCENE", "FH1_PERF_TOUR", "FH1_P6_AB", "CLAUDECODE"];

pub fn automation() -> bool {
    AUTOMATION.iter().any(|k| std::env::var_os(k).is_some()) || std::env::vars_os().any(|(k, _)| k.to_string_lossy().starts_with("FH1_P2_"))
}

/// Whether to show the launch screen / main menu this run.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| match std::env::var("FH1_LAUNCH_SCREEN").as_deref() {
        Ok("0") | Ok("off") => false,
        Ok("1") | Ok("on") => true,
        // `--track` given explicitly: load that map straight away.
        _ => !automation() && !std::env::args().any(|a| a == "--track"),
    })
}

/// Start / A / Enter / Space / a mouse click this frame.
pub fn pressed(keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>, pads: &Query<&Gamepad>) -> bool {
    keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space])
        || mouse.just_pressed(MouseButton::Left)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Start) || p.just_pressed(GamepadButton::South))
}

/// A Motorsport session start (spawns.json `kind`).
#[derive(Clone, Debug)]
struct Session {
    label: String,
    detail: String,
    /// `grid` / `pit` / `flying`; None = the first spawn; "ai" = not available yet.
    kind: Option<String>,
}

enum Screen {
    Title,
    Modes { cursor: usize },
    Worlds(MapBrowser),
    Tracks(MapBrowser),
    Cars(CarBrowser),
    Sessions { cursor: usize, list: Vec<Session> },
    Online(OnlineScreen),
}

#[derive(Resource)]
pub struct MainMenu {
    screen: Screen,
    horizon: Vec<(String, Vec<(String, String)>)>,
    motorsport: Vec<(String, Vec<(String, String)>)>,
    /// Greyed worlds / circuits of optional games that aren't imported (not in `horizon` / `motorsport`, which ONLINE
    /// and the loaders read).
    horizon_locked: Vec<LockedMap>,
    motorsport_locked: Vec<LockedMap>,
    /// Requirement of MOTORSPORT when no circuits are imported (the mode is greyed).
    motorsport_reason: String,
    catalog: Option<Arc<CarCatalog>>,
    track: Option<(String, String)>,
    car: Option<usize>,
    dirty: bool,
    /// Held-direction auto-repeat (browser.rs).
    repeat: super::browser::Repeat,
}

/// Motorsport groups: FM4 by track (maps.json `track`), rows = layouts with length / type.
fn motorsport_groups(assets: &Path, by_game: &[(String, Vec<(String, String)>)]) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut meta: std::collections::HashMap<String, serde_json::Value> = Default::default();
    if let Ok(b) = std::fs::read(assets.join("imported/fm4/maps.json")) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
            for m in v.as_array().or_else(|| v["maps"].as_array()).cloned().unwrap_or_default() {
                if let Some(id) = m["id"].as_str() {
                    meta.insert(id.to_owned(), m.clone());
                }
            }
        }
    }
    for (game, maps) in by_game {
        match game.as_str() {
            "FM4" => {
                for (id, name) in maps {
                    let m = meta.get(id);
                    let track = m.and_then(|m| m["track"].as_str()).map(str::to_owned).unwrap_or_else(|| name.split(" - ").next().unwrap_or(name).to_owned());
                    let layout = m.and_then(|m| m["layout"].as_str()).map(str::to_owned).unwrap_or_else(|| name.split(" - ").nth(1).unwrap_or(name).to_owned());
                    let mut label = layout;
                    if let Some(len) = m.and_then(|m| m["length_m"].as_f64()) {
                        label.push_str(&format!("   {:.2} km", len / 1000.0));
                    }
                    if let Some(t) = m.and_then(|m| m["type"].as_str()) {
                        label.push_str(&format!("  ·  {t}"));
                    }
                    let label = format!("{label}  ");
                    match out.iter_mut().find(|g| g.0 == format!("FM4  ·  {track}")) {
                        Some(g) => g.1.push((id.clone(), label)),
                        None => out.push((format!("FM4  ·  {track}"), vec![(id.clone(), label)])),
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// spawns.json kinds of a map: Grid (pole) / Pit / Flying, plus the AI race entry (not available yet).
fn sessions(assets: &Path, id: &str) -> Vec<Session> {
    let mut kinds: Vec<(String, f64)> = Vec::new();
    if let Ok(b) = std::fs::read(assets.join("imported").join(id).join("world/spawns.json")) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
            for s in v["spawns"].as_array().into_iter().flatten() {
                if let Some(k) = s["kind"].as_str() {
                    if !kinds.iter().any(|x| x.0 == k) {
                        kinds.push((k.to_owned(), s["speed_mph"].as_f64().unwrap_or(0.0)));
                    }
                }
            }
        }
    }
    let mut out: Vec<Session> = kinds
        .iter()
        .filter_map(|(k, mph)| {
            let (label, detail) = match k.as_str() {
                "grid" => ("Free practice  ·  grid start", "From pole position".to_string()),
                "pit" => ("Free practice  ·  pit start", "Out of the pit lane".to_string()),
                "flying" => ("Hot lap  ·  flying start", if *mph > 0.0 { format!("At {mph:.0} mph") } else { "Already at speed".into() }),
                _ => return None,
            };
            Some(Session { label: label.into(), detail, kind: Some(k.clone()) })
        })
        .collect();
    if out.is_empty() {
        out.push(Session { label: "Free practice".into(), detail: "From the start".into(), kind: None });
    }
    out.push(Session { label: "Race vs AI".into(), detail: "coming soon".into(), kind: Some("ai".into()) });
    out
}

/// World browser: opens on the game list (FH1 / FH2 / ...) with the cursor on the current world's game.
fn worlds(h: &[(String, Vec<(String, String)>)], locked: &[LockedMap], current: &str) -> MapBrowser {
    let mut b = MapBrowser::new(h.to_vec(), current).with_locked(locked.to_vec());
    b.show_games();
    b
}

/// Track browser of MOTORSPORT, opened on `current`.
fn tracks(m: &[(String, Vec<(String, String)>)], locked: &[LockedMap], current: &str) -> MapBrowser {
    MapBrowser::new(m.to_vec(), current).with_locked(locked.to_vec())
}

impl MainMenu {
    pub fn new(assets: &Path) -> Self {
        let by_game = Track::maps_by_game(assets);
        let motorsport = motorsport_groups(assets, &by_game);
        let horizon: Vec<(String, Vec<(String, String)>)> = by_game.into_iter().filter(|(g, _)| g != "FM4").collect();
        // Optional games that aren't imported: their worlds / circuits greyed.
        let (motorsport_locked, horizon_locked): (Vec<LockedMap>, Vec<LockedMap>) = crate::track::locked_maps(assets).into_iter().partition(|l| l.circuit);
        let motorsport_reason = crate::track::OPTIONAL_GAMES.iter().find(|g| g.circuits && !g.coming_soon).map_or_else(String::new, |g| g.requirement());
        // FH1_MAIN_MENU=modes|worlds: open on that screen (screenshots).
        let screen = match std::env::var("FH1_MAIN_MENU").as_deref() {
            Ok("modes") => Screen::Modes { cursor: 0 },
            Ok("worlds") => Screen::Worlds(worlds(&horizon, &horizon_locked, "colorado")),
            _ => Screen::Title,
        };
        Self {
            screen,
            horizon,
            motorsport,
            horizon_locked,
            motorsport_locked,
            motorsport_reason,
            catalog: None,
            track: None,
            car: None,
            dirty: true,
            repeat: Default::default(),
        }
    }

    /// (label, description, greyed).
    fn modes(&self) -> Vec<(&'static str, String, bool)> {
        let mut m = vec![("HORIZON", "Free roam in an open world".to_string(), false), ("ONLINE", "Free roam on a server with other players".to_string(), false)];
        if !self.motorsport.is_empty() {
            m.push(("MOTORSPORT", "Circuits: free practice and hot laps".to_string(), false));
        } else {
            // No circuits imported: shown greyed with what it needs.
            m.push(("MOTORSPORT", self.motorsport_reason.clone(), true));
        }
        m.push(("QUIT", String::new(), false));
        m
    }

    fn rows(&self) -> (String, Vec<BrowserRow>, usize, String) {
        match &self.screen {
            Screen::Title => (String::new(), Vec::new(), 0, String::new()),
            Screen::Modes { cursor } => (
                "Choose a mode".into(),
                self.modes().into_iter().map(|(l, v, locked)| BrowserRow { label: l.to_string(), value: (!v.is_empty()).then_some(v), locked }).collect(),
                *cursor,
                "Enter / A  select      Backspace / B  back".into(),
            ),
            Screen::Worlds(b) => ("HORIZON  ·  choose a world".into(), b.rows(), b.cursor, b.hint()),
            Screen::Tracks(b) => ("MOTORSPORT  ·  choose a track".into(), b.rows(), b.cursor, b.hint()),
            Screen::Cars(b) => (format!("MOTORSPORT  ·  {}", b.title()), b.rows(), b.cursor, b.hint()),
            Screen::Sessions { cursor, list } => (
                format!("MOTORSPORT  ·  {}", self.track.as_ref().map_or(String::new(), |t| t.1.trim().to_owned())),
                list.iter().map(|s| BrowserRow { label: s.label.clone(), value: Some(s.detail.clone()), locked: false }).collect(),
                *cursor,
                "Enter / A  start      Backspace / B  back".into(),
            ),
            Screen::Online(o) => (o.title(), o.rows(&self.horizon), o.cursor(), o.hint()),
        }
    }
}

fn map_name(groups: &[(String, Vec<(String, String)>)], id: &str) -> String {
    groups.iter().flat_map(|g| g.1.iter()).find(|m| m.0 == id).map_or_else(|| id.to_owned(), |m| m.1.trim().to_owned())
}

/// Builds the main menu widgets under the loading root: logo (or a text title), prompt, menu panel, legal line.
pub fn spawn(commands: &mut Commands, root: Entity, font: &UiFont, logo: Option<Handle<Image>>, title_logo: Option<Handle<Image>>) {
    let col = commands
        .spawn((
            LaunchPart,
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
        ))
        .id();
    // Darken the backdrop a little behind the title.
    commands.spawn((
        LaunchPart,
        MenuBox,
        ChildOf(col),
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
        FadeBg(0.3),
    ));
    // The title screen: the game's own 120_p_pre_splash layout (UI units in the 1280 x 720 space, from the centre, y up):
    // `Logo` = logomedium.png, 420 x 150 centred on (357, -187). Menu screens keep the logo centred above the panel.
    match (title_logo, logo) {
        (Some(img), _) => {
            commands.spawn((
                LaunchPart,
                PressPrompt,
                ChildOf(root),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent((640.0 + 357.0 - 210.0) / 12.8),
                    top: Val::Percent((360.0 + 187.0 - 75.0) / 7.2),
                    width: Val::Percent(420.0 / 12.8),
                    aspect_ratio: Some(2.8),
                    ..default()
                },
                ImageNode { image: img.clone(), image_mode: NodeImageMode::Stretch, ..default() },
                Fade(1.0),
            ));
            commands.spawn((
                LaunchPart,
                MenuBox,
                ChildOf(col),
                Node { width: Val::Percent(26.0), aspect_ratio: Some(2.8), ..default() },
                ImageNode { image: img, image_mode: NodeImageMode::Stretch, ..default() },
                Fade(1.0),
            ));
        }
        (None, Some(img)) => {
            commands.spawn((
                LaunchPart,
                ChildOf(col),
                Node { width: Val::Percent(44.0), aspect_ratio: Some(4.0), ..default() },
                ImageNode { image: img, image_mode: NodeImageMode::Stretch, ..default() },
                Fade(1.0),
            ));
            commands.spawn((LaunchPart, ChildOf(col), Text::new("REWRITE"), font.text(22.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)), shadow(), Fade(0.8)));
        }
        (None, None) => {
            commands.spawn((LaunchPart, ChildOf(col), Text::new("FORZA HORIZON"), font.text(96.0), TextColor(Color::WHITE), shadow(), Fade(1.0)));
            commands.spawn((LaunchPart, ChildOf(col), Text::new("REWRITE"), font.text(22.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)), shadow(), Fade(0.8)));
        }
    }
    // Menu panel.
    let panel = commands
        .spawn((
            LaunchPart,
            MenuBox,
            ChildOf(col),
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(760.0),
                max_width: Val::Percent(92.0),
                margin: UiRect::top(Val::Px(18.0)),
                padding: UiRect::all(Val::Px(18.0)),
                row_gap: Val::Px(4.0),
                border: UiRect::left(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.78)),
            FadeBg(0.78),
            BorderColor::all(ACCENT),
        ))
        .id();
    commands.spawn((LaunchPart, MenuTitle, ChildOf(panel), Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() }, Text::new(""), font.text(20.0), TextColor(ACCENT), shadow(), Fade(1.0)));
    for i in 0..ROWS {
        let row = commands
            .spawn((LaunchPart, ChildOf(panel), Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(24.0), ..default() }))
            .id();
        commands.spawn((LaunchPart, RowLabel(i), ChildOf(row), Text::new(""), font.text(24.0), TextColor(Color::WHITE), shadow(), Fade(1.0)));
        commands.spawn((LaunchPart, RowValue(i), ChildOf(row), Text::new(""), font.text(17.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)), Fade(0.6)));
    }
    commands.spawn((LaunchPart, MenuHint, ChildOf(panel), Node { margin: UiRect::top(Val::Px(10.0)), ..default() }, Text::new(""), font.text(14.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.5)), Fade(0.5)));
    // "PRESS START": the scene's TitleTapeText is right-aligned at x = 527, y = -260. Plain text until the game's own font
    // has drawn it (fill_press_text), which also leaves a small keyboard hint here.
    commands
        .spawn((
            LaunchPart,
            PressFallback,
            ChildOf(root),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Percent((640.0 - 527.0) / 12.8),
                top: Val::Percent((360.0 + 260.0 - 16.0) / 7.2),
                ..default()
            },
        ))
        .with_child((LaunchPart, PressPrompt, Text::new("PRESS START / ENTER"), font.text(26.0), TextColor(Color::WHITE), shadow(), Fade(1.0)));
    commands
        .spawn((
            LaunchPart,
            ChildOf(root),
            Node { position_type: PositionType::Absolute, bottom: Val::Px(14.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        ))
        .with_child((
            LaunchPart,
            Text::new("Unofficial reimplementation running on your own game files. Not affiliated with Microsoft, Turn 10 or Playground Games."),
            font.text(13.0),
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
            shadow(),
            Fade(0.6),
        ));
}

fn save(settings: &Settings, path: &SettingsPath) {
    if let Some(dir) = path.0.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(b) = serde_json::to_vec_pretty(settings) {
        crate::perf::writer::replace(path.0.clone(), b);
    }
}

/// Main menu input (runs while the launch cover is up; ui/loading.rs runs after it).
#[allow(clippy::too_many_arguments)]
pub fn main_menu(
    mut menu: ResMut<MainMenu>,
    mut ld: ResMut<Loading>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    time: Res<Time<Real>>,
    (garage, track, mut settings, settings_path): (Res<Garage>, Res<Track>, ResMut<Settings>, Res<SettingsPath>),
    mut pending: Option<ResMut<PendingWorld>>,
    mut exit: MessageWriter<AppExit>,
    (mut actions, mut switch): (MessageWriter<super::GameAction>, ResMut<super::world_load::MapSwitch>),
    (mut typed, mut net_join): (MessageReader<bevy::input::keyboard::KeyboardInput>, ResMut<crate::net::NetConnect>),
    profile: Option<Res<crate::progression::Profile>>,
    mut snd: MessageWriter<super::sfx::UiSfx>,
) {
    // A boot movie is up, or its skip press was just swallowed (ui/intro.rs).
    if !ld.on_launch() || super::intro::blocking() || crate::cutscene::swallowing() {
        return;
    }
    let now = time.elapsed_secs();
    // B1: the browser's controller mapping (held repeat, LB/RB games, LT/RT jumps, X/View filters); Esc = back and
    // Start = confirm here too.
    let mut input = super::browser::read_input(&keys, &pads, &mut menu.repeat, time.delta_secs());
    input.back |= keys.just_pressed(KeyCode::Escape);
    input.confirm |= pads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    let car_now = settings.car.as_ref().and_then(|c| garage.cars.iter().position(|x| x == c)).unwrap_or(garage.current);
    // Car ownership (progression::wallet, FH1_OWNERSHIP=0 = every car): only owned cars start a world or are listed.
    let owned = |i: usize| -> bool {
        !crate::progression::wallet::ownership_on() || profile.as_deref().is_none_or(|p| garage.cars.get(i).is_some_and(|c| crate::progression::wallet::owns(p, c)))
    };
    let car_now = if owned(car_now) {
        car_now
    } else {
        // The first owned car, in garage-list order (a new career: the starter Corrado).
        profile.as_deref().and_then(|p| crate::progression::wallet::owned(p).iter().find_map(|o| garage.cars.iter().position(|c| *c == o.car))).unwrap_or(car_now)
    };
    let owned_catalog = |cat: Arc<CarCatalog>| -> Arc<CarCatalog> {
        if crate::progression::wallet::ownership_on() && profile.is_some() { Arc::new(cat.subset(|e| owned(e.index))) } else { cat }
    };
    let mut start: Option<WorldChoice> = None;
    let menu = &mut *menu;
    let modes = menu.modes();
    let typed: Vec<bevy::input::keyboard::KeyboardInput> = typed.read().cloned().collect();
    if let Screen::Online(o) = &mut menu.screen {
        // Server answers arrive over several frames.
        menu.dirty |= o.poll();
    }
    match &mut menu.screen {
        Screen::Title => {
            if pressed(&keys, &mouse, &pads) {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::ACCEPT));
                menu.screen = Screen::Modes { cursor: 0 };
                menu.dirty = true;
            }
        }
        Screen::Modes { cursor } => {
            if input.vertical != 0 {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::VSCROLL));
                *cursor = (*cursor as i32 + input.vertical).rem_euclid(modes.len() as i32) as usize;
                menu.dirty = true;
            } else if input.back {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::CANCEL));
                menu.screen = Screen::Title;
                menu.dirty = true;
            } else if input.confirm {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::ACCEPT));
                match modes.get(*cursor).map(|m| m.0) {
                    Some("HORIZON") => {
                        // Greyed worlds count too: without FH2 the list still opens to show it.
                        let n: usize = menu.horizon.iter().map(|g| g.1.len()).sum::<usize>() + menu.horizon_locked.len();
                        if n <= 1 {
                            let id = menu.horizon.first().and_then(|g| g.1.first()).map_or_else(|| "colorado".into(), |m| m.0.clone());
                            start = Some(WorldChoice { track: id, car: Some(car_now), mode: Mode::Horizon, start: None });
                        } else {
                            let current = settings.map.clone().unwrap_or_else(|| "colorado".into());
                            menu.screen = Screen::Worlds(worlds(&menu.horizon, &menu.horizon_locked, &current));
                        }
                    }
                    Some("ONLINE") => {
                        menu.screen = Screen::Online(OnlineScreen::new(&settings_path.0));
                    }
                    // Greyed (no circuits imported): stays on the mode list.
                    Some("MOTORSPORT") if !menu.motorsport.is_empty() => {
                        let current = settings.motorsport_track.clone().unwrap_or_default();
                        menu.screen = Screen::Tracks(tracks(&menu.motorsport, &menu.motorsport_locked, &current));
                    }
                    Some("QUIT") => {
                        exit.write(AppExit::Success);
                    }
                    _ => {}
                }
                menu.dirty = true;
            }
        }
        Screen::Worlds(b) => match {
            let p = b.step(input);
            if let Some(s) = super::sfx::pick(&p) {
                snd.write(s);
            }
            p
        } {
            Pick::Browsing { changed } => menu.dirty |= changed,
            Pick::Leave => {
                menu.screen = Screen::Modes { cursor: 0 };
                menu.dirty = true;
            }
            Pick::Map(id) => start = Some(WorldChoice { track: id, car: Some(car_now), mode: Mode::Horizon, start: None }),
            Pick::Car(_) => {}
        },
        Screen::Tracks(b) => match {
            let p = b.step(input);
            if let Some(s) = super::sfx::pick(&p) {
                snd.write(s);
            }
            p
        } {
            Pick::Browsing { changed } => menu.dirty |= changed,
            Pick::Leave => {
                menu.screen = Screen::Modes { cursor: modes.iter().position(|m| m.0 == "MOTORSPORT").unwrap_or(0) };
                menu.dirty = true;
            }
            Pick::Map(id) => {
                let name = map_name(&menu.motorsport, &id);
                menu.track = Some((id, name));
                let cat = owned_catalog(menu.catalog.get_or_insert_with(|| catalog_for(&garage.assets, &garage.cars)).clone());
                menu.screen = Screen::Cars(CarBrowser::new(cat, menu.car.unwrap_or(car_now)));
                menu.dirty = true;
            }
            Pick::Car(_) => {}
        },
        Screen::Cars(b) => match {
            let p = b.step(input);
            if let Some(s) = super::sfx::pick(&p) {
                snd.write(s);
            }
            p
        } {
            Pick::Browsing { changed } => menu.dirty |= changed,
            Pick::Leave => {
                let current = menu.track.as_ref().map_or_else(String::new, |t| t.0.clone());
                menu.screen = Screen::Tracks(tracks(&menu.motorsport, &menu.motorsport_locked, &current));
                menu.dirty = true;
            }
            Pick::Car(i) => {
                menu.car = Some(i);
                let list = menu.track.as_ref().map_or_else(Vec::new, |t| sessions(&garage.assets, &t.0));
                menu.screen = Screen::Sessions { cursor: 0, list };
                menu.dirty = true;
            }
            Pick::Map(_) => {}
        },
        Screen::Sessions { cursor, list } => {
            if input.vertical != 0 {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::VSCROLL));
                *cursor = (*cursor as i32 + input.vertical).rem_euclid(list.len().max(1) as i32) as usize;
                menu.dirty = true;
            } else if input.back {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::CANCEL));
                let cat = owned_catalog(menu.catalog.clone().unwrap_or_else(|| catalog_for(&garage.assets, &garage.cars)));
                menu.screen = Screen::Cars(CarBrowser::new(cat, menu.car.unwrap_or(car_now)));
                menu.dirty = true;
            } else if input.confirm {
                snd.write(super::sfx::UiSfx::play(super::sfx::keys::ACCEPT));
                if let (Some(s), Some((id, _))) = (list.get(*cursor), menu.track.as_ref()) {
                    if s.kind.as_deref() != Some("ai") {
                        start = Some(WorldChoice { track: id.clone(), car: menu.car, mode: Mode::Motorsport, start: s.kind.clone() });
                    }
                }
            }
        }
        Screen::Online(o) => match {
            let p = o.step(&input, &typed, &keys, &menu.horizon);
            match &p {
                OnlinePick::Browsing { changed: true } => {
                    snd.write(super::sfx::UiSfx::play(super::sfx::keys::VSCROLL));
                }
                OnlinePick::Leave => {
                    snd.write(super::sfx::UiSfx::play(super::sfx::keys::CANCEL));
                }
                OnlinePick::Join { .. } => {
                    snd.write(super::sfx::UiSfx::play(super::sfx::keys::ACCEPT));
                }
                _ => {}
            }
            p
        } {
            OnlinePick::Browsing { changed } => menu.dirty |= changed,
            OnlinePick::Leave => {
                menu.screen = Screen::Modes { cursor: modes.iter().position(|m| m.0 == "ONLINE").unwrap_or(0) };
                menu.dirty = true;
            }
            OnlinePick::Join { addr, password, map } => {
                // net.rs connects once this world is loaded (crate::net::NetConnect).
                net_join.0 = Some((addr, password, map.clone()));
                start = Some(WorldChoice { track: map, car: Some(car_now), mode: Mode::Horizon, start: None });
            }
        },
    }
    let Some(choice) = start else { return };
    // Remember the choices.
    match choice.mode {
        Mode::Horizon => settings.map = Some(choice.track.clone()),
        Mode::Motorsport => settings.motorsport_track = Some(choice.track.clone()),
    }
    if let Some(name) = choice.car.and_then(|i| garage.cars.get(i)) {
        settings.car = Some(name.clone());
    }
    save(&settings, &settings_path);
    let groups = if choice.mode == Mode::Horizon { &menu.horizon } else { &menu.motorsport };
    let name = match choice.mode {
        Mode::Motorsport => menu.track.as_ref().map_or_else(|| choice.track.clone(), |t| map_name(&menu.motorsport, &t.0)),
        Mode::Horizon => map_name(groups, &choice.track),
    };
    let sub = match choice.mode {
        Mode::Horizon => "Loading the world".to_string(),
        Mode::Motorsport => "Loading the circuit".to_string(),
    };
    if let Some(p) = pending.as_mut() {
        // Main menu first: load the chosen world now, behind the loading card (every map, FH2's Anthem included: its
        // post chain / glows / cube reload in-process, X1b).
        info!("main menu: {:?} on {} (car {:?})", choice.mode, choice.track, choice.car.and_then(|i| garage.cars.get(i)));
        p.choice = Some(choice);
        ld.start_world(now, name.to_uppercase(), sub);
    } else if choice.track == track.id {
        // The world is already loading behind the menu (main.rs without the L1b patch).
        if let Some(i) = choice.car.filter(|&i| i != garage.current) {
            actions.write(super::GameAction::SelectCar(i));
        }
        ld.continue_loaded(now, name.to_uppercase(), sub);
    } else {
        // Another map than the loaded one: unload it and load the choice in-process (X1c, world_load::switch_world).
        info!("main menu: switching to {name} in-process");
        if let Some(i) = choice.car.filter(|&i| i != garage.current) {
            actions.write(super::GameAction::SelectCar(i));
        }
        switch.0 = Some(choice.track.clone());
        ld.start_world(now, name.to_uppercase(), sub);
        let _ = &mut exit;
    }
}

/// Draws the panel rows (windowed around the cursor) when they change.
#[allow(clippy::type_complexity)]
pub fn draw_main_menu(
    mut menu: ResMut<MainMenu>,
    mut boxes: Query<&mut Visibility, (With<MenuBox>, Without<PressPrompt>)>,
    mut prompt: Query<&mut Visibility, (With<PressPrompt>, Without<MenuBox>)>,
    mut texts: ParamSet<(
        Query<&mut Text, With<MenuTitle>>,
        Query<&mut Text, With<MenuHint>>,
        Query<(&mut Text, &mut TextColor, &RowLabel)>,
        Query<(&mut Text, &RowValue)>,
    )>,
) {
    if !menu.dirty {
        return;
    }
    menu.dirty = false;
    let title_screen = matches!(menu.screen, Screen::Title);
    for mut v in &mut boxes {
        v.set_if_neq(if title_screen { Visibility::Hidden } else { Visibility::Inherited });
    }
    for mut v in &mut prompt {
        v.set_if_neq(if title_screen { Visibility::Inherited } else { Visibility::Hidden });
    }
    let (title, rows, cursor, hint) = menu.rows();
    let first = cursor.saturating_sub(ROWS / 2).min(rows.len().saturating_sub(ROWS));
    for mut t in &mut texts.p0() {
        t.0 = title.clone();
    }
    for mut t in &mut texts.p1() {
        t.0 = hint.clone();
    }
    for (mut t, mut c, RowLabel(i)) in &mut texts.p2() {
        let k = first + i;
        let sel = k == cursor;
        t.0 = rows.get(k).map_or_else(String::new, |r| format!("{}{}", if sel { "›  " } else { "   " }, r.label));
        let a = c.0.alpha();
        // Greyed rows (optional games that aren't imported, browser.rs LOCKED).
        let locked = rows.get(k).is_some_and(|r| r.locked);
        c.0 = match (locked, sel) {
            (true, true) => LOCKED_ON.with_alpha(a),
            (true, false) => LOCKED.with_alpha(a),
            (false, true) => ACCENT.with_alpha(a),
            (false, false) => Color::WHITE.with_alpha(a),
        };
    }
    for (mut t, RowValue(i)) in &mut texts.p3() {
        t.0 = rows.get(first + i).and_then(|r| r.value.clone()).unwrap_or_default();
    }
}

/// "PRESS START" in the game's own vector font (the scene's `TitleTapeText`, font `horizon_e`): rasterised once on the CPU into a
/// white-on-transparent image, since Bevy UI text can't use the game's .dt fonts. Runs until the ui group is loaded, then
/// replaces the plain-text prompt and leaves a small keyboard hint under it.
#[allow(clippy::too_many_arguments)]
pub fn fill_press_text(
    mut done: Local<bool>,
    mut commands: Commands,
    data: Option<Res<super::scene::UiData>>,
    root: Query<Entity, With<LoadingRoot>>,
    mut fallback: Query<&mut Node, With<PressFallback>>,
    mut images: ResMut<Assets<Image>>,
    font: Res<UiFont>,
) {
    if *done {
        return;
    }
    let (Some(data), Ok(root)) = (data, root.single()) else { return };
    *done = true;
    let Some((image, size)) = data.font("Horizon_E").and_then(|f| raster(f, "PRESS START", RASTER_EM)) else { return };
    for mut n in &mut fallback {
        n.display = Display::None;
    }
    // The raster is RASTER_EM px per em for a 24 px (design units) em: shown at half size. Right-aligned at x = 527, centred on
    // y = -260 of the 1280 x 720 space.
    let (w, h) = (size.x as f32 / 2.0, size.y as f32 / 2.0);
    commands.spawn((
        LaunchPart,
        PressPrompt,
        PressPulse,
        ChildOf(root),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Percent((640.0 - 527.0) / 12.8),
            top: Val::Percent((360.0 + 260.0 - h / 2.0) / 7.2),
            width: Val::Percent(w / 12.8),
            aspect_ratio: Some(size.x as f32 / size.y as f32),
            ..default()
        },
        ImageNode { image: images.add(image), image_mode: NodeImageMode::Stretch, ..default() },
        Fade(1.0),
    ));
    commands.spawn((
        LaunchPart,
        PressPrompt,
        ChildOf(root),
        Node { position_type: PositionType::Absolute, right: Val::Percent((640.0 - 527.0) / 12.8), top: Val::Percent((360.0 + 260.0 + h / 2.0 + 4.0) / 7.2), ..default() },
        Text::new("ENTER / SPACE / CLICK"),
        font.text(13.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
        shadow(),
        Fade(0.7),
    ));
}

/// Pixels per em of the prompt raster (the scene's 36 pt text at node scale 0.5 is an 18 pt = 24 px em in the 1280 x 720 space;
/// drawn at twice that and shown at half size).
const RASTER_EM: f32 = 48.0;

/// Draws `s` in a vector font into a white image whose alpha is the glyph coverage (3 x 3 supersampled), with the Loop-Blinn
/// curve rules of fh1-ui's vfont: inner meshes fill where u² - v <= 0, the outer (negated-x) curve triangles where it is > 0.
fn raster(font: &VFont, s: &str, em: f32) -> Option<(Image, UVec2)> {
    const SS: usize = 3;
    let mut pen = 0.0f32;
    let mut glyphs = Vec::new();
    let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for c in s.chars() {
        if c == ' ' {
            pen += font.metrics.space_width;
            continue;
        }
        let g = font.glyph(c)?;
        for v in g.inner.verts.iter().chain(&g.outer.verts) {
            let p = Vec2::new(pen + g.offset[0] + v[0].abs() * g.scale, g.offset[1] + v[1] * g.scale);
            min = min.min(p);
            max = max.max(p);
        }
        glyphs.push((g, pen));
        pen += g.advance;
    }
    if glyphs.is_empty() || max.x <= min.x {
        return None;
    }
    let pad = 3.0;
    let (w, h) = (((max.x - min.x) * em + 2.0 * pad).ceil() as usize, ((max.y - min.y) * em + 2.0 * pad).ceil() as usize);
    let (sw, sh) = (w * SS, h * SS);
    let mut mask = vec![0u8; sw * sh];
    let to_px = |g_pen: f32, g: &fh1_ui::vfont::Glyph, v: &[f32; 4]| {
        let x = g_pen + g.offset[0] + v[0].abs() * g.scale;
        let y = g.offset[1] + v[1] * g.scale;
        Vec2::new(((x - min.x) * em + pad) * SS as f32, ((max.y - y) * em + pad) * SS as f32)
    };
    let fill = |mask: &mut Vec<u8>, mesh: &VMesh, outer: bool, g: &fh1_ui::vfont::Glyph, g_pen: f32| {
        for t in mesh.indices.chunks_exact(3) {
            let v: Vec<[f32; 4]> = t.iter().map(|&i| mesh.verts[i as usize]).collect();
            let (a, b, c) = (to_px(g_pen, g, &v[0]), to_px(g_pen, g, &v[1]), to_px(g_pen, g, &v[2]));
            let den = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
            if den.abs() < 1e-9 {
                continue;
            }
            let curve = v.iter().any(|q| q[2] != 0.0);
            let (x0, x1) = (a.x.min(b.x).min(c.x).floor().max(0.0) as usize, (a.x.max(b.x).max(c.x).ceil() as usize).min(sw - 1));
            let (y0, y1) = (a.y.min(b.y).min(c.y).floor().max(0.0) as usize, (a.y.max(b.y).max(c.y).ceil() as usize).min(sh - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let l1 = ((b.y - c.y) * (px - c.x) + (c.x - b.x) * (py - c.y)) / den;
                    let l2 = ((c.y - a.y) * (px - c.x) + (a.x - c.x) * (py - c.y)) / den;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                        continue;
                    }
                    let u = l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2];
                    let vv = l1 * v[0][3] + l2 * v[1][3] + l3 * v[2][3];
                    let f = u * u - vv;
                    let on = if outer { curve && f > 0.0 } else { !curve || f <= 0.0 };
                    if on {
                        mask[y * sw + x] = 1;
                    }
                }
            }
        }
    };
    for (g, g_pen) in &glyphs {
        fill(&mut mask, &g.inner, false, g, *g_pen);
        fill(&mut mask, &g.outer, true, g, *g_pen);
    }
    let mut data = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let mut n = 0u32;
            for dy in 0..SS {
                for dx in 0..SS {
                    n += mask[(y * SS + dy) * sw + x * SS + dx] as u32;
                }
            }
            data.extend_from_slice(&[255, 255, 255, (n * 255 / (SS * SS) as u32) as u8]);
        }
    }
    let image = Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    Some((image, UVec2::new(w as u32, h as u32)))
}
