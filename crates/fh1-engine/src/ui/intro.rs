//! When FH1's movies play (data/extracted/plans/fmv.md §1-2; playback itself is ui/fmv.rs):
//!
//! - **Boot:** straight to the title. The original opens with T10_MS_Combined -> Dolby_Corona_Intro (order VERIFIED in
//!   4 Pinyon runs); left out by the user's decision (2026-10-09), and setup doesn't copy them.
//! - **Title:** PressStart loops as the launch cover's backdrop (ui/loading.rs) for as long as the launch cover is up
//!   (title and menus), with its own stereo track at `ui_volume` (fmv.md Q8: which audio the title uses is UNKNOWN).
//! - **Attract:** the PRESS START title idle for the title video's own length ([`ATTRACT_IDLE_S`], the real 53.93 s of
//!   `PressStart.wmv`) plays the front end's trailer ([`ATTRACT`]) full screen: the game's own `999_DEV_ATTRACT` scene
//!   (UI.zip; its `CUSTREND_MOVIERENDERER` names `Splash_Intros\Forza_Intro.wmv` and it fires `INTRO_DONE` at the end).
//!   The title scene's own `Fader` fades the screen to black first ([`FADE_OUT_S`], its `FadeOut` track) and back after
//!   ([`FADE_IN_S`], `FadeIn`), drawn by ui/loading.rs as the topmost overlay child (under the movie layer). Any input
//!   (key, mouse button, gamepad button) restarts the wait, and a press during the fade-out cancels the trailer (the
//!   intro must never begin after Start). It only ever runs on the PRESS START title ([`MainMenu::on_title`]): a menu,
//!   a load or the trailer itself ends it, and returning to the title restarts the wait. The interval itself is a
//!   compiled constant in `default.xex` (ForzaUI::CAttract, next to the `noattractvideo` debug switch): it is in no
//!   data file, so the title video's length is used.
//! - **First-time career** (intro/story.rs, story.md §2): the player starts in the Viper and drives to the festival (no
//!   opening cutscene, no takeover), festival arrival (FMV_01), the starter car choice (intro/starter.rs: three free cars of
//!   the Corrado's class), Race Central (FMV_02) and the first wristband, driven by `story_flags` / `fmv_seen` in
//!   profile.json. `FH1_INTRO=0` = none of it (FMV_01 / FMV_02 then never auto-play), `FH1_INTRO=force` = run it on any
//!   career (nothing saved).
//! - **Story** (once per profile, `fmv_seen`; Colorado free roam, no cover, no race): FMV_04 the first time the player
//!   stands in a street race marker (HubId 1..3; the original plays it on the first entry into a street hub from free roam).
//! - **Skip:** Enter / Space / A / Start / a click (launch::pressed) ends a full-screen movie (FMV_0N are
//!   `confirmskip="true"`; the original's skip button is UNKNOWN, fmv.md Q3). The press is swallowed for a moment so it
//!   doesn't also act on the menu behind ([`blocking`], read by launch.rs `main_menu`).
//!
//! Not played (fmv.md): Forza_Tone / Forza_Intro / MSGS_Logo (triggers UNKNOWN), WristbandGet / IE_Welcome (not on
//! the disc). Flags: `FH1_FMV=0` = no movies (ui/fmv.rs); `FH1_FMV_STORY=0` = no story movies (the title loop stays).

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

mod starter;
mod story;

pub use starter::choice_open;

use super::fmv::{Ended, Fmv, Target};
use super::loading::{Cover, Loading, ScreenFade};
use super::world_load::{GameMode, Mode};
use super::Settings;
use crate::progression::Profile;
use crate::race::{Events, RacePhase, RaceState};
use crate::track::Track;

const TITLE: &str = "PressStart";
/// The front end's attract trailer (ui/intro.rs header): what the game's `999_DEV_ATTRACT` scene plays.
const ATTRACT: &str = "Forza_Intro";
/// How long the PRESS START title idles before the attract trailer (s): the real length of the title's own
/// `PressStart.wmv` (ffmpeg: 00:00:53.93), the "about a minute" the trailer follows. Any input restarts the wait.
const ATTRACT_IDLE_S: f32 = 53.93;
/// The title scene's own screen fader (`120_P_PRE_SPLASH` component `Fader`, UI.zip): `FadeOut` takes its black
/// `Rectangle` (object 343, full screen) from opacity 0 to 100 over 900 ms and fires `FADE_OUT_COMPLETE`; `FadeIn`
/// returns it to 0 over 300 ms (`FADE_IN_COMPLETE`). Both are VERIFIED in the `.bgf` opacity tracks. The attract
/// scene `999_DEV_ATTRACT` also has a `FADE OUT` slide (2474 ms) but no opacity track (timeline only).
const FADE_OUT_S: f32 = 0.9;
/// The fader's `FadeIn` track: back from black (s).
const FADE_IN_S: f32 = 0.3;
/// A skip press is swallowed this long (s).
const SWALLOW_S: f32 = 0.3;
/// Story movies wait this long after a cover ends / the world is calm (s).
const STORY_DELAY_S: f32 = 1.0;
/// The title loop is retried at most this often after failures.
const TITLE_TRIES: u32 = 2;

/// A full-screen movie is up, or a skip press was just swallowed: menus / driving ignore input.
static SWALLOW: AtomicBool = AtomicBool::new(false);

/// The scripted intro keeps the radio off until Radio_Unlock (read by radio.rs `radio_volume`).
pub fn radio_off() -> bool {
    story::radio_off()
}

/// Input belongs to the movie layer this frame (launch.rs `main_menu` returns early).
pub fn blocking() -> bool {
    SWALLOW.load(Ordering::Relaxed) || super::fmv::fullscreen_active()
}

/// `FH1_FMV_STORY` (default on).
fn story_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !matches!(std::env::var("FH1_FMV_STORY").as_deref(), Ok("0") | Ok("off")))
}

#[derive(PartialEq, Eq, Debug)]
enum Phase {
    /// Waiting for the window to show (no title audio behind a hidden window).
    Wait,
    /// Title loop and story movies run from here.
    Run,
}

/// The attract sequence (module doc): idle on the title, fade the title out through its own `Fader`, play the
/// trailer, fade back in. Only one step is active at a time.
#[derive(PartialEq, Debug)]
enum Attract {
    /// Not on the PRESS START title (a menu, a load, movies off): nothing to time.
    Off,
    /// Seconds of no input on the title.
    Idle(f32),
    /// Seconds into the fader's `FadeOut` ([`FADE_OUT_S`]), before the trailer starts.
    FadeOut(f32),
    /// The trailer is up; the fader is at black behind it.
    Trailer,
    /// Seconds into the fader's `FadeIn` ([`FADE_IN_S`]), back to the title.
    FadeIn(f32),
}

#[derive(Resource)]
struct Intro {
    phase: Phase,
    swallow_until: f32,
    title_fails: u32,
    /// Real time since when the story conditions have held (None = not calm).
    calm_since: Option<f32>,
    /// The attract sequence: idle on the title, the fade, the trailer (module doc).
    attract: Attract,
    /// The last movie that ended (taken from `Fmv` by `flow`, read once by story::sequence).
    ended: Option<(String, Ended)>,
}

pub struct IntroPlugin;

impl Plugin for IntroPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Intro { phase: Phase::Wait, swallow_until: 0.0, title_fails: 0, calm_since: None, attract: Attract::Off, ended: None })
            .init_resource::<story::Seq>()
            .init_resource::<starter::Starter>()
            .add_systems(Update, story::hold_input.after(crate::read_input))
            // The starter choice: after the pause menu's input (it shuts a pause menu opened over the cards), before sync_pause.
            .add_systems(Update, (starter::input, starter::draw).chain().after(super::menu_mouse).before(super::sync_pause));
        if super::fmv::enabled() {
            app.add_systems(Update, (flow, story, story::sequence).chain().before(super::launch::main_menu));
        } else {
            app.add_systems(Update, story::sequence.before(super::launch::main_menu));
        }
    }
}

/// Dev movie, title loop, skip.
#[allow(clippy::too_many_arguments)]
fn flow(
    mut intro: ResMut<Intro>,
    mut fmv: ResMut<Fmv>,
    ld: Res<Loading>,
    mut fade: ResMut<ScreenFade>,
    settings: Res<Settings>,
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    menu: Option<Res<super::launch::MainMenu>>,
    (keys, mouse, pads): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>, Query<&Gamepad>),
) {
    let now = time.elapsed_secs();
    let volume = settings.ui_volume.clamp(0.0, 1.0);
    // Skip a full-screen movie.
    if matches!(fmv.playing(), Some((_, Target::FullScreen))) && super::launch::pressed(&keys, &mouse, &pads) {
        info!("fmv: skipped {}", fmv.playing().map_or("", |p| p.0));
        fmv.stop();
        intro.swallow_until = now + SWALLOW_S;
    }
    SWALLOW.store(now < intro.swallow_until, Ordering::Relaxed);
    if let Some((name, how)) = fmv.take_ended() {
        if name == TITLE && matches!(how, Ended::Failed(_)) {
            intro.title_fails += 1;
        }
        intro.ended = Some((name, how));
    }
    if !fmv.available() {
        return;
    }
    let launch = ld.cover == Some(Cover::Launch);
    match intro.phase {
        Phase::Wait => {
            if let Some(name) = super::fmv::dev_movie() {
                // FH1_FMV=<name>: that movie, nothing else at boot.
                fmv.play(name, Target::FullScreen, false, volume);
                intro.phase = Phase::Run;
            } else if !launch || windows.single().is_ok_and(|w| w.visible) {
                intro.phase = Phase::Run;
            }
        }
        Phase::Run => {
            let backdrop = matches!(fmv.playing(), Some((_, Target::Backdrop)));
            if launch && fmv.playing().is_none() && intro.title_fails < TITLE_TRIES && super::fmv::dev_movie().is_none() {
                fmv.play(TITLE, Target::Backdrop, true, volume);
            } else if !launch && backdrop {
                fmv.stop();
            }
            // Attract trailer (module doc): the PRESS START title idle for the title video's own length, then the
            // title scene's own `Fader` fades the screen to black and the trailer plays; after it, the fader returns.
            // A menu, the trailer itself, a boot movie or a load ends the wait; any input restarts it.
            let on_title = launch && menu.as_deref().is_some_and(|m| m.on_title()) && !blocking();
            let input = keys.get_just_pressed().next().is_some()
                || mouse.get_just_pressed().next().is_some()
                || pads.iter().any(|p| p.get_just_pressed().next().is_some());
            let dt = time.delta_secs();
            intro.attract = match intro.attract {
                Attract::Off => {
                    if on_title && backdrop {
                        Attract::Idle(0.0)
                    } else {
                        Attract::Off
                    }
                }
                Attract::Idle(idle) => {
                    if !on_title || !backdrop {
                        Attract::Off
                    } else if input {
                        Attract::Idle(0.0)
                    } else if idle + dt >= ATTRACT_IDLE_S {
                        info!("fmv: title idle {ATTRACT_IDLE_S:.2} s: fading out for the attract trailer ({ATTRACT})");
                        Attract::FadeOut(0.0)
                    } else {
                        Attract::Idle(idle + dt)
                    }
                }
                Attract::FadeOut(t) => {
                    // A press before the trailer starts cancels it: the intro must never begin after Start.
                    if input {
                        Attract::Idle(0.0)
                    } else {
                        let t = t + dt;
                        if t < FADE_OUT_S {
                            Attract::FadeOut(t)
                        } else if fmv.play(ATTRACT, Target::FullScreen, false, volume) {
                            Attract::Trailer
                        } else {
                            Attract::Off
                        }
                    }
                }
                Attract::Trailer => {
                    // Stays while the trailer is the full-screen movie; any other movie (the title restarted) or
                    // none means it ended or was skipped.
                    if matches!(fmv.playing(), Some((n, Target::FullScreen)) if n == ATTRACT) {
                        Attract::Trailer
                    } else {
                        Attract::FadeIn(0.0)
                    }
                }
                Attract::FadeIn(t) => {
                    let t = t + dt;
                    if t < FADE_IN_S {
                        Attract::FadeIn(t)
                    } else {
                        Attract::Off
                    }
                }
            };
            // The fader alpha (ui/loading.rs draws it as the topmost overlay child, under the movie layer; the title
            // scene's black `Rectangle`: 0 = clear, 1 = black).
            fade.0 = match intro.attract {
                Attract::FadeOut(t) => (t / FADE_OUT_S).clamp(0.0, 1.0),
                Attract::Trailer => 1.0,
                Attract::FadeIn(t) => (1.0 - t / FADE_IN_S).clamp(0.0, 1.0),
                _ => 0.0,
            };
        }
    }
}

/// Once-per-profile street-hub movie (FMV_04); FMV_01 / FMV_02 belong to story.rs.
#[allow(clippy::too_many_arguments)]
fn story(
    mut intro: ResMut<Intro>,
    mut fmv: ResMut<Fmv>,
    ld: Res<Loading>,
    settings: Res<Settings>,
    time: Res<Time<Real>>,
    (track, mode, race, events): (Res<Track>, Option<Res<GameMode>>, Option<Res<RaceState>>, Option<Res<Events>>),
    profile: Option<ResMut<Profile>>,
) {
    let now = time.elapsed_secs();
    let calm = story_on()
        && fmv.available()
        && super::fmv::dev_movie().is_none()
        && intro.phase == Phase::Run
        && fmv.playing().is_none()
        && ld.cover.is_none()
        && track.id == "colorado"
        && mode.as_deref().is_none_or(|m| m.0 == Mode::Horizon)
        && race.as_deref().is_none_or(|r| r.race.is_none() && r.phase == RacePhase::Idle);
    if !calm {
        intro.calm_since = None;
        return;
    }
    let since = *intro.calm_since.get_or_insert(now);
    let Some(mut profile) = profile else { return };
    if now - since < STORY_DELAY_S {
        return;
    }
    let seen = |p: &Profile, n: &str| p.data.fmv_seen.iter().any(|s| s == n);
    let pick = if !seen(&profile, "FMV_04")
        && race.as_deref().and_then(|r| r.prompt).and_then(|i| events.as_deref().and_then(|e| e.races.get(i))).is_some_and(|r| r.hub > 0)
    {
        Some("FMV_04")
    } else {
        None
    };
    let Some(name) = pick else { return };
    // Seen as it starts (the original's SetMovieSeen runs before the movie).
    profile.data.fmv_seen.push(name.into());
    profile.commit();
    fmv.play(name, Target::FullScreen, false, settings.ui_volume.clamp(0.0, 1.0));
}
