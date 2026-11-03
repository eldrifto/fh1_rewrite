<p align="center">
  <img src="assets/branding/hero.png" alt="FH1 Rewrite: an orange supercar at the Horizon festival in Colorado" width="100%">
</p>

<p align="center">
  <a href="https://github.com/Sm1jjj/fh1_rewrite/releases/latest"><img alt="Download" src="https://img.shields.io/github/v/release/Sm1jjj/fh1_rewrite?label=download&style=for-the-badge&color=ff7a3c"></a>
  <img alt="Windows x64" src="https://img.shields.io/badge/windows-x64-1b1446?style=for-the-badge">
  <img alt="Linux x64 (experimental)" src="https://img.shields.io/badge/linux-x64%20(experimental)-1b1446?style=for-the-badge">
  <img alt="Rust + Bevy" src="https://img.shields.io/badge/rust-bevy%200.19-1b1446?style=for-the-badge">
  <img alt="License" src="https://img.shields.io/badge/license-GPL--3.0-1b1446?style=for-the-badge">
</p>

# FH1 Rewrite

A clean reimplementation of **Forza Horizon** (Xbox 360, 2012) in Rust and Bevy. Drive the Colorado open world with
the original cars, physics data, music and festival, rebuilt as a new PC engine. Bring your own disc: the installer
converts it on your machine, and nothing from the game is shipped here.

**What works today:** the full Colorado map with streaming scenery, props, crowds and traffic; 175 cars with their
own handling data, engine sounds and the in-game radio; races with AI drivers, wristband progression and credits;
a garage with paint, rims, body kits and upgrades; a PBR "remaster" renderer with time of day; and online free roam
on dedicated servers. Performance is a priority: the renderer is tuned for high frame rates, and a Console quality
preset targets low-end PCs (integrated graphics) at the original game's 720p. Parity with the original is the goal and still a work in progress.

## Play

1. [Download the latest release](https://github.com/Sm1jjj/fh1_rewrite/releases/latest) and unzip it anywhere with
   about 30 GB free.
2. Open **FH1 Rewrite.exe** and choose your Forza Horizon disc: the `.iso`, a `.zip` holding it, or the folder it's in.
3. Click **Install**, wait (up to an hour on slower PCs), then **PLAY**.

No Rust, Python or other tools needed. A gamepad is recommended; keyboard works too. The launcher updates itself
when a new release is out.

**Linux (experimental):** download `FH1Rewrite-<version>-linux-x64.tar.gz` from the same release, extract it and run
`./fh1-rewrite`. It's built automatically for every release and gets less testing than Windows.

### Graphics settings

Pause → **Options → Graphics**:

| Setting | Choices |
|---|---|
| Quality | **Console (720p)** for integrated graphics / older PCs, Low, Medium, High, Ultra. Sets shadows, draw distance, reflections, particles and crowd detail |
| Anti-aliasing | Off, FXAA, SMAA, SMAA + MSAA 2x (default), MSAA 2x, MSAA 4x |
| Motion blur | Off, Low, Medium (default), High |
| Render scale | 50-100 %, upscaled and sharpened |

Picking Console also switches to FXAA and turns motion blur off; you can change both afterwards.

### Got other Forza discs?

Forza Horizon is the only requirement. If you also own these Xbox 360 games, tick them in the installer to add
their content to the same game. Anything you don't own stays locked in the menus.

| Game | Adds |
|---|---|
| Forza Horizon 2 | 216 cars and the Southern Europe open world |
| Forza Motorsport 4 | 501 cars and 83 circuit layouts (a MOTORSPORT mode with grid, pit and flying starts) |
| Forza Motorsport 3 | 311 cars (415 with the Content Install Disc) and 52 circuit layouts at 17 venues (69 layouts with Disc 2) with their scenery |
| Forza Motorsport 2 | 32 circuit layouts with textured, lightmapped scenery (road brightness is still being matched to the original on a few tracks) and its cars |
| Forza Motorsport (Xbox, 2005) | 231 cars from the original Xbox game, read from its ISO or zip (paint, glass and lamps work; suspension, aero and brakes borrow a similar car's data; no engine sounds) |

## Online

Pick **ONLINE** in the main menu. The official server and every public community server are listed with their map,
player count and ping, or use **Direct connect** with `host:port`. A server holds up to 24 players on one map.

Want to host? The server needs no game files and no GPU: see [`server/README.md`](server/README.md) (Ubuntu or
Windows, systemd units included).

## Controls

| | Keyboard | Gamepad |
|---|---|---|
| Throttle / brake | W / S | RT / LT |
| Steer | A / D | Left stick |
| Handbrake | Space | A |
| Shift up / down (manual) | E / Q | B / X |
| Clutch | Left Shift | LB |
| Rewind (hold) | X | Back |
| Reset car | R | Y |
| Camera | C | RB |
| Photo mode | F | |
| Radio station | , / . | D-pad left / right |
| Pause | Esc | Start |

## Goals

Long-term goals, so the direction is visible rather than implied. **None of these are implemented**; the list is a plan, and
progress is recorded in [`MODLOG.md`](MODLOG.md). The detailed version, with each goal's design, decisions and open questions,
is a local working document (`docs/GOALS.md`, not committed).

Order is the intended work order. The `G` numbers are stable ids, not a sequence.

| order | | Goal | In one line |
|---|---|---|---|
| 1 | **G6** | Visual parity with the 360-era games | FM2, FM3, FM4, FH1 and FH2 look like the originals, measured against frozen reference pictures rather than judged by eye |
| 2 | **G1** | Completely modular game features | Swap any part of a Forza game for another's: look, HUD, menus, physics, cameras, audio, AI, rules, and so on, independently |
| 3 | **S6** | Performance index, per game | One backend rating per car, shown in each game's own class ladder and number |
| 4 | **G2** | Complete car and track list | Every car and circuit from FM1 through FH6 in three stages, optionally limited to a game's era (e.g. a 2008-cutoff FM2 roster) |
| 5 | **G3** | Best car model per car | Pick the better of each car's versions by measurement, not by eye, with every version still available |
| 6 | **G4** | Enhanced customisation | Body kits, wheels, interiors, exhausts, lighting, stance, engine swaps and era-correct aero, using the games' own parts |
| 7 | **G5** | Optimisation | 720p at 60 fps on Snapdragon 8 Gen 2 handhelds, and as small a footprint as possible |

### G6 visual parity: what works and what does not

Stated as a measurement rather than an impression. Twelve FM2 circuits, 1280×720, fixed poses, compared
whole-frame against Xenia captures. The instrument is `data/work/frame_diff.py`; the history is in
[`MODLOG.md`](MODLOG.md), and the decision below is `docs/DESIGN.md` § "Phase H".

**What works.** FM2 circuits import, render and drive: geometry, textures, sky, lightmaps, cars, HUD and the
game's own shadow rule on car paint. The frames are the right brightness — road, sky and car within ~1.0× the
reference — and the per-vertex car lighting is now the game's own (MODLOG 314, measured on 12 of 12 tracks).

**What does not.** The frame differs from the original by **30.5 mean absolute levels out of 255**, with
**1.36 %** of pixels within ±2. That is a visible difference, not a subtle one. It is also uniform: sky 29.25,
road 30.55, scenery 30.14, car 30.18.

**The target this measurement supports** is not the 2.0 the plan originally set; that would require removing
93 % of what remains, from mechanisms measured as unable to do it. The accepted ceiling for the current
approach is **~30.5**, and three specific items are open with their unblocking conditions written down (car
paint light units, the road's per-pixel detail, and the sky's per-track exposure). See `docs/DESIGN.md`
"Phase H" for the table and the reasoning.

## How it works

FH1 Rewrite is a new engine, not an emulator. The setup tool reads the game's own archives, models, textures,
database and audio from your disc and converts them into formats a modern engine can stream. Gameplay is then
rebuilt system by system from reverse-engineering research: the tyre model, drivetrain, brakes and steering are
checked against the original game running in a static recompilation, and the menus and HUD follow the original
layouts.

| Crate | Purpose |
|---|---|
| `fh1-formats` | Parsers: Forza zips (XMemCompress LZX), Xbox 360 textures, `.carbin` car models, scenery, PVS zones |
| `fh1setup` | Your disc → converted assets, per asset group, so updates only re-convert what changed |
| `fh1-launcher` | The installer and launcher (`FH1 Rewrite.exe`) |
| `fh1-engine` | The game: vehicle simulation, cameras, world streaming, races, AI, traffic, HUD and menus |
| `fh1-remaster` | The PBR renderer: static world on the GPU, shadows, sky, car paint, post |
| `fh1-shaders` / `fh1-render` | Xenos shader microcode → WGSL; shared render pieces (particles, sky, quality presets) |
| `fh1-world` | Track collision and surfaces |
| `fh1-audio` / `fh1-radio` | Engine sound from the game's FMOD banks, and the radio |
| `fh1-ui` | UI data: string tables, fonts, HUD scenes |
| `fh1-video` | FMV playback of the game's movies (intros, cutscenes) through an ffmpeg child process |
| `fh1-net` | Multiplayer protocol, game server and server list |

## Build from source

Requires Windows, Rust (stable, MSVC toolchain) and your own Forza Horizon disc.

```
cargo run --release -p fh1setup -- "path/to/Forza Horizon.iso"     # or an extracted disc folder
cargo run --release --no-default-features -p fh1-engine
```

`fh1-engine` compiles the Remaster RTX renderer (DLSS-RR) by default, which needs the NVIDIA DLSS SDK
(the `DLSS_SDK` environment variable), the Vulkan headers and libclang. Without those, build with
`--no-default-features` as above; the game then runs in its standard renderer. The `fh1setup` line keeps the
optional FH2/FM4 importers; drop to `--no-default-features` there for an FH1-only setup tool.

`fh1setup` also needs `ffmpeg` on `PATH` (or `FH1_FFMPEG` pointing at it): the `audio` group decodes the game's XMA
banks through it and setup stops there without it. The release ships a pinned build; source builds must supply their
own, and `fh1-engine` then uses the same ffmpeg for FMV playback.

The Xbox 360 retail XEX key is needed to read the shaders inside `default.xex`. Source builds look for it in
`data/xex_key.txt` or `FH1_XEX_KEY`; it is not part of this repository. Optional importers:

```
cargo run --release -p fh1setup --features fh2 -- import-fh2 "path/to/FH2.iso"
cargo run --release -p fh1setup --features fm4 -- import-fm4 "path/to/FM4 Play Disc.iso" --content "path/to/FM4 Content Install Disc.iso"
cargo run --release -p fh1setup --features fm3 -- import-fm3 "path/to/FM3 Play Disc.iso" --disc2 "path/to/FM3 Content Install Disc.iso"
cargo run --release -p fh1setup -- import-fm6 "path/to/FM6/Mount" --cars NIS_IDxNISMO_15
```

FM6 (Xbox One) cars, first one only (2026-10-30, MODLOG 97 to 110): the Nissan IDx Nismo imports from the game folder (`import-fm6`; textures,
models, materials, wheels sized by the game's own morph data, physics from FM6's database) and loads and drives in the engine
(`fh1-engine --car ../imported/fm6/cars/NIS_IDxNISMO_15`). Checked by numbers, not by eye: headless 0-60 mph 6.4 s (FM6's database: 6.0 s),
60-0 in 38 m, 1.06 g cornering; one game run with no errors. Not done: tyres are our own shape (the game has no tyre model file), several
looks are inferred, no normal maps / lamps / cockpit / damage, three detail textures missing, FM6's tyre-width grip rule not ported, the
launcher's setup does not offer FM6 yet. Other FM6 cars: not tried.

FM5 (Xbox One) cars, first one only (2026-10-30, MODLOG 113): the Lotus E21 imports with the same code (`fh1setup import-fm5 "path/to/FM5/Mount" --cars LOT_E21_13`)
and loads and drives (`--car ../imported/fm5/cars/LOT_E21_13`): headless 0-60 mph 3.1 s (FM5's database: 2.8 s), 60-0 in 21 m, 1.29 g cornering, weight
split at rest as in FM5's data (MODLOG 113, 114); one game run with no errors. Not looked at in the game.

FH2 (Xbox One) cars, first one only (2026-10-30, MODLOG 115): the Jaguar F-Type Coupe, a car the engine did not have, imports with the same code
(`fh1setup import-fh2xb1 "path/to/FH2/Mount" --cars JAG_FTypeCoupe_15`, into `imported/fh2xb1`, separate from the Xbox 360 FH2 import) and loads and drives
(`--car ../imported/fh2xb1/cars/JAG_FTypeCoupe_15`): headless 0-60 mph 4.3 s (database: 3.6 s), 60-0 in 28 m, 1.30 g cornering, weight split as in the data;
the Fast7 Nissan GT-R from the standalone FH2 Fast & Furious game (`NIS_GTRFast7_12`, same command with that game's Mount folder, MODLOG 116) imports too, weight split 55.0 % front as in the data, not yet seen in the game window;
one game run with no errors. Not looked at in the game.

FM2 circuits use FM2's own in-race HUD (LAPS, RACE / Lap timer, tach and boost gauge, minimap) and pause screen, drawn from the disc's UI
files (`fh1setup import-fm2 <disc> --only ui`); not done: the countdown lights, results pages, the main menus (see MODLOG 35 to 39).

FM2 circuits' 3D look (work in progress, not matched to the original yet): the car reflects the track's own car cube map at full resolution; Test Track's
car shadow follows the game's display-space shadow rule; opt-in switches `FH1_FM2_CAR_PRT=1` (the car lit by the game's own spherical-harmonic light and the
per-vertex transfer data of its model; better on some circuits, worse on others, so off by default) and `FH1_FM2_GLOW=1` (the sun's glow card). Details and
numbers: MODLOG 76 to 82, STATUS.md section 0b.

FM2 circuits' scenery (2026-10-29, checked by eye against twelve Xenia shots, `STATUS.md` 0c): spectators and crowds, the New York marquee, curb edges and road paint baked from the game's own pixel shaders, cut-out foliage drawn two-sided, grass tufts and sky brightness; two values are inferred from the shots, not game data (per-circuit sky gain, far leaf-alpha factor), both switchable.

Imported circuits (FM2, FM4) have a race against 7 of that game's cars, through the same race code as Colorado's events
(grid, countdown, laps, results): `FH1_RACE=0 fh1-engine --track fm2/lagunaseca_00` (`FH1_RACE_LAPS=n`, default 3). The AI follow the
game's own racing line and stay on the tarmac (`FH1_AI_ROAD=0` turns that off). Tested on FM2 Laguna Seca and FM4 Catalunya with a
non-driven player; a race driven by a person is not tested. Not in the menus yet.

`tools\release.ps1` builds the Windows release zip. Vendored crates keep their own licenses: `vendor/lzxd` (patched for
XMemCompress streams), `vendor/bevy_pbr` (shadow cascade caching, cached bind groups, static atmosphere LUTs),
`vendor/wgpu-core` (bind group tracking once per pass / command buffer) and `vendor/bevy_solarik`; each patched crate
lists its changes in `FH1_PATCHES.md`.

## AI usage

This project was built with AI coding tools, working from the game's own data files and from reverse-engineering
research against the original game. Every format, number and behaviour is checked against the real disc or the
original running game before it is called verified. AI did the typing; the goal is still to understand how Forza
Horizon actually works, and parity is measured rather than assumed.

## Contributing

Contributions are welcome: open an issue or a pull request. By contributing you agree that your work is licensed under
the GPL-3.0 like the rest of the project.

## License

Copyright (c) 2026 Sm1jjj and the FH1 Rewrite contributors.

FH1 Rewrite is licensed under the [GNU General Public License v3.0 only](LICENSE) (`GPL-3.0-only`). You may use,
study, modify and share it, but if you distribute it or anything built from it, you must release your full source
under the same licence and keep the copyright notices. It may not be taken closed-source. Vendored third-party code
keeps its own licence.

This is an unofficial fan project, not affiliated with or endorsed by Microsoft, Xbox Game Studios, Turn 10 Studios
or Playground Games. Forza Horizon and all game content, names and trademarks belong to their owners. This repository
contains no game assets, keys or decompiled code; you need your own copy of the game.
