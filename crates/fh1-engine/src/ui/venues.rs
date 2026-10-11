//! Festival venues in free roam: drive into one's zone, press Enter / A, and its screen opens.
//!
//! The venues and their trigger zones come from the game's own `story/triggers.json` (the `festival_NN` zones of
//! autoshow.xml, paintshop.xml, workshops.xml, race_central.xml, carclub.xml and dlccenter.xml: engine position, `radius`
//! 21-24 m and `max_mph` 60, VERIFIED in the data). Inside a zone and under its max speed the prompt reads ENTER NOW.
//! Enter / the pad's X (automatic gearbox) or Select (manual, manual with clutch) enters a venue; the prompt's button
//! glyph is swapped to match. Autoshow opens Garage > Autoshow, Paint Shop and Dak's Garage open Customize (paint / upgrades are on its hub), Race
//! Central opens the world map (events). Car Club and the Marketplace have no screen yet: their prompt is the locked one.
//! Colorado only; `FH1_VENUES=0` turns it off.
//!
//! The prompt is the game's own `ActivationPrompt` widget of the `947_HUD` scene, the one the race code shows at an event
//! marker ("SIGN ME UP!"): the scene has `SET_EVENTICON_AUTOSHOW / PAINTSHOP / WORKSHOP / RACECENTRAL / CARCLUB /
//! DLCCENTER` for exactly these venues. Here its event-name text is the venue name and its call-to-action text is ENTER
//! NOW, both from the game's `Activities` string table (IDS_Description_*, IDS_EnterNow); the title line is left hidden.
//! INFERRED: that Paint Shop and Dak's Garage open the same Customize hub, and the locked texts (SLOW DOWN, COMING SOON).

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;

use super::hud::Fh1Hud;
use super::scene::{AnarkScene, UiData};
use super::{garage, Menu, Page, Settings, Shifting};
use crate::missions::hud::MissionHud;
use crate::missions::Activity;
use crate::race::anark_hud::{below, opacity, s as string, text, text_below};

const MPH: f32 = 0.44704;
/// The widget's button glyph (`horizon/icons/activationzoneicons/abutton.png`), swapped while an ENTER NOW prompt is up.
const GLYPH_KEY: &str = "horizon/icons/activationzoneicons/abutton.png";

/// The pad button that enters a venue and the game's image of it: X with the automatic gearbox, Select (Back / View) with
/// the manual ones (manual shifting uses the face buttons). Enter on the keyboard always works.
fn enter_button(shifting: Shifting) -> (GamepadButton, &'static str) {
    match shifting {
        Shifting::Automatic => (GamepadButton::West, "x_button"),
        Shifting::Manual | Shifting::ManualClutch => (GamepadButton::Select, "back_button"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Autoshow,
    PaintShop,
    Workshop,
    RaceCentral,
    CarClub,
    DlcCentre,
}

impl Kind {
    /// `festival_NN` object of the zone in triggers.json (festival_07 is the media centre: no position).
    fn from_object(object: &str) -> Option<Kind> {
        Some(match object {
            "festival_01" => Kind::Autoshow,
            "festival_02" => Kind::RaceCentral,
            "festival_03" => Kind::CarClub,
            "festival_04" => Kind::DlcCentre,
            "festival_05" => Kind::PaintShop,
            "festival_06" => Kind::Workshop,
            _ => return None,
        })
    }

    /// Fallback name when the string table isn't installed.
    fn name(self) -> &'static str {
        match self {
            Kind::Autoshow => "AUTOSHOW",
            Kind::PaintShop => "PAINT SHOP",
            Kind::Workshop => "DAK'S GARAGE",
            Kind::RaceCentral => "RACE CENTRAL",
            Kind::CarClub => "CAR CLUB",
            Kind::DlcCentre => "MARKETPLACE",
        }
    }

    /// The widget event that picks the venue's icon (947_HUD `SET_EVENTICON_*`).
    fn icon_event(self) -> &'static str {
        match self {
            Kind::Autoshow => "SET_EVENTICON_AUTOSHOW",
            Kind::PaintShop => "SET_EVENTICON_PAINTSHOP",
            Kind::Workshop => "SET_EVENTICON_WORKSHOP",
            Kind::RaceCentral => "SET_EVENTICON_RACECENTRAL",
            Kind::CarClub => "SET_EVENTICON_CARCLUB",
            Kind::DlcCentre => "SET_EVENTICON_DLCCENTER",
        }
    }

    /// Has a screen to open.
    fn usable(self) -> bool {
        !matches!(self, Kind::CarClub | Kind::DlcCentre)
    }
}

/// What the prompt's call to action says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pill {
    /// In the zone and slow enough: Enter / A opens the venue.
    Enter,
    /// In the zone but over the zone's max speed (the widget's locked mode).
    SlowDown,
    /// No screen behind it yet (the widget's locked mode).
    SoonOnly,
}

struct Venue {
    kind: Kind,
    /// Engine position (x, y, z), the zone's centre.
    pos: Vec3,
    /// Trigger zone radius (m) and max speed (mph).
    radius: f32,
    max_mph: f32,
    /// String id of the name (`IDS_Description_Autoshow`, in the `Activities` table).
    prompt: String,
}

/// The widget's parts in the `947_HUD` scene.
struct Widget {
    prompt: usize,
    name: Option<usize>,
    enter: Option<usize>,
    locked: Option<usize>,
}

impl Widget {
    fn find(p: &fh1_ui::player::Player) -> Option<Self> {
        let prompt = p.find("ActivationPrompt")?;
        let call = below(p, prompt, "Call_to_Action");
        let part = |from: Option<usize>, n: &str| from.and_then(|f| below(p, f, n)).and_then(|n| text_below(p, n));
        Some(Self {
            prompt,
            name: below(p, prompt, "Event_Name").and_then(|e| text_below(p, e)),
            enter: part(call, "Default"),
            locked: part(call, "Locked"),
        })
    }
}

#[derive(Resource, Default)]
struct Venues {
    list: Vec<Venue>,
    loaded: bool,
    widget: Option<Widget>,
    /// The prompt on screen: its venue and what it says.
    shown: Option<(usize, Pill)>,
    /// `Fh1Hud::reset_count` last seen (a scene reset re-hides the widget).
    resets: u32,
}

/// The `festival_NN` zones of `story/triggers.json` that have an engine position.
fn load(json: &str) -> Vec<Venue> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    v["zones"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|z| {
            let kind = Kind::from_object(z["object"].as_str()?)?;
            let e = z["engine"].as_array()?;
            let f = |i: usize| e.get(i)?.as_f64().map(|x| x as f32);
            Some(Venue {
                kind,
                pos: Vec3::new(f(0)?, f(1)?, f(2)?),
                radius: z["radius"].as_f64()? as f32,
                max_mph: z["max_mph"].as_f64()? as f32,
                prompt: z["prompt"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FH1_VENUES").map_or(true, |v| v != "0"))
}

pub struct VenuesPlugin;

impl Plugin for VenuesPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.init_resource::<Venues>()
            // After draw_menu: the press that opens a screen is spent before the menu reads input next frame.
            .add_systems(Update, venues.run_if(super::driving).after(super::draw_menu))
            // Menus, cutscenes and loading covers hide the prompt (venues only runs while driving).
            .add_systems(Update, hide_prompt.run_if(not(super::driving)));
    }
}

fn hide_prompt(mut venues: ResMut<Venues>, hud: Option<Res<Fh1Hud>>, mut scenes: Query<&mut AnarkScene>) {
    if venues.shown.take().is_none() {
        return;
    }
    if let (Some(hud), Some(w)) = (hud, venues.widget.as_ref()) {
        if let Ok(mut sc) = scenes.get_mut(hud.scene_entity()) {
            sc.player.fire_at("HIDE", w.prompt);
            sc.texture_overrides.remove(GLYPH_KEY);
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn venues(
    mut venues: ResMut<Venues>,
    track: Res<crate::track::Track>,
    garage: Res<crate::Garage>,
    settings: Res<Settings>,
    mut menu: ResMut<Menu>,
    mission_hud: Res<MissionHud>,
    (activity, rs): (Option<Res<Activity>>, Option<Res<crate::race::RaceState>>),
    cars: Query<&crate::Car>,
    (keys, pads): (Res<ButtonInput<KeyCode>>, Query<&Gamepad>),
    (ui, hud, assets): (Option<Res<UiData>>, Option<Res<Fh1Hud>>, Res<AssetServer>),
    mut scenes: Query<&mut AnarkScene>,
) {
    if !venues.loaded {
        venues.loaded = true;
        venues.list = std::fs::read_to_string(garage.assets.join("story/triggers.json")).map(|t| load(&t)).unwrap_or_default();
        info!("venues: {} festival venue zones", venues.list.len());
    }
    let (Some(ui), Some(hud)) = (ui, hud) else { return };
    let Ok(mut scene) = scenes.get_mut(hud.scene_entity()) else { return };
    let p = &mut scene.player;
    if venues.widget.is_none() {
        venues.widget = Widget::find(p);
        info!("venues: ActivationPrompt widget {}", if venues.widget.is_some() { "found" } else { "missing" });
    }
    if venues.widget.is_none() {
        return;
    }
    // A scene reset re-hid the widget: show it again if it should be up.
    if hud.reset_count() != venues.resets {
        venues.resets = hud.reset_count();
        venues.shown = None;
    }

    // Another prompt owns the widget or the screen (a race marker's, a mission's) or a race is on.
    let busy = track.id != "colorado"
        || !activity.is_none_or(|a| a.free())
        || mission_hud.prompt.is_some()
        || rs.is_some_and(|r| r.race.is_some() || r.prompt.is_some());
    let Some(car) = cars.iter().next() else { return };
    let here = Vec2::new(car.0.position.x, car.0.position.z);
    let mph = car.0.speed() / MPH;

    // The zone the car is in (the nearest centre when zones overlap), and what its prompt says.
    let want = if busy {
        None
    } else {
        venues
            .list
            .iter()
            .enumerate()
            .map(|(i, v)| (i, Vec2::new(v.pos.x, v.pos.z).distance(here), v))
            .filter(|(_, d, v)| *d < v.radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _, v)| {
                let pill = if !v.kind.usable() {
                    Pill::SoonOnly
                } else if mph >= v.max_mph {
                    Pill::SlowDown
                } else {
                    Pill::Enter
                };
                (i, pill)
            })
    };

    let changed = want != venues.shown;
    let (pad_button, glyph) = enter_button(settings.shifting);
    if changed {
        let w = venues.widget.as_ref().unwrap();
        match want {
            Some((i, pill)) => {
                let v = &venues.list[i];
                let name = if v.prompt.is_empty() { v.kind.name().to_owned() } else { string(&ui, &format!("Activities:{}", v.prompt), v.kind.name()) };
                opacity(p, Some(w.prompt), 100.0);
                text(p, w.name, name);
                text(p, w.enter, string(&ui, "Activities:IDS_EnterNow", "ENTER NOW"));
                text(p, w.locked, if pill == Pill::SoonOnly { "COMING SOON" } else { "SLOW DOWN" });
                p.fire_at(v.kind.icon_event(), w.prompt);
                p.fire_at(if pill == Pill::Enter { "SET_MODE_AVAILABLE" } else { "SET_MODE_LOCKED" }, w.prompt);
                // The title line above the name is the race prompt's (series name): not for venues.
                p.fire_at("HIDE_TITLE", w.prompt);
                p.fire_at("SHOW", w.prompt);
            }
            None => {
                p.fire_at("HIDE", w.prompt);
            }
        }
        venues.shown = want;
    }
    if changed {
        // The widget's A glyph becomes the button that actually enters (insert / remove, not replace: the scene's draw key
        // counts the overrides).
        scene.texture_overrides.remove(GLYPH_KEY);
        if matches!(want, Some((_, Pill::Enter))) {
            scene.texture_overrides.insert(GLYPH_KEY.into(), assets.load(format!("ui/textures/horizon/controller/{glyph}.png")));
        }
    }

    let Some((i, Pill::Enter)) = venues.shown else { return };
    let confirm = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) || pads.iter().any(|p| p.just_pressed(pad_button));
    if !confirm {
        return;
    }
    let (kind, prompt) = (venues.list[i].kind, venues.widget.as_ref().unwrap().prompt);
    scene.player.fire_at("HIDE", prompt);
    scene.texture_overrides.remove(GLYPH_KEY);
    venues.shown = None;
    super::open_menu(&mut menu, &settings, &garage);
    match kind {
        Kind::Autoshow => {
            menu.page = Page::Cars;
            menu.shop.open(garage::Mode::Shop);
            menu.open_browsers(&garage, &track.id);
        }
        Kind::PaintShop | Kind::Workshop => {
            menu.page = Page::Customize;
            menu.custom.open();
        }
        Kind::RaceCentral => {
            menu.page = Page::Map;
            menu.map_direct = true;
        }
        Kind::CarClub | Kind::DlcCentre => {}
    }
    menu.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_come_from_triggers_json() {
        let json = r#"{"zones":[
            {"object":"festival_07","name":"festival_07","radius":3,"max_mph":10,"prompt":"IDS_Description_MediaCenter","engine":null},
            {"object":"festival_01","name":"festival_01","radius":21,"max_mph":60,"prompt":"IDS_Description_Autoshow","engine":[-868.919373,-7.934125,143.792099]},
            {"object":"gasstation_01","radius":70,"max_mph":8,"prompt":"x","engine":[1.0,2.0,3.0]},
            {"object":"festival_06","radius":24,"max_mph":60,"prompt":"IDS_Description_Workshop","engine":[-1028.46,-9.28,148.65]}
        ]}"#;
        let v = load(json);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].kind, Kind::Autoshow);
        assert_eq!((v[0].radius, v[0].max_mph), (21.0, 60.0));
        assert!((v[0].pos.z - 143.79).abs() < 1e-2);
        assert_eq!(v[1].kind, Kind::Workshop);
        assert_eq!(v[1].prompt, "IDS_Description_Workshop");
    }

    #[test]
    fn every_venue_has_an_icon_event_and_a_name() {
        for k in [Kind::Autoshow, Kind::PaintShop, Kind::Workshop, Kind::RaceCentral, Kind::CarClub, Kind::DlcCentre] {
            assert!(k.icon_event().starts_with("SET_EVENTICON_") && !k.name().is_empty());
        }
    }
}
