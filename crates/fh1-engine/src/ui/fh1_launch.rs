//! FH1's own front-end menu, from the game's `120_P_PRE_SPLASH` scene (`ui/intro.rs`'s title scene) — the *same*
//! scene the title screen is built from. VERIFIED with the fh1-ui `slide_probe` (2026-10-15):
//!
//! - the menu is the `SPLASH_MENU` component's `LIST`: `SPLASH_MENU_BUTTON1..5`, each a tape (`Crisp_Tape` /
//!   `smudge_tape`) with a `TEXT_BUTTON`; the list sits on the right of the 1280x720 space (x ~ 540..570);
//! - its container is the node named `Menu` (bsg node), which every scene slide sets to `opacity 0`: the *game code*
//!   opens it. We do the same (`set(menu, OPACITY, 100)`) after `goto_slide(Scene, "Menu")`, and the list draws;
//! - the `ButtonBar` + `HelpButton0..6` (A SELECT / B BACK); label ids `HelpButtons:IDS_Select` / `IDS_Back`;
//! - `Video` / `loading_image` (`horizon/placeholder.png`) — pointed at the running title movie for the backdrop.
//!
//! The two `P_PRE_SPLASH_MENU_BUTTON` / `_BUTTON2` objects are NOT the menu: they hang off `P_PRE_SPLASH_INSTALL`
//! (the disc-install prompt) and are hidden on every slide. The first two list items are relabelled with the game's
//! SINGLE PLAYER (`Main:IDS_Horizon`) / MULTIPLAYER (`Main:IDS_Multiplayer`).
//!
//! Only `Screen::Modes` (ui/launch.rs `launch_view`) drives this; the title and every sub-menu keep their own
//! drawing. While it draws, `Fh1LaunchOn` tells ui/loading.rs and ui/launch.rs to hide their own launch nodes: the
//! scene's meshes draw on the UI camera *under* Bevy UI, so the overlay must step aside for it to show.

use bevy::prelude::*;
use fh1_ui::anark::bgf::ObjectKind;
use fh1_ui::names;
use fh1_ui::player::{Player, Value};

use super::fmv;
use super::launch::MainMenu;
use super::loading::{Cover, Loading};
use super::scene::{AnarkScene, UiData};

/// The launch scene is drawing the menu this frame (ui/loading.rs and ui/launch.rs hide their own launch nodes).
#[derive(Resource, Default)]
pub struct Fh1LaunchOn(pub bool);

/// The locals of the `120_P_PRE_SPLASH` menu.
#[derive(Resource)]
pub struct Fh1Launch {
    entity: Entity,
    /// The `Menu` container node the slides keep hidden (`None` if the scene changes).
    menu: Option<usize>,
    /// Nodes hidden because they draw over the menu (the `Video` / `loading_image` backdrop quad).
    hide: Vec<usize>,
    /// `SPLASH_MENU_BUTTON1..5` and their `TEXT_BUTTON`.
    list: Vec<(usize, Option<usize>)>,
    /// `HelpButton0..6` and their `TEXT_BUTTON`.
    help: Vec<(usize, Option<usize>)>,
    /// `SPLASH_MENU_BREADCRUMB*` / `TEXT_SECTION`.
    section: Vec<usize>,
    /// The first two items' labels, from the string table (the game's SINGLE PLAYER / MULTIPLAYER).
    big_text: [String; 2],
    select: String,
    back: String,
    /// Last applied (cursor, item count); re-bound when it moves.
    shown: Option<(usize, usize)>,
}

impl Fh1Launch {
    /// How many `SPLASH_MENU_BUTTON` rows the scene has.
    pub fn list_len(&self) -> usize {
        self.list.len()
    }

    /// `entity` is the spawned scene; `p` locates the objects. None if the menu list isn't found.
    pub fn new(entity: Entity, p: &Player, data: &UiData) -> Option<Self> {
        let text = |r: &str, d: &str| {
            data.strings.as_ref().and_then(|s| s.resolve(r)).map(fh1_ui::strtable::strip_markup).unwrap_or_else(|| d.to_owned())
        };
        let button = |n: &str| focus_button(p, n).map(|b| (b, find_below(p, b, "TEXT_BUTTON")));
        let list: Vec<(usize, Option<usize>)> = (1..=5).filter_map(|i| button(&format!("SPLASH_MENU_BUTTON{i}"))).collect();
        if list.is_empty() {
            return None;
        }
        let help: Vec<(usize, Option<usize>)> = (0..7)
            .filter_map(|i| p.find(&format!("HelpButton{i}")).map(|b| (b, find_below(p, b, "Text_BUTTON").or_else(|| find_below(p, b, "TEXT_BUTTON")))))
            .collect();
        let section: Vec<usize> = (0..4)
            .filter_map(|i| {
                let n = if i == 0 { "SPLASH_MENU_BREADCRUMB".to_string() } else { format!("SPLASH_MENU_BREADCRUMB{}", i + 1) };
                p.find(&n).and_then(|c| find_below(p, c, "TEXT_SECTION"))
            })
            .collect();
        Some(Self {
            entity,
            menu: p.find("Menu"),
            hide: ["Fader"].into_iter().filter_map(|n| p.find(n)).collect(),
            list,
            help,
            section,
            big_text: [text("Main:IDS_Horizon", "SINGLE PLAYER"), text("Main:IDS_Multiplayer", "MULTIPLAYER")],
            select: text("HelpButtons:IDS_Select", "SELECT"),
            back: text("HelpButtons:IDS_Back", "BACK"),
            shown: None,
        })
    }
}

pub struct Fh1LaunchPlugin;

impl Plugin for Fh1LaunchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fh1LaunchOn>();
    }
}

/// `FH1_LAUNCH_SCENE=0`: keep the custom mode panel. On by default: drive the game's own `120_P_PRE_SPLASH` menu.
fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !matches!(std::env::var("FH1_LAUNCH_SCENE").as_deref(), Ok("0") | Ok("off")))
}

/// Shows the scene's menu on the mode screen and drives the list and the help bar from `MainMenu` (ui/launch.rs).
/// Hidden everywhere else (the title, the sub-menus, any other cover).
pub(crate) fn drive(
    mut on: ResMut<Fh1LaunchOn>,
    fh1: Option<ResMut<Fh1Launch>>,
    mut scenes: Query<&mut AnarkScene>,
    menu: Option<Res<MainMenu>>,
    ld: Res<Loading>,
) {
    let Some(mut fh1) = fh1 else { return };
    let view = (enabled() && ld.cover == Some(Cover::Launch)).then(|| menu.as_deref().and_then(|m| m.launch_view())).flatten();
    let Ok(mut sc) = scenes.get_mut(fh1.entity) else { return };
    let Some(view) = view else {
        if on.0 {
            info!("fh1 launch: menu off");
        }
        sc.visible = false;
        on.0 = false;
        fh1.shown = None;
        return;
    };
    // The running title movie as the scene's own backdrop (`horizon/placeholder.png`).
    match fmv::backdrop() {
        Some(h) => {
            sc.texture_overrides.insert("horizon/placeholder.png".into(), h);
        }
        None => {
            sc.texture_overrides.remove("horizon/placeholder.png");
        }
    }
    // The first two items carry the game's SINGLE PLAYER / MULTIPLAYER labels.
    let list: Vec<(String, bool)> = view
        .list
        .iter()
        .enumerate()
        .map(|(i, (l, g))| (if i < 2 { fh1.big_text[i].clone() } else { l.clone() }, *g))
        .collect();
    let cursor = view.cursor;
    let key = (cursor, list.len());
    if fh1.shown != Some(key) {
        let first = fh1.shown.is_none();
        // Clone the small vectors so the borrow of `fh1` ends before `p`.
        let menu = fh1.menu;
        let hide = fh1.hide.clone();
        let buttons = fh1.list.clone();
        let help = fh1.help.clone();
        let section = fh1.section.clone();
        let (select, back) = (fh1.select.clone(), fh1.back.clone());
        let p: &mut Player = &mut sc.player;
        if first {
            // Fresh state, the `Menu` scene slide, then open the container the slides keep closed (the game code's
            // job; the scene's `Menu` slide hides node `Menu` by track).
            p.goto_slide(0, "Menu");
            if let Some(m) = menu {
                p.set(m, names::OPACITY, Value::Float(100.0));
            }
            // The `Video` / `loading_image` quad is full screen and draws over the menu list: hide it. The title movie
            // stays as the launch cover's own backdrop instead.
            for o in &hide {
                p.set(*o, names::OPACITY, Value::Float(0.0));
            }
        }
        for (i, (b, t)) in buttons.iter().enumerate() {
            match list.get(i) {
                Some((l, greyed)) => {
                    if let Some(t) = t {
                        p.set_text(*t, l.clone());
                    }
                    // Scene opacity is 0..100 (not 0..1): 1.0 here would make the row 0.01 = invisible.
                    p.set(*b, names::OPACITY, Value::Float(if *greyed { 45.0 } else { 100.0 }));
                }
                // More list buttons than modes: hide the spares (the tape art goes with the button).
                None => {
                    p.set(*b, names::OPACITY, Value::Float(0.0));
                }
            }
        }
        for (i, (b, t)) in help.iter().enumerate() {
            let label = if i == 0 { Some(&select) } else if i == 1 { Some(&back) } else { None };
            match label {
                Some(l) => {
                    if let Some(t) = t {
                        p.set_text(*t, l.clone());
                    }
                }
                None => {
                    p.set(*b, names::OPACITY, Value::Float(0.0));
                }
            }
        }
        for t in section.iter() {
            // The top level has no breadcrumb section: hide the trail.
            p.set(*t, names::OPACITY, Value::Float(0.0));
        }
        // Static focus looks (the "Focus"/"Blur" slides are the transitions; we jump to the settled state).
        for (i, (b, _)) in buttons.iter().enumerate() {
            p.goto_slide(*b, if i == cursor && i < list.len() { "Focused" } else { "Blurred" });
        }
        fh1.shown = Some(key);
        if !on.0 {
            sc.player.update(0.0);
            let f = sc.player.evaluate();
            let n = f.draws.len();
            let sample: Vec<String> = f.draws.iter().take(8).map(|d| format!("{}@{:.0}/{:.2}", d.name, d.world[1][3], d.opacity)).collect();
            info!("fh1 launch: menu on ({} items, {n} draws, menu {:?}, sample [{}])", list.len(), menu, sample.join(", "));
        }
    }
    sc.visible = true;
    on.0 = true;
}

/// A focus-cycling component named `name` (the ones with a `Blurred` slide; `P_PRE_SPLASH_MENU_BUTTON` has a
/// namesake without one).
fn focus_button(p: &Player, name: &str) -> Option<usize> {
    for (i, o) in p.scene.bgf.objects.iter().enumerate() {
        if o.kind != ObjectKind::Component || p.name(i) != Some(name) {
            continue;
        }
        if p.scene.bgf.slides.iter().any(|s| s.component as usize == i && s.name == "Blurred") {
            return Some(i);
        }
    }
    None
}

/// First descendant of `from` with this name.
fn find_below(p: &Player, from: usize, name: &str) -> Option<usize> {
    let objs = &p.scene.bgf.objects;
    let mut stack = vec![from];
    while let Some(o) = stack.pop() {
        for (i, c) in objs.iter().enumerate() {
            if c.parent == o as i32 {
                if p.name(i) == Some(name) {
                    return Some(i);
                }
                stack.push(i);
            }
        }
    }
    None
}
