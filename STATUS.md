# STATUS, 2026-11-02: FM2 visual parity — detailed handoff

> This section is the current, detailed state of the **FM2 visual-parity effort**. Everything below it is older material kept for its numbers and dead ends; where they disagree, this section is newer. The plan is `docs/DESIGN.md` (git-ignored); every measurement is in `MODLOG.md` entries 148–222.

## 1. The short version

**The goal is not met.** The plan's acceptance is a road-mask score of **≤ 2.0** (sum of |ln(ours/Xenia)| over the twelve circuits, each track within ±10 %). The current full run reads **5.08**, with only Sebring (0.95) at the target.

What *is* solid: the game's textures are proven to be exactly the disc's bytes, the console does no gamma decode, the colour-space order makes no difference, a real pipeline bug in the material tables was found and fixed, and one default improvement (the track lightmap colour) was adopted. What is missing: the per-draw material brightness — the right decoding/scaling of each road draw's colour layers and lightmap, which our importer approximates and the game's shader computes.

## 2. How it is measured

Three independent scores, all computed from screenshots our engine takes at the same pose as the Xenia references in `screenshots/xenia_ref/` (chase camera, Skyline R34, on the grid, race clock ~4–5 s):

- **Road mask (the plan's acceptance).** `data/work/parity.py`: selects "asphalt-like" pixels in both pictures, takes the median ours/Xenia and sums |ln| over the tracks. **Target ≤ 2.0, each track ≤ 0.10.**
- **Anchored (diagnostic).** `data/work/anchor.py`: for a dense screen grid, keeps only pixels where the engine's own ray probe returns a ground point, and compares ours vs Xenia at exactly those pixels. Two runs, median. **Not the acceptance**, but it is immune to the colour-threshold selection problem.
- **Ground-only (diagnostic).** `data/work/ground_metric.py`: the same, but drops pixels whose *reference* colour is a saturated/dark object (proven to be the car — see §5).

## 3. The numbers today

### Road mask (acceptance), `data/work/parity_rev.txt`
```
track              ratio   |  track            ratio
Tsukuba             2.14   |  Mugello           0.86
Laguna Seca         0.95   |  Suzuka            0.45
Road Atlanta        0.43   |  Sunset Infield    0.53
Silverstone GP      0.59   |  Maple Valley      0.43
Nordschleife        0.93   |  New York          (no mask)
Sebring             0.95 ✔ |  TT King Cobra     1.44
sum |ln| = 5.08 over 11 tracks   (target ≤ 2.0, per track ≤ 0.10)
sum |ln| over the 24 fixed boxes = 11.21
```

### Anchored (diagnostic, 100–460 verified ground pixels per track, two runs)
```
Tsukuba 1.54  Laguna 2.86  RoadAtlanta 1.63  Silverstone 0.59  Nordschleife 0.76
Sebring 1.42  Mugello 0.85  Suzuka 1.63  Sunset 0.69  Maple 2.75  NewYork 1.04  TT 0.92
sum |ln| = 5.23 over 12
```

### Ground-only (diagnostic; New York and TT have too few ground pixels)
```
Tsukuba 1.36  Laguna (16 px)  RoadAtlanta 0.41  Silverstone 0.49  Nordschleife 0.72
Sebring 0.70  Mugello 0.84  Suzuka 1.55  Sunset 0.49  Maple 2.24
sum |ln| = 5.80 over 10
```

**Reading them:** the open circuits (Road Atlanta, Silverstone, Sunset, Sebring, Nordschleife) render **too dark** (0.4–0.7); Maple, Suzuka and Tsukuba **too bright** (1.3–2.2). No single global rule moves both groups toward 1 — that has been measured five times over.

## 4. What is proven (keep these; do not re-litigate)

| fact | evidence |
|---|---|
| The console samples textures **raw**: no gamma decode anywhere. | 1,594,390 non-zero fetch-sign records at the Tsukuba grid, all `signs 55` on format 29; no `kGamma` in the session. MODLOG 175, 179 (the earlier title-screen readings were invalidated and redone at the verified grid). |
| **BC4** (the road grains) guest bytes are **byte-identical** to the disc payload. | `texraw`: 100.00 % of 131,072 bytes on one pair; correlation 1.0000, gamma fit 1.000. MODLOG 181. |
| **BC1** (the colour layers) is **value-identical**. | `texmatch`: correlation 1.0000, fitted exponent 1.000 on two Tsukuba BC1 discs. MODLOG 219 (218 was the wrong track and is corrected there). |
| The **colour-space order** is irrelevant: decoding to linear and lighting in linear with the post re-encoding is equivalent to staying in display space. | Raw upload + display pass-through measured identical to the default (0.39/0.49/2.24 vs 0.41/0.49/2.24). MODLOG 217. |
| The **remaster material table** must be rebuilt after every import, or every material keeps stale flags/roles and switch tests are meaningless. | `fh1setup <FH1 disc> --only remaster --force`; the file was 4 days stale while the textures were fresh. MODLOG 196. **This burned several days of experiments before it was found.** |
| The track's own **`TrackLightMapColor`**, which the engine never read, is applied and helps. | The cropped lightmap tile is scaled by `1 − TrackLightMapColor` from `TrackSettings.xml`. Adopted as a default: 5.23 → 5.06. MODLOG 191. |

## 5. What was measured and rejected (do not repeat)

| hypothesis | measurement | verdict |
|---|---|---|
| Family sRGB mask / game-word / per-texel bake / flat bake / 16-mask sweep | 5.21 / 5.61 / 5.33 / 4.96 / brackets but never lands | rejected, earlier (MODLOG ≤174) |
| **Mirrored lightmap crop** (`FM2_LM_MIRROR=1`) | anchored 10.88 vs 4.71; 9 of 12 tracks far darker | rejected; Maple's gain was a coincidence |
| **Shadow reach/coverage** (`FH1_RM_SHADOW_DIST=600`, `CASCADES=3`, `ATMOSPHERE=0`, `LM_LIGHT_FLOOR=0`) | frame changes (2.2–4.0 mean abs) but the measured ground pixels do not move | rejected |
| **Squared `ShadowColor`** as the shadow floor | mask 4.67, anchored 4.44, but whole frames darker (New York 4/23/30/48/84 vs 11/31/46/77/118) and it contradicts the game's documented `min(1−(1−ShadowColor)·S, lerp(dark,light,lm))` | rejected as a fit; kept off behind `FH1_FM2_SHADOW_SQ` |
| **Post** tuning (`FH1_FM2_POST_T=1`, `FH1_FM2_POST=0`, `FH1_FM2_RAMP=1`, `CONTRAST=1`) | 8.21, 5.35, 6.46, Tsukuba-only gain | rejected |
| **Bake the road draws from their own pixel shader** | Maple 2.24 → 5.22, Road Atlanta 0.41 → 1.39 | rejected (mixed) |
| **Bake stored linear** (`FM2_BAKE_LINEAR=1`) | Maple 5.22 → 2.12, Road Atlanta → 0.42, **Silverstone → 0.08** | the encoding is per texture, not global |
| **Per-format decode** (`FM2_DECODE_PER_FORMAT=1`: BC1/BC3 sRGB, BC4 linear) | 0.35 / 0.46 / 2.37 vs 0.41 / 0.49 / 2.24 | no change |
| **Force raw on both sides** (`FM2_BC4_SRGB=0` + linear upload) | anchored 6.56; most ground ratios 1.5–2.3 | rejected |
| **Plan v4: raw + display-space product** (`FM2_RAW_TEX=1` + `FH1_FM2_RAW_TEX=1`) | ground 0.32 / 0.03 / 0.08 | rejected (double conversion) |
| **Interpreter varyings**: real view-space `r2` (`FM2_VS_R2=1`), lightmap `r4` (`FM2_VS_LM=1`) | Maple 5.22 unchanged, Road Atlanta worse | rejected |
| **Forcing a single albedo candidate** (`FM2_FORCE_ALBEDO=0x114`) | Maple 2.51 → 0.19 | the truth is *between* the blend and the candidate |

## 6. What is still unknown

1. **The per-draw material brightness.** Maple model 111 renders with a two-layer blend whose layers average 0.4924 and 0.2356 through a mask of 0.5017 → 0.364 (sRGB). The reference implies **0.162**. The draw's own single candidate (276) is 0.219, and forcing it renders 0.19 (far too dark). No combination of the draw's own data lands on the reference.
2. **The draw's shader constants are known but not usable yet.** The probe now prints them (model 111: `c1 0.08, c2 0.05, c3 0.20, c4 0.60, c5 0.65, c6 0.50, c7 0.10`). The pixel shader uses them as per-layer affine transforms and then mixes light, specular and a distance blend — it is not a mean correction. Running the same shader in our CPU bake gives 5.22 for Maple, i.e. our evaluation disagrees with the game even with the two varyings we could identify fed in.
3. **The bake's internal sampling** (which mip, which gamma mask per sample, which register the light factor comes from) is the last unverified piece. Every input we can name from the disassembly has been supplied.

## 7. Tooling built for this (all in-tree, all untracked)

**Scripts (`data/work/`)**
- `parity.py <prefix>` — the acceptance harness: shoots all twelve, writes `parity_<prefix>.txt`.
- `anchor.py <prefix>` — anchored metric (probe grid, ground pixels, two runs). `--tracks=`, `--env K=V`.
- `ground_metric.py <prefix><run>` — ground-only split of an anchored run.
- `anchor_split.py <prefix> <track> <ref>` — per-surface split (asphalt / grass / white line / other).
- `dark_pixels.py <prefix> <track> <ref>` — crops the worst pixels to see what they are (this is how the car contamination was found).
- `track_corr.py` — per-track TrackSettings data joined with the ratios.
- `sky_warmth.py`, `depth_profile.py` — sky hue and row-band diagnostics.
- `xtr_scan.py`, `xtr_signs.py`, `xtr_mem.py`, `xtr_regs.py` — Xenia `.xtr` trace tooling, including a pure-Python snappy decoder.

**Rust probes (`crates/fh1setup/examples/`)**
- `texcmp` — disc DDS vs guest dump: stats, ratio, correlation.
- `texmatch` — ranks dumps against a disc by correlation (BC4/BC1/BC3).
- `texraw` — byte-level disc payload vs guest dump, all endian modes.
- `ddsppm` — DDS to PPM for scripting.

**Importer debug switches** (all opt-in; see `fm2_scenery.rs`)
`FM2_DUMP_MODEL=<n>` (any model's tail shaders), `FM2_PROBE_AT=x,y,z` (+ `FM2_PROBE_LERP`), `FM2_DEBUG`, `FM2_VS_R2`, `FM2_VS_LM`, `FM2_BAKE_SIGS`, `FM2_BAKE_LINEAR`, `FM2_FORCE_ALBEDO=<hex>`, `FM2_DECODE_PER_FORMAT`, `FM2_RAW_TEX`, `FM2_LM_TLMC`, `FM2_LM_MIRROR`, `FM2_ROAD_BLEND_IDS`.

**Engine switches** (all opt-in)
`FH1_FM2_RAW_TEX`, `FH1_FM2_SHADOW_SQ`, `FH1_RM_FM2_SHADOW_COL`, `FH1_RM_SHADOW_DIST`, `FH1_RM_CASCADES`, `FH1_RM_ATMOSPHERE`, `FH1_RM_LM_LIGHT_FLOOR`, `FH1_FM2_POST`, `FH1_FM2_POST_T`, `FH1_FM2_RAMP`, `FH1_FM2_CONTRAST`.

**Instrumented Xenia** (untracked, in `data/work/`)
`xenia-build` (source, commit `828014b9` + instrumentation: fetch-sign log, texture dump with metadata sidecar, F4 frame trace), built to `xenia-build-out-828`; storage root `tools/xenia-test` (release exe and config restored afterwards). Verified grid runs: `screenshots/xenia_ref/instr_828.png`, `instr_828b.png` (mean abs diff **0.4** vs the reference). Texture dumps: `data/work/texdump4` (962 textures + `dump_index.txt`).

## 8. How to reproduce a measurement

```powershell
# import (after any change) -- then ALWAYS rebuild the remaster tables:
.\target\debug\fh1setup.exe import-fm2 "data/work_fm2/disc" --only maps
.\target\debug\fh1setup.exe "data/work/disc" --only remaster --force

# the acceptance run (writes data/work/parity_<prefix>.txt)
python -I data/work/parity.py <prefix>

# the anchored diagnostic
python -I data/work/anchor.py <prefix>
python -I data/work/ground_metric.py <prefix>0

# one track's draw, with the game's own numbers:
$env:FM2_TRACKS="maplevalley"; $env:FM2_PROBE_AT="232.91,-88.33,708.08"
.\target\debug\fh1setup.exe import-fm2 "data/work_fm2/disc" --only maps
```

The band-centre world point of every track is in `data/work/road_keys.json`.

## 9. The decision left

1. **Accept** the measured state, with this document as the record.
2. **Calibrate per track** (a documented fitted gain per draw). It would very likely reach ≤ 2.0, but it is drawing the target, not reproducing the game — against the rule "use the game's data before guesswork".
3. **Continue deeper** into the bake's internal sampling (which mip / gamma mask / register feeds the light factor). This is the last technical unknown; it is research, not a switch, and its payoff is not guaranteed.

## 10. Housekeeping

- **Nothing is committed.** The working tree has the changes from this effort; `.gitignore` is still the whitelist.
- The user's Xenia install is untouched (release exe 17,381,376 bytes, `d3d12_bindless = true`).
- The engine and importer build green (`cargo build --release -p fh1-engine`, `cargo build -p fh1setup`).
- The twelve tracks are imported with the default settings and the remaster tables freshly built, so any future run starts from a consistent state.


---

# Older material (kept for its numbers and dead ends; where it disagrees with the section above, the section above is newer)

# STATUS, 2026-10-30 (end of day): the project, FM2 in detail, and where everything stands

> **2026-11-01 update: the reference-capture automation is built, but input into the emulator is the blocker.**
> Goal: an sRGB (`kernel_display_gamma_type = 1`) reference set of the **longest layout of each track family**, taken from the **grid** in System Link (Arcade starts with a rolling start, so it was dropped), in the isolated install `tools/xenia-test/`.
> Proven and usable: **capture with `PrintWindow`** while the window is unfocused and colour-faithful (means 51.52 screen vs 51.64 PrintWindow, half the pixels bit-identical, no offset); the **gamma A/B result** (the setting changes the reference by 28.9 levels of mean channel difference — see MODLOG 141); the **System Link path** (lobby, class, car, assists, grid start); the **47-entry lobby track list** and its mapping to our import ids (MODLOG 142); all state signatures (launch/quit dialogs, pause bar, race, lobby, tab bar, track block) measured and working in `tools/xenia-test/loop_tracks.ps1`.
> Blocker: **input**. With the d-pad bound to `Ctrl+letter`, a lost modifier or button key-up leaves the pad logically held; the game then ignores plain keys and only d-pad combos work, which produced the silent "Game Options did not open" / "launch dialog never appeared" failures and the car-select screen that would not accept `A`. After one restart, the emulator accepted **no input at all** (posted, real, with and without a key-release sweep); forcing window activation failed (`GetForegroundWindow` never became the game). See MODLOG 144 and 145.
> Two ways forward, both needing the user: install **ViGEmBus** (a virtual XInput pad is polled independently of window focus, so the background route works by design; capture already works unfocused), or run the **focused** loop with the machine left idle.
> Artifacts: **all 12 longest layouts are captured** at the grid in sRGB as `screenshots/xenia_ref/g1_<index>_<slug>.png` (Tsukuba, Laguna Seca, Road Atlanta, Silverstone GP, Nordschleife, Sebring, Mugello, Suzuka, Sunset Infield, Maple Valley, New York Circuit, TT King Cobra) — chase camera behind the car, Skyline R34, stationary on the grid, race clock 4-5 s (New York 11.9 s, same pose); verified by eye. `tsukuba_grid_gamma1.png` / `..._gamma2.png` remain valid. Nothing has been committed. The old `xenia2` set is neither the same camera nor the same pose, so it is not directly comparable with this one.


This is the current picture, written to be read cold. Everything under "XBOX ONE FAMILY REFERENCE" and "HISTORY AND EVIDENCE" further down is older, dated material, kept because it holds the numbers and the dead ends; where it disagrees with this part, this part is newer. Every step with its numbers is in `MODLOG.md` (entries 86 to 96: the FM2 scenery pass; 97 to 118: the Xbox One imports; 119 to 136: today's hunt for game data behind the inferred FM2 values). The plan for that hunt is in `docs/DESIGN.md` (git-ignored).

Words used here: **engine** = the game program (`fh1-engine`); **importer** = `fh1setup`, which turns a game's files into files the engine reads; **Xenia** = the Xbox 360 emulator the user runs the original games in, used only as a picture reference (twelve screenshots `screenshots/xenia2/x1..x12.png`, taken with Windows PrtScn); **inferred** = a value or rule fitted to those pictures or worked out from behaviour, NOT read from the game's own files or code; **road-band score** = our tool `data/work/cmp12.py`: for twelve circuits it compares the brightness of two road patches against Xenia's (sum of |ln ratio| over 24 patches; 0 = perfect, lower is better); **cut-out** = a flat card with see-through parts (leaves, fences, grass); **baked** = computed once at import time and stored as a texture; **probe** = a small command-line tool that prints numbers.

## 1. What this project is

A Rust / Bevy rewrite of the Xbox 360 era Forza games. It plays Forza Horizon 1 from the user's own disc and imports content from other Forza games the user owns: FH2 (Xbox 360 and Xbox One), FM4, FM3, FM2 (circuits and cars), FM1's cars, and cars from FM6, FM5 and FH2 for Xbox One. It reads the games' own data and rebuilds the game systems from it; it is not an emulator.

Hard rules (`AGENTS.md`, git-ignored): no game files or extracted data in the repository (converted data lives in git-ignored `data/installations/<id>/assets/private/`, scratch work in git-ignored `data/work/` and `data/work_fm2/`); `.gitignore` is a whitelist; game installs and the user's Xenia folders are read only; single-player and offline only; prefer the game's own data to guesses; the assistant cannot see the game, so it checks by numbers and logs and says "not tested" when something is not; commit only when asked; after two real failed attempts on a problem, stop and write it up.

| Crate | Job |
|---|---|
| `fh1setup` | The importer: `import-fm2` (circuits, cars, spectators, cubes, UI), `import-fm4`, `import-fm3`, `import-fh2`, `import-fm1`, `import-fm6`, `import-fm5`, `import-fh2xb1` |
| `fh1-formats` | File parsers (zips, Xbox 360 textures and models, FM2 scenery / PVS / `.crowd` / skinned spectators / animations, and the Xbox One family: `grub`, `swatch`, `modelbin`, `materialbin`, `tyre`) and many probes in `examples/` |
| `fh1-engine` | The game: physics, cameras, streaming, races, AI, HUD, crowds |
| `fh1-remaster`, `fh1-render`, `fh1-shaders` | The PBR renderer (materials, shadows, sky, car paint, the FM2 final combine), shared render pieces, and the Xbox 360 shader disassembler / translator / CPU interpreter |
| `fh1-world`, `fh1-audio`, `fh1-radio`, `fh1-video`, `fh1-ui`, `fh1-net`, `fh1-launcher` | Collision, sound, radio, movies, UI, online, launcher |

Data flow: game files -> `fh1setup` -> `data/installations/<id>/assets/private/...` (imported games under `imported/<game>/`) -> `fh1-engine`. The engine mirrors z against the game's files; every importer-to-engine path flips it.

## 2. Git state (end of day)

Branch `ui-elements`, ahead of `main`, not pushed. Commits of today, oldest first (all on top of `3989c1c` "FM2 UI docs"):

| Commit | What |
|---|---|
| `6547243` | formats: Xbox One family parsers and probes |
| `8782528` | setup: `import-fm6` / `import-fm5` / `import-fh2xb1` |
| `d6dc1a7` | engine: FM6 / FM5 / FH2 (Xbox One) in the optional-game list, `drive_probe SETTLE=1` |
| `56bf8a4` | FM2 import: crowds, cut-out and blend-shader bakes, sky, car light data, the LED rule |
| `8d9edd6` | FM2 render: final combine, car paint, per-vertex car light, opt-in alpha-to-coverage |
| `cce1329` | FM2 engine: crowds with per-figure tint, FM2 defaults, sky, camera, HUD |
| `eddf4e5` | docs: MODLOG, README, STATUS |

Uncommitted at the time of writing: `crates/fh1-remaster/src/post.rs` (the opt-in `FH1_FM2_RAMP` switch, section 6), `MODLOG.md` (entries 134 to 136) and this file. The untracked file `1` at the repository root (a 755 KB list of numbers, origin unknown, there before these sessions) is deliberately left alone and not committed. Only the final tree was built as a whole; the importer was also built alone at the Xbox One step; the other intermediate commits were not individually build-tested. `core.safecrlf` was set to `false` in this repository (git warns that LF will become CRLF on several files); `fm4.rs` and `main.rs` in `fh1setup` were converted to LF.

## 3. State at a glance

| Area | State | Evidence |
|---|---|---|
| Forza Horizon 1 (Colorado) | Works per README; only regression-checked lately | MODLOG 86, 114 |
| FM2 circuits, 3D scene vs Xenia | Close on all twelve reference shots, not identical; road-band score 6.54; global brightness and several inferred values remain | section 4, MODLOG 86 to 96, 119 to 136 |
| FM2 HUD, minimap, pause | Done and committed (commits `0f62149` to `3989c1c`) | history section 0 |
| FM2 cars | Imported and raced by the AI; drawn through the glTF car path with fitted paint knobs; body paint brighter than Xenia's | section 4 |
| Races on imported circuits (FM2 / FM4) | Work through the dev hook `FH1_RACE=0`; never driven by a person; not in the menus | history section 6 |
| FM1 / FM3 / FM4 / FH2 (Xbox 360) | Imported earlier; not re-checked | README |
| Xbox One cars: FM6 IDx, FM5 E21 | Import, load, drive; the user has driven them ("very rough but satisfy me for now") | XBOX ONE REFERENCE below |
| Xbox One cars: FH2 F-Type, Fast7 GT-R, Rocket 69, FH2 R34 | Import, load, drive; the user has seen all four in game ("very rough but satisfy me for now") | same |
| Launcher / setup UI | Does not offer FM5, FM6 or FH2 Xbox One (command line only) | |

## 4. FM2 in detail

### 4.1 What works (all committed)
- **Circuits:** 32 layouts imported (`import-fm2`); twelve compared against Xenia shots (Tsukuba, Test Track, Sunset Infield, Silverstone, Sebring, Road Atlanta, Nurburgring, New York, Mugello, Maple Valley, Laguna Seca, Suzuka).
- **Spectators:** 358,757 placed from the disc's `.crowd` files (`fm2_crowd.rs`); far sprite cards plus near 3D skinned figures drawn on the GPU (`crowd.rs`, `crowd/figures_gpu.rs`, `crowd_skin.wgsl`, `crowd_skin_frag.wgsl`). Near figures are tinted from the map's `<Crowds>` colours and each spectator's light byte (section 4.4, value 3).
- **Scenery fixes found in the game's data:** the New York marquee (cut-out rule; LED-board pixel shader family), Silverstone's curb outline (terrain-blend pixel shaders run on the CPU and baked, 90 of 90 draws; 232 on Road Atlanta), Nurburgring's trees (alpha-only texture rule, two-sided cut-outs, a distance alpha rule), Road Atlanta's grass tufts, the sky (sector photos, zenith cap, runtime textures from `.stx.bin`, sun glow card opt-in), road textures (degenerate-uv rule, Nurburgring grain).
- **Final combine:** the game's own `FinalCombineBloomTonemap` (desaturate toward luminance, tint, a luminance curve blending a dark and a light parameter set from `TrackSettings.xml`) in `post.rs`, working on gamma-coded values.
- **Cars:** imported (`fm2_cars.rs`), raced by the AI; the car light block (spherical-harmonic light with 16 per-vertex transfer numbers) is rebuilt on the CPU (`car_prt.rs`, opt-in `FH1_FM2_CAR_PRT=1`); the car paint reflects the track's own car cube map (256 x 256 x 6).
- **HUD, minimap, pause screen** from the disc's own UI scenes.

### 4.2 Still different from Xenia (not fixed)
- **Global brightness:** luminance percentiles (5 / 25 / 50 / 75 / 95 %, mean of twelve): Xenia 11 / 48 / 86 / 139 / 206, ours 31 / 72 / 102 / 141 / 187: ours is about 20 too bright in the darks and about 19 too dim in the highlights.
- **Car body:** Xenia's Skyline is near-black navy with bright sky reflections on about nine tracks; ours is bright blue with weak reflections. The body shader's own shadow term is decoded (4.5) but its inputs are not found.
- **Shadows:** weaker and softer than Xenia's; only Test Track has the display-space shadow fix; the game's dynamic sun direction is unknown.
- **Open smaller items:** Tsukuba's big clouds (in no readable sky file), Mugello's sun disc (sky draw 0 not drawn), the speckled start marker, Nurburgring's road gloss, the fainter pylon, Nurburgring's denser and darker canopies.
- **Road brightness outliers** (ours / Xenia in linear green, left | right band, default): Test Track 0.72 | 0.89 in the latest run, Nurburgring 1.30 | 0.21, Road Atlanta 1.55 | 0.98, Maple Valley 0.56 | 1.21, New York 0.58 | 0.52 (right bands are often wall or shadow, not clean road).

### 4.3 The reference: how the Xenia pictures were made
- Windows PrtScn of a 1280 x 720 Xenia window (crop `(1,51)-(1281,771)` in our comparisons), not Xenia's own screenshot key.
- Config (`D:\Games\Emulators\xeniamanager\Emulators\Xenia Canary\config\Forza Motorsport 2.config.toml`, byte-identical to the global `xenia-canary.config.toml`, both last written 2026-10-30 16:20, so the 2026-10-26/27 session's settings cannot be told from the dates): resolution scale 1x, no post-AA, bilinear 1:1, dither on, `render_target_path_d3d12 = "any"` (host render-target path "RTV", documented as lower accuracy), `gamma_render_target_as_unorm16 = true`, `readback_resolve = "full"`, `kernel_display_gamma_type = 2` (BT.709). Latest log: title Forza Motorsport 2 (4D5307EA), AMD Radeon RX 7800 XT, Direct3D 12, no game patches. The config notes the exact `rov` path crashes shader compilation on AMD D3D12 drivers (not tried).
- **Game-data fact found today (MODLOG 135):** FM2's own Direct3D code (`default.xex`, `0x82377258` / `0x82377400`) calls `VdGetCurrentDisplayGamma` and builds its 1024-entry output gamma ramp from the type reported: 1 sRGB, 2 BT.709 (constants read from the image: 4.5, 0.018, 0.45, 1.099, 0.099), 3 power. With Xenia's type 2 the screenshots therefore end in the BT.709 OETF, not sRGB.
- Test: our percentiles read as sRGB light and re-encoded with that ramp give 16 / 57 / 88 / 129 / 179 (rms to Xenia 13.4, from 17.9 for ours as is): darks and mids come close, highlights go the wrong way. As a switch in the combine (`FH1_FM2_RAMP`, section 6) the road-band score got WORSE (16.78 and 26.46 against 6.54), because every fitted value was tuned with an sRGB output (MODLOG 136). Not adopted.

### 4.4 The inferred values (fitted to the Xenia shots, not read from the game)
Each has a switch. Today's search for game data behind each one, in plan order (`docs/DESIGN.md`):

| # | Value | Where | What the search found | State |
|---|---|---|---|---|
| 1 | LED gain 1.3 (New York marquee, dome lamps) | `fh1setup/src/remaster.rs` | The LED pixel shader uses NO lightmap: `light = sat(N.L) * (1,1,1) + LightAmbient`; New York `LightAmbient` (0.30, 0.27, 0.25) so a board facing the sun gets 1.27 (MODLOG 119, 120) | **Replaced**: gain = `1 + mean(LightAmbient)` per map (`FH1_FM2_LED_GAIN=1.3` restores the old value). Per-draw N.L not modelled; not seen in the game window |
| 2 | Lightmap light floor 0.06 (`FH1_RM_LM_LIGHT_FLOOR`) | `main.rs`, `material.rs` | The game's lightmap shaders (families 9958d0e45436df7b, 93a3b0c2c3ccd51e) compute `2 * lerp(DarkColor, LightColor, lightmap) * texture` with NO floor; the shader that draws New York's NISSAN banner (why 0 turns it blue) is not identified (MODLOG 121) | Still inferred; stays |
| 3 | Near-spectator grey 0.75 | `crowd.rs`, `figures_gpu.rs` | `TrackSettings.xml` `<Crowds dark_.. light_..>` plus the `.crowd` record byte at offset 26 (0..255, 158 distinct values): hypothesis `lerp(dark, light, byte/255)`; New York mean 0.79 (MODLOG 122, 124, 125) | **Replaced** by per-map and per-figure tint (`FH1_CROWD_TINT=0` = flat 0.75). Byte meaning UNVERIFIED; the Xenia check (MODLOG 133) was inconclusive (the compared pixels do not line up). Far cards unchanged |
| 4 | Far cut-out cutoff factor 0.3 (`FH1_RM_CUTOUT_FAR`) | `material.rs` | The game draws with alpha-to-coverage (render state `0x150`, on about 39,000 of Nurburgring's 44,000 draws) and its own cull state (`0x38`) (MODLOG 122); swapping A2C in (`FH1_RM_CUTOUT_A2C=1`, needs `FH1_AA=msaa4`) was neutral to slightly worse on four circuits (MODLOG 123) | Still inferred; A2C stays opt-in |
| 5 | Road wear darkening 0.2, Nurburgring grain gain 0.78 | `fm2_scenery.rs` | Both are bake-level stand-ins for the road pixel shader (family 4aa0eec057a0743d); its own output on real inputs was 3.5 times brighter than Xenia's (history, 2026-10-19) (MODLOG 131) | Still inferred |
| 6 | Car paint / glass / lamp knobs (`FH1_RM_CAR_FILL` 0.1, `_LAMP` 250, `_GLASS_ALPHA` 1.5, `_PAINT_ALBEDO` 0.7, `_REFLECTANCE` 0.2, `_CLEARCOAT` 0.3, `_TAIL_FLOOR`) | `main.rs` | The game's own per-technique car settings are on the disc (`Media/cars/ShaderSettings/Normal.xml`, `GlobalCarAttribs.xml`: body ColorScale 0.6 / ColorClamp 0.9, Fresnel, CubeMultiplier 12 / 6 / 4 / 2.2, glass colours); FM2 cars use our glTF path, so they have no one-to-one counterpart (MODLOG 126, 132) | Still inferred; glass colour / alpha and cube multipliers could be driven from the file (not done) |
| 7 | Test Track shadow gamma 2.2 (`FH1_RM_FM2_SHADOW_GAMMA`) | `main.rs` | No new data. Unread lead: Test Track `StaticShadowColor` 0.36 against 0.65 on most circuits | Still inferred |
| 8 | Per-circuit tone-curve blend, sky gain table, saturation x1.5 (`FH1_FM2_SAT`) | `main.rs`, `fm2_sky.rs` | Blocked on the lightmap x2 (4.5); the saturation guess does not come from `DesaturateAmount` (0.83 to 0.95); Tsukuba's `DesatPostMod` alpha is the debug fill 0xCDCDCDCD | Still inferred |

### 4.5 The lightmap x2 puzzle and the car body shadow (two stopped investigations)
**Lightmap x2.** The FM2 lightmap pixel shaders add the lerp to itself (`2 * lerp(...) * texture`); our engine omits the 2 and matches the roads. Tested, all on the twelve circuits (road-band score, default 6.54): doubling the lightmap light in linear space 13.58 (roads 1.3 to 2.4 times too bright, `x2dbl`); the game-style product in gamma-coded space alone 6.67 (neutral, `gmonly`); the x2 in coded space (x4.6 linear) 28.40 (`gm2x`). Read: the final combine shader has no 0.5 (MODLOG 127); the `.stx.bin` texture descriptors carry no exponent or gain field and the texture header builder (`0x823c5e00`) is the console's standard one (MODLOG 128); the D3D resolve routine (`0x8237d158..0x8237df4c`) was located but no exponent bias isolated (MODLOG 130). Four approaches, no source for the halving; the only untried idea is the resolve's exponent-bias field, found by tracing the caller arguments, or a Xenia GPU trace (rejected earlier for road brightness). This blocks values 7 and 8.

**Car body shadow (STATUS note kept from 12b).** The body pixel shader's self-shadow (switch `doselfshadow`, b128): a two-value texture `tf0` read as a variance shadow map's mean and mean-of-squares; receiver depth `min(dot(c_shadowParams, vec4), 1)`; lit if depth minus mean is not positive, else a Chebyshev-style test (variance over variance plus squared difference), then `factor = c_shadowColour + (1 - c_shadowColour) * lit`; the final colour is the WHOLE lit colour times `factor`, which is why Xenia's cars go near black in shadow (MODLOG 126). Not found: what fills `c_shadowParams` / `c_shadowColour` / `doselfshadow` (the effect object keeps the handles at `0x2cc` / `0x2d0` / about `0x2d8`; grep for reads of those offsets only returned unrelated fields). Untried: the object's own methods that call SetValue with those handles, or a Xenia GPU capture.

## 5. What changed in code today (all committed except the ramp switch)

New switches (all default OFF or equal to the old behaviour unless noted):
- `FH1_FM2_LED_GAIN=<n>` (importer remaster step): the LED gain; default is now `1 + mean(LightAmbient)` of the map.
- `FH1_RM_CUTOUT_A2C=1` (+ `FH1_AA=msaa4`): FM2 cut-outs use alpha-to-coverage. Off.
- `FH1_CROWD_TINT=0`: no spectator tint (flat 0.75). Default on, but the installed data must carry the new crowd files (`import-fm2 ... --only crowd`, already re-run on the user's install).
- `FM2_PS_LIST=1` (importer): one line per draw with its pixel-shader signature and which light constants it reads.
- `FH1_FM2_RAMP=1|2` (`post.rs`, UNCOMMITTED): the BT.709 output ramp in the FM2 combine. Off.

New probes: `fm2_state_track` (render state per draw across a model), `stx_desc` (raw `.stx.bin` descriptors) in `crates/fh1-formats/examples`.

Data written by the importer: the crowd record's 4th byte (was 0) now holds the spectator's light byte; `index.json` has `tint {dark, light, mean}`; `fx_globals.json` has `LightAmbient`.

## 6. Not tested
- The LED change and the crowd tint in the game window (only logs and pixel counts: New York runs with no shader error, 3.34 % of the frame's pixels change with the tint, mean luma 53.51 against 53.32).
- That the spectator byte is a sun share (hypothesis); the far sprite cards; the other circuits' crowds (Sebring's tint is 1.81, above 1).
- A full lap or race on the FM2 scenery by a person; frame cost beyond one HUD reading per circuit; the 16 MB GPU matrix buffer past 16 MB of clips; Linux builds.
- Alpha-to-coverage frame cost and its look (only crude top-of-frame numbers; it cannot be judged by eye here).
- The BT.709 ramp with re-fitted values (the fit would be a fitting exercise, not game data).
- Intermediate commits of today as individual builds.

## 7. How to check it (Windows, PowerShell)

Build the engine (without the DLSS SDK) and the importer:
```powershell
cargo build --release -p fh1-engine --no-default-features
cargo build -p fh1setup
```
Re-import (the crowd step only writes the per-map `crowd/` folders; a maps import deletes each map's folder first, then the remaster step rebuilds the material tables):
```powershell
./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only crowd
./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps
./target/debug/fh1setup.exe data/work/disc --only remaster --force
```
Run a circuit and see the crowd tint in the log:
```powershell
$env:FH1_OWNERSHIP="0"; $env:FH1_CROWD_LOG="1"; ./target/release/fh1-engine.exe --track fm2/newyork_01
```
Expect a `crowd: figure tint [0.793, 0.793, 0.789]` line and a crowd of varying brightness along the walls (`$env:FH1_CROWD_TINT="0"` makes them one flat grey). Try the opt-in switches by setting them in the same way (`FH1_RM_CUTOUT_A2C=1` with `FH1_AA=msaa4`; `FH1_FM2_RAMP=1`).

Scoring tools (git-ignored, in `data/work`, bash): `bash data/work/shoot_some.sh <prefix> <tracks...>` writes `screenshots/<prefix>_<track>.png` (needs `FH1_OWNERSHIP=0`; a reused prefix returns at once with the old pictures because `/tmp/shoot_<prefix>.done` is left behind); `python data/work/cmp12.py <baseline prefix> <new prefix>` prints the road-band score (needs all twelve tracks shot under both prefixes). Shots of today: `x2base` (default, 6.54), `x2dbl`, `gmonly`, `gm2x`, `rmp1`, `rmp2`, `a2cbase` / `a2con`, `pftint` / `pftint0`. Python helpers live in `data/work/pylib` (PIL); run with `python -I`, and never name a script after a standard module (`dis.py`, `bisect.py` broke imports before).

Game-code reading tools: `data/work/fm2_xex/image.bin` and `text.asm` (disassembly of FM2's `default.xex`), `data/work/ppc_xref.py` (finds code that references a string); `fm2_fxdump <default.xex> <technique>` and `fm2_fxlist` (embedded effects) in `fh1-shaders/examples`.

## 8. Mistakes recorded (so they are not repeated)
- Left a stray `python -` command waiting on input in the background for 30 minutes (a mistyped one-liner); it blocked the goal check until stopped with TaskStop. Never pipe into an interpreter without a script.
- Split a mixed file by line-number-only partial patches (`git apply --unidiff-zero`), which scrambled `fm4.rs` in a commit; undone before pushing and redone by patching a copy and hashing it into the index. Check `git diff --cached` after any partial staging.
- Reported the goal as "done" language too early at times; the stop hook was right that it was not met. State the state, not the hope.
- Read the first display-gamma test the wrong way round (decode with BT.709 and re-encode with sRGB gave 28.2); the right direction (encode our light with the BT.709 ramp) gave 13.4. Check the direction of a transfer curve before drawing a conclusion.
- Compared crowd pixels against Xenia at the same screen positions although Xenia's crowd and camera need not line up: an invalid test, recorded as inconclusive (MODLOG 133).
- Older, still true: measured crowd distances from the engine's placeholder camera instead of the real spawn; wrote files with shell heredocs and lost backslashes (use the editor tool); named a helper `bisect.py` / `dis.py`; called a car "not in the install" after a file-name search (search the database and string tables); gave a `cargo run` command without `--bin fh1-engine`; blamed FM5's suspension data for a body contact point resting on the ground.

## 9. What to do next (pick one)
1. **User:** re-take two reference shots in Xenia (New York and Tsukuba, same start position) once as-is and once with `kernel_display_gamma_type = 1`; if the pictures differ, the reference depends on that setting and the inferred values should be re-fitted to a known reference (the best lead for values 2, 5, 7 and 8).
2. Read Test Track's `StaticShadowColor` (0.36 against 0.65 elsewhere) against the shadow-gamma fit (value 7); not yet looked at.
3. Drive the FM2 car glass colour / alpha and the per-technique cube multipliers from `ShaderSettings/Normal.xml` (value 6; small, data based, needs the user's eyes).
4. Re-fit the tone blend / saturation / sky gain under the BT.709 ramp (a fitting exercise; only worth it if step 1 shows the gamma setting matters).
5. Untried game-code leads: the D3D resolve's exponent bias (lightmap x2), the callers that set the car effect's `c_shadowParams` / `c_shadowColour`.
6. Commit the uncommitted pieces (the ramp switch, MODLOG 134 to 136, this file) when asked.
7. Import more FH2 Xbox One cars; normal maps and headlights for the Xbox One cars (XBOX ONE REFERENCE below).

---

# XBOX ONE FAMILY REFERENCE (kept from the 2026-10-30 status; the cars have since been seen in game by the user)

Note on this part: where it says a car is "not tested in the game window" or "seen by the user: no", the user has since seen the four newer cars (FH2 F-Type, Fast7 GT-R, Rocket 69, FH2 R34) in the game and said they are "very rough but satisfy me for now". Open Xbox One items (normal maps, headlights, the other electric cars, more FH2 Xbox One cars) are unchanged.

## 3. The Xbox One family: what is decoded, and how sure

FM6 (Xbox One) was decoded first on the Nissan IDx (proof of concept, user 2026-10-30: "leave fm6 for now"); FM5 and FH2 for Xbox One turned out to be the same family with older versions. Each line says how it was checked.

| Piece | What it is | Checked by |
|---|---|---|
| Container `burG` (`grub.rs`) | Header + table of chunks (`Mesh`, `VerB`, `IndB`, `VLay`, `MatI`, `Skel`, `Mrph`, `MBuf`, `TXCB`...). Version 0x0101 (FM6): count u32 at 16, table at 0x14. Version 1 (FM5, FH2 Xbox One, FM6's generic wheel): count u16 at 6, table at 0x10 | Every file of every car passes the layout rules: IDx 159 / 159, E21 88 / 88, F-Type 213 / 213 |
| Textures `.swatchbin` (`swatch.rs`) | Header (size, mips, 4 flag bytes); formats from the flags: BC1, BC4, BC5, BC7, 8888. Pixels in AMD GCN tiling as in Microsoft's `xg_x.dll`: micro tiles of 8x8 blocks with Morton order inside; mips under 64x32 blocks in rows of tiles; larger mips in macro tiles of 64x32 blocks with pipe / bank XOR bits (`p0 = x3^y3^x4, p1 = x4^y4, p2 = x5^y5, b0 = y4^y5^x6, b1 = y3^x7`). Small compressed textures: mip 0 is macro-tiled even below one macro tile | In-tile order read from `xg_x.dll`'s code (capstone); the rest fitted from the data and checked by "each mip shrunk 2x equals the next" (mismatch 0.06-1.7 against 5-113 for wrong layouts) and by clean pictures (Nissan badge, interior atlas, livery, grille patterns) |
| Models `.modelbin` (`modelbin.rs`) | Shared position stream + per-range attribute streams, vertex layouts with DXGI formats, mesh records (material, bone, LOD, index range, base vertex, bindings, five UV rects). FM6: packed 16-bit positions with scale / offset, normal split between position.w and two values. FM5 / FH2 Xbox One: float positions, half-float normals | Decoded box equals the stored box on every LOD0 mesh (largest difference 0.0000 m, all three cars); normals agree with triangles at 0.978-0.984 |
| Skeleton and placement | `Skel` bones (29 on the IDx); a mesh's u16 at +2 is its bone; shared-library parts (wheels, brakes, arms) placed by name at the wheel locators (`Locators.xml`) | Doors / glass / wipers / gauges land in plausible places; wheels match the body's wheel arches (IDx: front arch centre z +1.21, locator +1.240) |
| Morphs (`Mrph`, `MBuf`) | The wheel model has two targets: rim radius (x 1.3182 of the distance from the axle per unit weight) and barrel width (inner edge moves -0.9) | Exact fits (spread +-0.0003, error 0.0000); rims then come out at the database's rim radius |
| Materials (`materialbin.rs`) | Instance -> parent chain through the shared libraries (`media/cars/_library`, `media/_library`); shader name; parameters (types 0, 1, 2, 3, 4, 6, 11; texture path lengths are 7-bit variable-length numbers; FM5-version entries have no 16-byte GUID) | All 310 IDx material instances resolve; 3,145 of 3,151 library material files parse to the end |
| Physics | FH1's own builder (`cars::physics_for`) on each game's database (FM6 `media/db/GameDB.slt`, FM5 `media/db/gamedb.slt`, FH2 Xbox One `media/stripped/gamedbRC.slt`); every table and column it reads exists in all three | Cars load and drive (section 4) |

## 4. The three cars, by numbers

All from `drive_probe` (headless, release build): `SETTLE=1 FLAT=1` = wheel loads after 5 s at rest; `CHECKS=1 FLAT=1` = launch, 60-0 stop, full lock at 40 mph. FH1's own NIS_GTR_12 for comparison: settled 53.0 % front (data 53.0 %), 0-60 3.63 s, 60-0 27.2 m, 1.17 g.

| | FM6 Nissan IDx Nismo | FM5 Lotus E21 | FH2 XB1 Jaguar F-Type Coupe |
|---|---|---|---|
| Command | `import-fm6 ... --cars NIS_IDxNISMO_15` | `import-fm5 ... --cars LOT_E21_13` | `import-fh2xb1 ... --cars JAG_FTypeCoupe_15` |
| Output | `imported/fm6/cars/NIS_IDxNISMO_15` | `imported/fm5/cars/LOT_E21_13` | `imported/fh2xb1/cars/JAG_FTypeCoupe_15` |
| Model | 79 parts, 307 body meshes, 207,580 triangles, 25 textures | 30 parts, 184 body meshes, 243,863 triangles, 42 textures | 85 parts, 411 body meshes, 235,050 triangles, 30 textures |
| Tyres (database) | 225/40 R19 all round | 265/60 R13 front, 355/45 R13 rear | 255/35 R20 front, 295/30 R20 rear |
| Settled front share | 54.0 % (data 54.0 %) | 45.9 % (data 46.0 %) | 52.1 % (data 52.0 %) |
| 0-60 mph | 6.37 s (database 6.0 s) | 3.09 s (database 2.8 s) | 4.26 s (database 3.6 s) |
| 60-0 mph | 38.3 m, 0.90 g | 20.6 m, 1.58 g | 27.8 m, 1.27 g |
| Full lock, 40 mph | 1.06 g | 1.29 g | 1.30 g |
| Game run (log) | spawned, no errors | spawned, no errors | spawned, no errors |
| Seen by the user | yes | yes (driven) | no |

## 4b. Cars added after the first three (2026-10-30, later)

| Car | Command (importer / game folder) | Headless result | Notes |
|---|---|---|---|
| FH2 FnF standalone: Fast7 Nissan GT-R `NIS_GTRFast7_12` | `import-fh2xb1 .../FH2FNF/Mount` | 55.0 % front (data 55.0 %); scale 1.0: 0-60 2.97 s, 60-0 43.8 m | One of only two cars exclusive to the standalone (other: `BUG_VeyronFast7_12`, not imported); 2 textures missing |
| FM6 Rocket 69 `FAL_Atom_77` (electric) | `import-fm6 .../FM6/Mount` | 48.1 % front (data 48.0 %); 0-60 6.27 s (database 5.0 s) | Needed the new electric path (fix 8). Found by decoding the string table, not by file name |
| FH2 Fast & Furious R34 `NIS_SkylineFF_99` | `import-fh2xb1 .../FH2/Mount` | 56.8 % front (data 57.0 %); 0-60 3.41 s, 60-0 45.0 m | Chosen by name only; headlight models skipped (72-byte layout); 4 textures missing |

None of these three has been seen in the game window. Run any with `--car ../imported/<fh2xb1|fm6>/cars/<name>`.

## 5. Fixes found by loading the cars (all in the importer unless said)

1. **Split tyre curve** (FM6, FM5, FH2 XB1): the databases split the tyre's forward grip curve into acceleration and braking; the engine has one. It gets the acceleration curve (inferred); the braking curve is kept as `friction_longitudinal_brake`. Symptom before: the engine refused to load the car (`missing number MinLoadCurve`).
2. **Zero tyre-width grip** (FM6 only): FM6 stores `TireFricScale0/1` as 0 and FH1 multiplies grip by them; zeros become the engine's neutral defaults. Symptom before: 0.02 g cornering, never reached 60 mph. FM6's own replacement rule (`DiameterAffectFriction*`) is not ported.
3. **Camera box** (FM6, and all three now): the engine's cameras measure from the car's box, which FM6's database has as zeros and none of these games ship as MAXData. The importer writes the whole car's box as the camera box. Symptom before (user): "the camera is way too far forward".
4. **Body contact box** (FM5 E21, all three now): the engine's fallback body contact points are placed from `PristineBoundingBox`; with the wheels inside that box a point rested on the ground and took weight off the E21's front tyres. The importer now writes a body-only box there. (An earlier written guess, "FM5's suspension data means something else", was wrong.)
5. **Hub height** (all three): FH1's own rule (tyre radius - the database's ride height) instead of the locators' height; the raked E21 otherwise stood on its rear wheels.
6. **Opacity masks and small textures**: cut-out decals, emblems and grilles (37 cut-out materials on the IDx); mip 0 of small compressed textures decoded. Symptom before (user): "several missing textures".
7. **Wheel motion-blur disc** dropped (the game draws it only at speed; it covered the spokes).
8. **Electric cars** (`cars.rs`, `car_physics`): a car with a stock `List_UpgradeMotor` row and no engine row (Rocket 69) now takes `Data_Motor` as its engine and the motor's torque curve as its camshaft's; before, the engine refused it ("torque samples"). INFERRED: 100 rpm sample spacing, stall / start 0. The 11 other electric FM6 cars (Tesla Model S, ten Formula E) are untried; no battery or motor upgrades.

## 6. Values and rules that are INFERRED (Xbox One family)

- Which material parameter is the base colour: `1f9b6488` (textured shaders), `938926b0` (simple shaders), `c0cb2820` (paint in a car's colour file). Parameter names are hashes that match no CRC-32 / FNV-1a of strings in the shaders; meanings come from values across the IDx's materials.
- Metal and glass looks from the shader's name; glass drawn as dark see-through (alpha 0.35).
- UV set 0 with the third of five rects, as offset + scale (checked only where that rect is the identity: the IDx's interior atlas, 0.0 % of triangles on empty background).
- Paint fallback when a car has no colour file: the database's first `Combo_Colors` row. It looks like a placeholder (the same value on the IDx and the E21); FM5's `ManufacturerColors.bin` probably holds the real paint, not decoded.
- Shared-library parts' positions: their bone's x, the wheel locator's y and z; right-side parts turned half a circle rather than mirrored.
- Rim width = tyre width (the databases have no rim width).
- **The tyre's whole shape** (`tyre.rs`): no tyre model exists in the games' files (searched: every zip under `media`, the executable's strings); the database names the tyre "model b" and its materials `tireTread_b` / `tireSidewall_b`, so the game probably builds the shape in code. Ours: rounded sidewalls, flat tread over 80 % of the width, dark rubber. Textures of those materials not used.
- The camera box and the body contact box are the model's own extent, not the games' (they ship none).
- A morph buffer with one target holds the last target (seen once).

## 7. Known gaps (not done)

- Normal, gloss and ambient-occlusion maps are ignored (carbon weave, stitching, brushed metal look flat). Normal maps need tangents.
- About 21 IDx meshes whose only colour source is the game's grey placeholder texture stay mid grey (brake calipers, some trim).
- Missing textures that are in no library zip: IDx `gradient_012`, `rtint_colors_001`, `screw_003_opac`; E21 `exhausthole`, two screw masks; F-Type `exhausthole`, `gradient_002`, its symbol sheet, a pattern mask.
- The ambient-occlusion texture format (flags `6e 02 00 90`) and 16-bit float textures do not decode.
- The gauge cluster's 72-byte vertex layout (IDx and E21) and FM6's `ProxyLOD` (48-byte layout) are not read.
- Cockpit view, lit lamps, damage, morph normals, skinning: not done.
- Car names: the string tables of these games are not resolved; the index shows the media name.
- The launcher's setup does not offer FM5, FM6 or FH2 Xbox One.
- Headlight models (72-byte vertex layout) are skipped on cars that have them (the R34).
- The Rocket 69's 0-60 is 1.3 s slower than the database (one gear, flat curve; not investigated).
- 0-60 times are 0.3-0.7 s slower than the databases' figures on all three (launches are grip-limited; not investigated).

## 8. Not tested

The F-Type in the game (only the log of one run); the Fast7 GT-R, Rocket 69 and R34 in the game window; the IDx since the camera / texture / small-texture fixes; any other FM5, FM6 or FH2 Xbox One car; these cars in the car browser, in races, with AI, online (offline only by design); their sound (each uses an FH1 sound donor picked by the FM4 rule); Linux builds; textures above 1024 pixels (pipe / bank terms beyond x7 unknown); 2D mips whose sides are not multiples of 64x32 blocks (the decoder refuses them).


## 10. How to check it (Windows, PowerShell)

Build (without the DLSS SDK):
```powershell
cargo build --release -p fh1-engine --no-default-features
cargo build --release -p fh1setup
```
Import the three Xbox One cars (reads the game folders, writes the install only):
```powershell
./target/release/fh1setup.exe import-fm6 "D:/Games/Non-Steam Games/Azimuth/Game Files/FM6/Mount" --cars NIS_IDxNISMO_15
./target/release/fh1setup.exe import-fm5 "D:/Games/Non-Steam Games/Azimuth/Game Files/FM5/Mount" --cars LOT_E21_13
./target/release/fh1setup.exe import-fh2xb1 "D:/Games/Non-Steam Games/Azimuth/Game Files/FH2/Mount" --cars JAG_FTypeCoupe_15
```
Expected report lines: IDx `79 parts, 307 body meshes, 4 wheels, 207580 triangles, 25 textures`; E21 `30 parts, 184 body meshes, 4 wheels, 243863 triangles, 42 textures`; F-Type `85 parts, 411 body meshes, 4 wheels, 235050 triangles, 30 textures` (each followed by the textures that did not load).

Drive one (`$env:FH1_OWNERSHIP="0"` turns off the owned-games check if a car is refused):
```powershell
cargo run --release -p fh1-engine --no-default-features --bin fh1-engine -- --car ../imported/fh2xb1/cars/JAG_FTypeCoupe_15
```
Numbers without the game window:
```powershell
$env:SETTLE="1"; $env:FLAT="1"; ./target/release/drive_probe ../imported/fh2xb1/cars/JAG_FTypeCoupe_15
```
(expected: front share 52.1 %, total 100.0 % of weight). Clear `SETTLE` and set `CHECKS=1` for the launch / stop / cornering lines.

Format probes (point them at a car unpacked with `cargo run -p fh1-formats --example zip_extract -- <car.zip> <git-ignored folder>`): `grub_probe` (container), `modelbin_probe` (models, box and normal checks, OBJ output), `swatch_dump` (textures to PNG), `swatch_survey` (texture sizes in zips), `fm6_car_probe` (placement, materials, wheels, tyres).

FM2 circuit checks: see the history section below ("How to check it").


---

# HISTORY AND EVIDENCE (older sections, newest first)

# PREVIOUS STATUS (2026-10-29): the FM2 scenery pass, then the first FM6 / FM5 notes

(This was the top of STATUS.md on 2026-10-29; superseded by the 2026-10-30 status above.) The change log with every step and every number is `MODLOG.md` (entries 86 to 96 are this latest pass).

**Nothing from the last sessions is committed** (AGENTS.md: commit only when asked). The working tree on branch `ui-elements` holds a lot of uncommitted work (importer, engine, renderer, formats, docs). Before any commit, run the commands in "How to check it" and commit in small steps.

## FM6 Nissan IDx, step 2 (textures): was BLOCKED, SOLVED in the sixth round below (2026-10-29)

(The template `STATUS-handoff.md` named in AGENTS.md does not exist in the repository, so this follows the rules in AGENTS.md instead.)

**Goal:** get the Nissan IDx Nismo from Forza Motorsport 6 (`D:\Games\Non-Steam Games\Azimuth\Game Files\FM6\Mount\media\cars\NIS_IDXNismo_15.zip`) into the engine. Plan: `docs/DESIGN.md` "FM6 IDx". Step 1 (the `burG` wrapper) is done and checked (MODLOG 97).
**Where it stops:** step 2, turning `.swatchbin` textures into pictures. The header is understood; the pixel layout is not.

What is known (checked on all 68 textures of the IDx, `crates/fh1-formats/examples/swatch_probe.rs`):
- The metadata of the `TXCB` chunk is: `HCXT` tag, 4 bytes, a 16-byte GUID, then u16 width, u16 height, u16 depth (1), the same three again, u8 mip count twice, a zero byte, and 4 flag bytes. Example: symbols DIFF = 512x256, 10 mips, flags `0e 00 00 a0`.
- Pixel byte counts fit block-compressed data with a full mip chain plus padding: 0.5 byte per pixel (flags start `6e`, GLOS / OPAC maps: probably BC4), 1 byte per pixel (flags `ae`, NRML maps: probably BC5; `2e`: probably BC3 or BC7), 4 bytes per pixel (the radiosity maps, flags `ae 01 12 90`, probably uncompressed 32-bit). The first flag byte looks like [3 format bits][5 bits always 01110]; this is a guess from the pattern, not read from the game's code.
- Reading mip 0 row by row gives a recognisable but scrambled picture, so the blocks are stored tiled, not linear.

Tried and failed (all on `NIS_idxnismo_15_symbols_DIFF`, as BC1, mip 0 only): linear order; 4x4, 8x8, 16x16, 32x32 blocks per tile in Morton order (x first and y first); the AddrLib "1D thin" micro-tile bit pattern for 8-byte elements with tiles row by row. The 8x8 attempts gave partly coherent pieces, so a tile structure exists, but no attempt gave a clean picture. The picture is also tinted red, so the format guess (BC1) may be wrong too; two unknowns were being varied at once.
**Not tested:** any other format guess for this file; a texture with known content (a flat colour or gradient) to read the layout from; the game's own code (`forza_durango_release_final.exe` in the FM6 Mount folder, 31 MB) for the swizzle routine or the format table.

**Second round (same day, after "try the leads"):** partly new facts, still no clean picture.
- NEW, solid (arithmetic, checked on two textures): the pixel data is mip 0 first, then each smaller mip in turn, and EVERY mip is padded to a multiple of one micro tile = 8x8 blocks (512 bytes for 8-byte blocks, 1024 for 16-byte blocks). Example: 1024x1024 BC4, 11 mips: 699,064 bytes of mips + 2,376 bytes of padding = the file's 701,440 (the five smallest mips are padded to 512 each); symbols 512x256 BC1: 90,112 = 65,536 + 16,384 + 4,096 + 1,024 + 6 x 512. This fits a "1D tiled" layout (micro tiles only, no larger macro tiles), where the micro tiles would be in row order and only the order of blocks INSIDE a micro tile is scrambled.
- NEW, weak (a smoothness score on block means; weak because most textures are mostly dark or noisy): inside a micro tile the order that scores best on three textures is Morton `x0 y0 x1 y1 x2 y2` (or the same with x and y swapped; the score cannot tell). But with row-major micro tiles over the whole image the same score picks a different order for the BC1 symbols texture ([y0 y1 x0 y2 x1 x2]) than for the BC4 gloss map (the Morton one), so the combination is NOT confirmed. No picture decoded with this layout looked right (gloss map by eye: structured fragments, tile boundaries visible).
- Format column (from byte counts, still a guess): flag byte 0 = `0e` BC1, `2e` BC3 or BC7, `6e` BC4, `ae` BC5 (or 32-bit for the radiosity maps).
- The two leads: (1) `wiper_motion_gradient` is not a plain gradient (11,264 distinct index patterns in mip 0), so it did not show the order by itself. (3) the smoothness search: pure bit-interleave search, an XOR hill-climb, and a within-tile search all run; see above for what they could and could not say. (2) the game executable: its strings contain no texture tiling routine names (the one "TileMode" hit is the UI brush property), and the loader is x64 code that I did not disassemble. Not tried: `xg_x.dll` (Microsoft's texture layout library, in `FM6\EmbeddedXvd\Windows\System32`), which holds the real tile modes; a disassembler (not installed here).
- Tools left in git-ignored `data/work`: `swz_search.py`, `swz_gf2.py`, `swz_tile.py`, `swz_macro.py`, `swz_rm.py` (all take a swatchbin path, a format (bc1 / 8 / 16 = bytes per block) and the width and height); in the repo: `swatch_probe.rs` (modes `m<N>`, `my<N>`, `g<N>`, `x=<index positions>`).
- Recommendation: the picture is the cheaper half to give up on first. Step 3 (the meshes) does not need textures; a grey untextured IDx in the engine would prove the car pipeline, and the texture layout can be solved afterwards (best remaining lead: read `xg_x.dll`'s tile-mode table, or find a texture that is a known solid pattern).

**Third round (same day, "keep on textures"):** a real lead on WHAT the layout is, a correction, still no picture.
- NEW, solid: `xg_x.dll` (135 KB, in `FM6\EmbeddedXvd\Windows\System32`) is Microsoft's Xbox One texture-layout library and contains AMD's AddrLib (GPU address code): its strings list the tile modes (`ADDR_COLOR_1D_THIN1`, `ADDR_COLOR_2D_THIN1`/`THIN2`/`THIN4`, `ADDR_COLOR_2D_THICK`...) and the fields (`ADDR_PIPES`, `ADDR_BANKS`, `ADDR_MICRO_TILEMODE`, `ADDR_PIPE_CONFIG`, swizzles). So the swatch pixels follow a standard GPU layout, not a Forza-specific shuffle. AddrLib rule I remember (from memory of its source, NOT checked against the dll): big mips use a 2D macro-tiled mode with pipe / bank swizzles, mips smaller than a macro tile fall back to 1D (micro tiles only). That would explain why the small mips are padded to 512 bytes while the big ones show no padding, and why my row-major-tile attempts only gave fragments for mip 0.
- CORRECTION to the format guess in "Second round": the first flag byte does NOT decide the format alone. Bytes per pixel (file size / pixels of the mip chain) against the four flag bytes: `0e 00 00 a0` 0.5 (symbols); `6e 00 00 a0` 0.5 (mostly; one 4.0); `2e 01 xx b0` 1.0; `6e 02 00 90` 1.0 (the 27 AO maps, so not BC4); `ae 00 00 b0` 1.0 (and one 2.2, one 7.9); `ae 01 00 90` and `ae 01 12 90` 4.0 (NRML maps and the radiosity maps); `6e 01 00 a0` 8.0 (ProxyLOD, one mip); `ad 01 00 40` 5.2 (trip, 33x37). A simple "pad every mip to AxB blocks" size model fits only 32 of the 68 files (best A=B=4), so the mip padding is not that simple (the 2D macro-tile padding of AddrLib is the likely reason). The earlier statement that 6e = BC4 and ae = BC5 should be read as unproven.
- Not done: reading the 80 KB of x64 code in `xg_x.dll` (capstone is installed for Python; no other disassembler), or implementing AddrLib's 1D/2D thin addressing from its public source and testing it. The cheapest next test is the 1D-thin micro-tile bit order for 64-bit and 128-bit elements on the smallest mips (those are 1D), where no macro-tile guess is needed: decode the last 4 mips of any texture and check them for a plausible picture.

**Fourth round (same day, "more textures"):** two firm facts, one partial fit, still no clean picture of any mip bigger than 8x4 blocks.
- FIRM: the smallest mips are stored densely and in index order. Which of a mip's 64 (or 1024-byte) slots hold data: a 2x1-block mip uses slots 0-1, 4x2 uses 0-7, 8x4 uses 0-31, 4x4 uses 0-15, 2x2 uses 0-3 (checked on three textures; the rest of the 512-byte slot is padding). So there is no 8x8 Morton scramble for these. Decoded row by row with the mip's own width, the 8x4-block mip of the symbols texture is a coherent picture (two red bars); this is the first decode that looks right.
- FIRM (statistics of the block bytes): the `2e ...` textures (1 byte per pixel: interior DIFF, gauges, livery, radiosity maps) are BC7, not BC3 (the first byte's mode field clusters on modes 4, 5 and 6; BC3 would show colour endpoints c0 > c1 in nearly every block, and only 35 to 40 percent do). `ae 00 00 b0` (normal maps) behave like BC5; the 27 AO maps (`6e 02 00 90`, 1 byte per pixel) match neither BC3 nor BC7 cleanly (many all-zero blocks): their format is UNKNOWN.
- PARTIAL: the symbols texture's 16x8-block mip is two 8x8 tiles; its best tile order is the plain one (tile 0 left, tile 1 right), but decoded with rows of 8 blocks inside a tile it still shows jagged staircase edges, so "linear inside the tile" is NOT confirmed (it may be a different in-tile order that happens to agree for the 8x4 case). The 32x16-block mip (8 tiles): an exhaustive search over all 8! tile arrangements cut the edge mismatch from 110.6 to 32.6 with a clear XOR rule (tile index bits: t0 = tx0 ^ ty0, t1 = tx0 ^ tx1 ^ 1, t2 = ty0 ^ 1). The same kind of fit on bigger mips (symbols mip 1; the badge gloss map's mips 0 to 2) gives different rules each time and only partial cost cuts, so those fits are local minima, not answers (the in-tile order they depend on is unconfirmed). No big-mip picture was decoded correctly.
- Pixel-format chain consequence: nothing can be exported to the engine from textures yet.
- Next step I would take: get the real rule from the code instead of fitting it: disassemble `xg_x.dll`'s texture-layout routine with capstone (the AddrLib strings are at 0x1e220 to 0x1f5a8 of the file; `XGComputeTexture2DLayout` is export number 8 of 15), or port AddrLib's public SI/GCN tile code (1D thin micro tile, 2D thin macro tile, pipe/bank swizzle) and match its output against the three firm facts above. Both are larger jobs than the probes so far.

**Fifth round (same day, "port tiling code"):** the in-tile order is now PROVEN from the game's own library code; the order of whole tiles in big mips is still unsolved. This corrects the "linear inside the tile" reading in rounds two and four.
- FIRM, from code: I disassembled `xg_x.dll` (capstone; the function at file RVA 0x10390 turns a position inside a micro tile into x and y). For the "thin" micro-tile types (types 1 and 2 of the setting at +0x160) the in-tile index maps to x and y by plain Morton interleave, whatever the element size: `x0 y0 x1 y1 x2 y2` (index bit 0 = x0, bit 1 = y0, bit 2 = x1, bit 3 = y1, bit 4 = x2, bit 5 = y2). The "display" type (type 0) is row-by-row for 8-bit elements and uses other orders for larger ones. A micro tile is 8x8 elements, and for block-compressed formats an element is one 4x4 block.
- FIRM, from the data: that Morton rule fits the swatches. Mip-to-mip consistency (a mip shrunk 2x should equal the next mip): with Morton inside the tile the mismatch is 1.8 against 12.8 for row-by-row (symbols, mip 4 to 5), 4.9 / 4.8 against 68.6 / 55.3 (badge gloss map, mips 4 to 5 and 5 to 6), 3.4 against 105 (grill opacity map, mip 3 to 4). This also explains the "dense slots 0..N-1" finding of round four: an 8x4, 4x2 or 2x1 block region in Morton order occupies exactly the first 32, 8 or 2 slots, and a 4x4 region the first 16.
- FIRM: mips that are 1 to 4 tiles wide and high (up to 32x32 blocks = 128x128 pixels) use plain row-by-row tile order with Morton inside each tile: checked on the badge gloss map (mips 3 and 2, mismatch 3.1 and 1.3) and the symbols texture (mip 2, tile index = tx0, tx1, ty0 on all 5 confident tiles).
- NOT SOLVED: mips of 8x8 tiles or more (64x64 blocks = 256x256 pixels and up, so mip 0 and 1 of most textures). Row-by-row tile order gives mismatch 77 (badge mip 1) against 1 to 3 for correct ones, so these mips use a different (2D macro-tiled, pipe/bank swizzled) arrangement. A GF(2) hill-climb and a per-tile match against the next mip both failed because most tiles are flat or empty (too few confident matches: 8, 15 and 1 of 64 on three textures, and the fitted rules disagreed). The code for the macro layer is the XOR-heavy functions at RVA 0x10940 (638 instructions), 0xd9a0 and 0xe5d0 in xg_x.dll; they take configuration fields from a struct (pipes, banks, bank width and height, macro aspect: values for the Xbox One GPU are unknown to me) and are not read yet.
- Mistake recorded: I named a helper script `dis.py`, which shadowed Python's `dis` module and broke the capstone import (the same kind of mistake as `bisect.py` before). Renamed to `xgdis.py`.
- Next: (1) find textures whose big mips have content in every tile (colourful BC1 or BC7 maps) and run the per-tile match on them (needs a BC7 decoder in the solver; the Rust crate texture2ddecoder has one); (2) read RVA 0x10940 and its callers to get the macro-tile formula directly; (3) with Morton and row-by-row tiles solved, mips 2 and smaller of every texture can already be decoded: that gives 128x128 previews today (and the car's textures at low resolution as a stopgap for step 3).

**Sixth round (same day, "solve macro tiling"): SOLVED for the formats the car's main textures use. Step 2 is no longer blocked.**
- The full address rule is in `crates/fh1-formats/src/swatch.rs` (doc comment) and decodes 37 of the IDx's 68 textures, including the 1024x1024 BC7 interior atlas (seats, dash, door card, steering wheel, all clean) and the livery number. Check: `cargo run --release -p fh1-formats --example swatch_dump -- data/work/fm6_idx data/work/fm6_png/all` (PNGs go to the git-ignored folder).
- How it was found: (1) the in-tile Morton order from `xg_x.dll` (round five); (2) per-block matching of a mip against the next smaller mip (each 4x4 block, shrunk, against every 2x2 patch), keeping only clear matches (31 on the badge gloss map), then an exact XOR (GF(2)) fit of every address bit with RANSAC; this gave the 64x64-block rule (mismatch 0.7 against 77 for row order) and decoded the Nissan badge cleanly; (3) the same on mip 0 (139 matches, mismatch 0.33); (4) the two rules together matched AMD's 2D macro tiling with 8 pipes, 4 banks, 256-byte pipe interleave and 64x32-element macro tiles, which I wrote as one formula; (5) a search for extra XOR terms on three wide or large textures found one more: bank bit 1 also takes x7 (combined mismatch 72.7 to 1.77).
- Checks of the final formula (mismatch of each mip, shrunk, against the next; wrong layouts give 5 to 113): BC4 badge 0.7 / 0.33, BC1 symbols 0.39 / 0.66 (non-square), BC4 grill 0.47, BC4 interior gloss 0.24 / 0.16 / 1.71, BC5 normal maps 0.35, 0.67 / 0.31, 8888 512x512 0.08, 8888 1024x1024 0.06; 1D below 64x32 elements, 2D from 64x32 up (checked at 64x32 and 64x64).
- Also fixed: mip sizes. Every mip is padded to whole 8x8-element tiles in BOTH directions (a 16x4-block mip takes 16x8 blocks). With that, the exact file size is reproduced for 36 of 67 textures (all BC1, BC4, BC5, BC7 and 8888 ones except the three below). The earlier "pad to 512 bytes" model was wrong for thin mips.
- Still NOT decoded (31): the 27 baked ambient-occlusion maps and two `Rear_*` maps (flags `6e 02 00 90`: about 1 byte per pixel, but neither BC3 nor BC7 by their block statistics and their size does not fit the padded model; format unknown); `ProxyLOD` (`6e 01 00 a0`, 8 bytes per pixel, probably 16-bit float, not tried); `NIS_grill_001_*` and `PTN_circle_006_NRML` (their sizes are 2 to 4 times what 64x64 / 128x128 needs: unexplained).
- Not checked: 2D mips whose sides are not multiples of 64x32 elements (the decoder refuses them; none among the decodable IDx textures); textures above 1024 pixels (pipe/bank terms beyond x7 unknown); BC7 decode correctness beyond "the pictures look right" (BC7 cannot be checked by my Python tools; the interior atlas and gauges look correct by eye); the byte order of the 8888 maps (R G B A assumed); sRGB or linear (flag byte 3 might say, unknown).

Next steps I would take (in this order): (1) read `data/work/fm6_idx/Textures/Wiper/wiper_motion_gradient.swatchbin` or `splatterMask`: a gradient or mask shows the block order much more clearly than a symbol atlas; (2) search the Durango executable for the swatch loader and its tile-mode / DXGI format table; (3) score candidate layouts by how smooth the decoded picture is at block borders instead of judging by eye.

## RESOLVED (2026-10-30): FM5 Lotus E21, front tyres under-loaded at rest
The cause was not FM5's suspension data (the earlier guess here was wrong): a fallback body contact point, placed from a car box that included the wheels, rested on the ground. The importer now gives the engine a body-only box; the E21 settles at 45.9 % front (data 46.0 %) with all of its weight on the tyres. Details: MODLOG 114.

Words used here: **engine** = the game program (`fh1-engine`); **importer** = `fh1setup`, which turns a game disc into files the engine reads; **Xenia** = the Xbox 360 emulator the user runs the original games in, used only as a reference picture (twelve screenshots `screenshots/xenia2/x1..x12.png`); **cut-out** = a flat card with see-through parts (leaves, fences, grass tufts); **baked** = computed once at import time and stored as a texture; **inferred** = a value I fitted to the Xenia pictures, not read from the game's files.

## 1. What this project is

A Rust / Bevy rewrite of the Xbox 360 era Forza games. It plays Forza Horizon 1 from the user's own disc and imports content from other Xbox 360 Forza games the user owns (Forza Horizon 2, Forza Motorsport 4, 3, 2 and the original FM1's cars). It reads the games' own data and rebuilds the game systems from it; it is not an emulator. Hard rules (AGENTS.md): no game files or extracted data in the repository (converted data lives in git-ignored `data/installations/<id>/assets/private/`); `.gitignore` is a whitelist; game installs are read only; single-player and offline only; prefer the game's own data to guesses; the assistant cannot see the game, so it measures from logs and screenshots and says "not tested" when something is not.

| Crate | Job |
|---|---|
| `fh1setup` | The importer: disc -> asset groups; `import-fm2` (circuits, cars, spectators, cubes, UI), `import-fm4`, `import-fm3`, `import-fh2`, `import-fm1` |
| `fh1-formats` | File parsers (Forza zips, Xbox 360 textures, models, scenery, PVS visibility, FM2 `.crowd`, skinned spectator models, animations) |
| `fh1-engine` | The game: car physics, cameras, streaming, races, AI, HUD, crowds |
| `fh1-remaster` | The PBR renderer: materials, shadows, sky, car paint, post-processing, FM2 scenery with lightmaps |
| `fh1-render`, `fh1-shaders` | Shared render pieces and the Xbox 360 shader-microcode disassembler / WGSL translator / CPU interpreter |
| `fh1-world`, `fh1-audio`, `fh1-radio`, `fh1-video`, `fh1-ui`, `fh1-net`, `fh1-launcher` | Collision, sound, radio, movies, UI, online, launcher |

Data flow: disc -> `fh1setup` -> `data/installations/<id>/assets/private/...` (imported games under `imported/<game>/`, remaster tables under `remaster/imported/...`) -> `fh1-engine`. The engine mirrors z against the game's files; every importer-to-engine path flips it (a mistake of this kind was one of the crowd bugs below).

## 2. Current state at a glance

| Area | State | Evidence |
|---|---|---|
| Forza Horizon 1 (Colorado) | Works per README; only regression-checked this pass: Colorado's crowd after the GPU-buffer change (same look, 9.7 / 9.8 ms) | README (not re-tested in full), MODLOG 86 |
| FM2 circuits, 3D scene vs Xenia | Close on all twelve, not identical; scenery defects found in this pass are fixed (section 3); a global brightness difference and three inferred values remain (sections 4 and 5) | twelve shots by eye, road-band score 6.44 to 6.46 (6.75 before this pass; noise about 0.1) |
| FM2 HUD, minimap, pause screen | Done, committed earlier (branch `ui-elements`) | section 0 below |
| FM2 cars | Imported and raced; body paint still much brighter than Xenia's near-black on most circuits | sections 0a, 0b below |
| Races on imported circuits (FM2 / FM4) | Work through the dev hook `FH1_RACE=0`; never driven by a person; not in the menus | section 6 below |
| FM1 / FM3 / FM4 / FH2 content | Imported; not re-checked in this pass | README, section 4 below |

## 3. The FM2 scenery pass (2026-10-28 / 29), item by item

For each: what was wrong, the cause found in the game's data, the fix, and the evidence. Evidence is always "by eye against the Xenia shot" plus, where it applies, the road-band score (`data/work/cmp12.py`, lower is better).

1. **Spectators (crowds).** Ours had none. The disc has `Media/Spectators.zip` (skinned figures, animations, a sprite atlas, `spectators.xml`) and a `.crowd` placement file per track (358,757 spectators on 32 maps). New importer `fm2_crowd.rs` (`import-fm2 --only crowd`) and engine support (`crowd.rs`, `crowd.wgsl`, `crowd/figures_gpu.rs`). Bugs found on the way: the `.crowd` z was negated twice (every crowd mirrored, so none near the start); the GPU skinning-matrix buffer was re-created each time a new animation clip was added and figures made earlier read zeros and vanished (now allocated at 16 MB up front); the track's `<Crowds lod0..3>` (5 / 20 / 50 / 125) are read as distances with a 600-figure cap; near figures are drawn unlit at a 0.75 grey on FM2 maps (inferred). Matches Xenia on Sunset, Maple Valley, Tsukuba, New York (both walls), Mugello (balcony marshals and photographers), Nurburgring; the circuits without a visible crowd in Xenia show none in ours. Frame cost: no measurable change (one HUD reading per circuit, 7 to 11 ms).
2. **New York marquee ("Welcome to Forza Motorsport 2 - Fuel Your Fantasy").** The quads existed but drew as nothing: the importer's cut-out rule called the texture a cut-out because its alpha runs only 21..168, and the 0.5 alpha test deleted it. Cut-out rule fixed; the pixel shader (family `3d69a1a8dce9ca9e` / `8ae1a07996db6f64`) is an LED-board shader, drawn unlit with a 1.3 gain (inferred). Text colour (243,186,109) against Xenia (236,188,125).
3. **Silverstone's white-and-black curb outline and far-left arc.** The terrain-blend pixel shaders (4 to 6 texture fetches, a branch) were guessed by the importer. They are now run on the CPU and baked into one texture per draw (`fm2_scenery.rs bake_blend`, using `fh1-shaders/src/ps_cpu.rs`): 90 of 90 Silverstone draws, 232 on Road Atlanta, a handful elsewhere. A thin strip needed the texture to cover the draw's own uv box.
4. **Nurburgring.** (a) The frosted pale-cyan spruces: the shader reads only the alpha of the first texture and the colour of the second; the importer used the first as colour (fixed: alpha-only textures are not colour candidates). (b) Thin, bare-pole trees: every one of 13,148 cut-out materials was one-sided, so single-quad leaf cards vanished from behind; FM2 cut-outs are now two-sided (`remaster.rs`). (c) A distance-dependent alpha cutoff (cutoff x 0.3 from 80 m, `FH1_RM_CUTOUT_FAR`; inferred) keeps far leaf sheets from thinning out as their mip-averaged alpha falls. Result: the round tree at the road's edge has its canopy, the tall dead trees have their branches.
5. **Road Atlanta's yellow and green boxes along the wall base.** They were grass-tuft cards whose textures are 98 to 99.8 % clear (a few strands); the cut-out rule rejected them and they drew as solid rectangles. Rule fixed (any solid texel, and alpha only has to reach 128). Every record and texture bisection missed it because the boxes came from dozens of materials; it was found by listing the draws round the wall base and looking at their textures.
6. **Suzuka's red line across the start.** Also in Xenia's shot (thin dark red); ours is only brighter. Not a defect.
7. **Night.** FM2 circuits have one baked daytime lightmap and no night data; `FH1_TOD=22:00` changes only the car's lamps. Nothing to check.
8. **Sky.** The sky pixel shader was read (microcode at `.gpu` +0x28e0 of the sky model; `FM2_DUMP_SKY_PS=1`): `out.rgb = tex.rgb * (A + (1 - A) * c104.w)` with `A = tex.a * skyGain`, and `c104.w` (GlobalSpecularLevel's alpha) is 1.0 on every track, so the game's sky shader returns the texture unchanged. `c_skyMultiplier` (constant 188) is `skyGain` from the XML (traced through the executable: field 0x684, per-frame block +0xf50). The per-track differences between Xenia and ours are therefore not from the sky shader. See section 5.
9. **Small items.** Lightmap light floor 0.12 -> 0.06 on FM2 (inferred; 0 turned New York's red banner blue); the combine and tone curves are the game's own and were not changed.

## 4. What is close and what is still different

Close: overall colour, skies (with the gain table, section 5), roads on most circuits, crowds, banners and signs, curbs, foliage, the New York marquee, the HUD. Verified by eye on all twelve (Nurburgring, Maple Valley, Suzuka, Tsukuba, Sebring, Road Atlanta, Test Track, Sunset, Silverstone, New York, Mugello, Laguna Seca side by side).

Still different (not fixed):
- **Global brightness.** Luminance percentiles (5 / 25 / 50 / 75 / 95 %, mean of twelve): Xenia 11 / 48 / 86 / 139 / 206, ours 31 / 72 / 102 / 141 / 187: ours is about 20 too bright from the darks to the middle and about 19 too dim at the highlights. A global contrast curve (`FH1_FM2_CONTRAST=1`, full map) moves the percentiles to 19 / 55 / 92 / 148 / 202 but wrecks the road bands (21.5); a partial-strength test was started and not completed (MODLOG 96). The sky residuals below are the same difference seen in the sky.
- **Car body.** Xenia's Skyline is near-black navy with bright sky reflections on about nine tracks; ours is bright blue with weak reflections. The game's per-vertex transfer-number lighting is rebuilt (`FH1_FM2_CAR_PRT=1`, opt-in, neutral overall); the body shader's own shadow term is what is missing (sections 0a and 0b below).
- **Shadows** are weaker and softer than Xenia's (Test Track's car shadow was 3.7x too bright; only Test Track has the display-space shadow fix).
- **Tsukuba's big clouds**, **Mugello's sun disc**, the **start marker speckle** and **Nurburgring's road gloss** are open (older notes below).
- Nurburgring's canopies are denser and darker in Xenia than ours.

## 5. Values that are INFERRED (not read from the game); all can be switched off

| Value | Where | Why inferred | Switch |
|---|---|---|---|
| Per-circuit sky gain (Tsukuba x1.55 ... ), ratio^2.2, max 2; none for New York and Road Atlanta (banners in the measuring patch) | `fh1-engine/src/main.rs`, `fm2_sky.rs` | Xenia / ours sky-patch brightness per circuit. The game's sky shader applies no per-track grade (section 3 item 8); `skyGain` correlates 0.32 with the residual, `TrackLumLimits` max 0.78 (adaptive exposure is the likely family, not modelled). The engine's frame-average estimator is the same as the game's (a mip-chain average), so the residual is the global brightness difference | `FH1_FM2_SKY_GAIN=1` |
| Far cut-out cutoff factor 0.3 (from 15 m to 80 m) | `fh1-remaster/src/material.rs` | Stands in for the game's alpha-to-coverage; 0.12 to 0.5 looked right at about 150 m; 1.0 left distant trees sparser | `FH1_RM_CUTOUT_FAR=1` |
| Lightmap light floor 0.06 | `main.rs` | New York banner (73,20,30) against Xenia (47,8,10), 0.12 gave (101,28,39), 0 turned it blue | `FH1_RM_LM_LIGHT_FLOOR=0.12` |
| LED-board gain 1.3, near-figure grey 0.75 | `remaster.rs`, `crowd.rs` | Fitted to one shot each | `FH1_FM2_LED_GAIN`, `FM2_NO_LED=1` |
| Earlier per-circuit tone-curve blend, saturation x1.5, car look, wear darkening 0.2, Test Track shadow gamma 2.2 | `main.rs` | see section 0a below | see section 0a below |

Sky-patch ratios Xenia / ours: with the gain table off (game's combine on): Tsukuba 1.55, Mugello 1.40, Nurburgring 1.25, Suzuka 1.20, Sunset 1.06, Sebring 0.96, Silverstone 0.91, Maple Valley 0.85, Laguna 0.85; with the table: 0.87 to 1.17. Forcing the combine's blend `t` to 0 matches the sky better than the computed `t` on Tsukuba and Mugello but wrecks the roads (road bands 11.32), so the fix would be a better frame-luminance input, which is not available.

## 6. Importer and engine changes of this pass (all uncommitted)

- New: `crates/fh1setup/src/fm2_crowd.rs`; `crates/fh1-remaster/src/car_prt.rs` and the car-paint changes (earlier pass); examples `spectator_atlas`, `skin_probe`, `anim_probe`, `carbin_fm2_tail`, `stx_list`, `xpr_cube` in `fh1-formats/examples`.
- Changed: `fm2_scenery.rs` (cut-out rules, `bake_blend`, `alpha_only_items`, LED family, debug switches), `fm2_uv.rs` (`shader_eval_at`, `dump_shaders`), `fm2.rs` (maps import also writes the crowd placement: re-importing a map deletes its folder), `remaster.rs` (LED unlit, two-sided cut-outs), `material.rs` (far cutoff), `post.rs`, `crowd.rs` / `crowd.wgsl` / `figures_gpu.rs`, `main.rs` (FM2 defaults), `fm2_sky.rs` (sky gain), `fh1-shaders/src/fm2.rs` (`disassemble` public).
- Debug switches added (importer): `FM2_DUMP_PS=<texture hex>`, `FM2_PS_SIG=<hash>`, `FM2_DUMP_SKY_PS=1`, `FM2_ATLAS_MODEL`, `FM2_FORCE_ATLAS`, `FM2_DATA_DUMP`, `FM2_DEBUG` (rejected sprite patterns, bake failures), `FM2_NEAR=x,z,r` (draws near a point), `FM2_FIND_TEX`; (engine) `FH1_CROWD_LOG=1`, `FH1_CROWD_LOD_DIST=0`, `FH1_CROWD_FIG_CAP`, `FH1_TELEPORT=x,y,z,yaw`.
- Old rules still available: `FM2_OLD_CUTOUT=1`, `FM2_OLD_CUTOUT_SPARSE=1`, `FM2_CUTOUT_ONE_SIDED=1`, `FM2_NO_BAKE_BLEND=1`, `FM2_ALPHA_ONLY_COLOUR=1`.

## 7. How to check it (Windows)

Build (without the DLSS SDK) and import once after pulling these changes:
```powershell
cargo build --release -p fh1-engine --no-default-features
cargo build -p fh1setup
.\target\debug\fh1setup.exe import-fm2 data\work_fm2\disc --only maps
.\target\debug\fh1setup.exe data\work\disc --only remaster --force
```
Run a circuit (offline, no ownership check):
```powershell
$env:FH1_OWNERSHIP="0"; .\target\release\fh1-engine.exe --track fm2/nurburgring_00
```
What you should see: spectators behind the walls (New York, Sunset, Mugello, Maple Valley); an amber marquee under the NISSAN banner (New York); no curb outline round the grass patch (Silverstone); full-canopy trees and no pale frosted spruces (Nurburgring); no yellow or green rectangles along the wall (Road Atlanta).

The measuring tools are in git-ignored `data/work` (not in the repository): `shoot_some.sh <prefix> <tracks...>` (writes `screenshots/<prefix>_<track>.png`, needs `FH1_OWNERSHIP=0 FH1_SHOT=... FH1_SHOT_AT=28`), `cmp12.py <prefix...>` (road-band score), `skyratio.py`, `skyratio2.py`, `mat_bisect.py <track> x0 y0 x1 y1 yellow|white` (hides groups of materials to find which draw makes a feature), `rec_bisect.py`. A fresh prefix is needed each run (`/tmp/shoot_<prefix>.done` is left behind and a reused prefix returns at once with the old pictures).

## 8. Not tested

A full lap or a race on the new FM2 scenery; frame cost beyond one HUD reading per circuit; Colorado's walkers close up, a long drive, and other spots on the Colorado map after the GPU-buffer change; every other circuit's grass tufts and foliage one by one; the 16 MB GPU matrix buffer's behaviour past 16 MB of clips; Linux builds; the effect of any of this on FM4 / FM3 circuits (the importer changes apply to `import-fm2` only except the shared shader code).

## 9. Mistakes recorded in this pass (so they are not repeated)

- Measured crowd distances from the engine's placeholder camera (0, 3, 8) instead of the real spawn: the "nearest spectator is 178 m away" finding was wrong (the real cause was a doubled z flip).
- Mislabelled the Nurburgring pale-tree defect as Maple Valley (read the wrong row of my own comparison sheet).
- A helper script named `bisect.py` shadowed Python's `bisect` and broke the scoring script.
- Read Road Atlanta's boxes as a terrain-blend / atlas problem through seven failed hypotheses; they were opaque grass cards. Lesson: look at the textures of the draws at the feature before bisecting materials.
- Called the foliage mass a haze / lighting problem before testing back-face culling. Lesson: test the cheap render-state switches (two-sided, cutoff, alpha mode) first.
- A partial-strength contrast edit failed its assertion silently and the "0.3 / 0.6" runs were off / full strength; read the tool result before reporting a result.

## 10. What to do next

1. Decide whether by-eye parity with the twelve shots is the standard; if not, the one game-data lead left for the sky and the global brightness is the game's adaptive-exposure code (`TrackLumLimits`, `ToneMapeDelay`) in `default.xex` (`data/work/fm2_xex/text.asm` has the disassembly; `ppc_xref.py` finds code that references a string).
2. A real partial-strength contrast test (mix the full map with the plain image, score road bands and percentiles).
3. The car body's shadow term (sections 0a and 0b below).
4. Commit in steps when asked: formats + importer (crowds, cut-outs, bake), renderer (two-sided cut-outs, far cutoff, LED), engine (crowds, sky gain, defaults), docs.

---

# OLDER HISTORY (written 2026-10-26 and 2026-10-27)

# PROJECT SUMMARY (written 2026-10-26)

This is the long-form overview of the whole project: what it is, how it is put together, what works, what does not, and where the work stands. The sections after it ("STATUS: FM2 road brightness" and the dated "Update" entries) are the detailed evidence, newest at the bottom of the file. Where this summary repeats a claim from README.md that I did not re-check in the latest sessions, it says so.

Terms used once and then freely: **engine** = the game program (`fh1-engine`); **importer / setup tool** = `fh1setup`, which converts a game disc into files the engine reads; **imported map / circuit** = a track converted from another Forza game; **AI** = the computer-driven opponents; **racing line** = the path an AI car tries to follow; **lap gate** = an invisible line across the road that counts as a checkpoint; **Xenia** = the Xbox 360 emulator the user runs the original games in, used here only as a reference picture.

## 0c. FM2 scenery pass (2026-10-28 / 29; MODLOG 86 to 94; nothing committed)
Each item: what was wrong, cause (from the game's data), fix, evidence. "Evidence" is by eye against `screenshots/xenia2/x1..x12.png` plus the road-band score `data/work/cmp12.py` (6.75 before this pass, 6.46 after; run-to-run noise about 0.1); a Xenia shot is the only reference there is.
- Spectators: 358,757 placed from each map's `.crowd` (importer `fm2_crowd.rs`); z was negated twice (fixed); GPU figure buffer re-created while growing (fixed, 16 MB up front); lod0..3 read as distances; crowds match Xenia on Sunset, Maple Valley, Tsukuba, New York, Mugello (balcony), Nurburgring.
- New York marquee: its texture was rejected as a cut-out (alpha 21..168); rule fixed; the LED-board shader family drawn unlit (text (243,186,109) against Xenia (236,188,125)).
- Silverstone curb outline and arc: terrain-blend pixel shaders are run on the CPU and baked per draw (`bake_blend`, 90 of 90 draws; 232 on Road Atlanta).
- Nurburgring: spruces were coloured from the alpha-only texture (fixed); canopies and foliage mass: cut-outs were one-sided (two-sided now, `remaster.rs`), plus a distance-dependent alpha cutoff.
- Road Atlanta wall-base boxes: sparse grass-tuft textures (98 to 99.8 % clear) were rejected as cut-outs and drew opaque; rule fixed.
- Suzuka's red start line: also in Xenia, not a defect. Night: FM2 circuits have only a baked daytime look, nothing to check.
INFERRED, not game data (both overridable): the per-circuit sky gain table (`main.rs`, `FH1_FM2_SKY_GAIN=1` = off; the game's own combine is already applied and does not explain the residual, MODLOG 94) and the far cut-out factor 0.3 (`FH1_RM_CUTOUT_FAR=1` = off), and LED gain 1.3, the 0.75 foliage grey, plus earlier values in section 0a.
Not measured: frame cost beyond one HUD reading per circuit, anything not in the twelve shots.

## 0a. CURRENT STATE (2026-10-27): FM2 3D scene vs the Xenia shots

**Goal (set with /goal, a Stop hook keeps checking it):** the engine's FM2 circuits should be indistinguishable from the twelve Xenia screenshots in `screenshots/xenia2` (x1 Tsukuba, x2 Test Track, x3 Sunset Infield, x4 Silverstone, x5 Sebring, x6 Road Atlanta, x7 Nurburgring, x8 New York, x9 Mugello, x10 Maple Valley, x11 Laguna Seca, x12 Suzuka; x13 Tsukuba rolling start), using the game's own data first and inference only where it makes the picture more accurate. **That goal is NOT met.** The scene is much closer than at the start of the session but is plainly different in the ways listed under "Still different". Nothing from this session is committed (AGENTS.md: commit only when asked). The UI work (HUD, minimap, pause) is finished for now (section 0 below).

### Where things stand, in plain terms
- **Colour, tone and sky are close.** Skies show their real photos with clouds on most tracks; overall brightness and colourfulness are near Xenia on most tracks.
- **Roads are the right brightness on about half the tracks** (twelve-track road-band score 8.51; 0 = perfect; 10.9 before this session's work, 8.1 at the best point after the road texture fixes).
- **Cars are the biggest visible gap.** Xenia's Skyline is near-black navy on about nine tracks with bright sky reflections on the roof and trunk; ours is bright blue with weak, blurry reflections.
- **Shadows are weaker than Xenia's** (Test Track: the car shadow on the road is about 3.7x too bright).

### What was found in the game's own data and is now in the engine (all uncommitted)
1. **FM2's final combine (post-process) works on gamma-coded values**, not linear ones (earlier attempts were in linear space, which is why they looked "worse"). `crates/fh1-remaster/src/post.rs` (`Fm2Post`). On by default for FM2 tracks (`FH1_FM2_POST=0` turns it off). The game picks between a dark and a light tone curve from the scene's average brightness; we cannot reproduce that input, so the per-circuit blend `t` is a fitted table (inferred, see below).
2. **Sky:** the sky model has `shader_sky_cap` (draw 0, the zenith cap), then four sector photos (`shader_sky1..4`, alpha = sky coverage), then the sun disc and a glow card. The sectors are drawn **opaque**; the cap is draw 0 (not "the square texture": Sunset Infield and others have square sector textures, which used to be faded into a plain sky). Runtime textures (PVS flags 1) live in `.stx.bin` (Tsukuba's and Silverstone's cap is a 1x1 placeholder: nothing to draw). `crates/fh1-engine/src/fm2_sky.rs`, `crates/fh1setup/src/fm2_sky.rs`. The sun disc (untextured sphere, draw 6) is drawn additive yellow; **its look and place are not verified on screen** (it was never in frame in my shots).
3. **Road textures:** (a) shader-derived texture coordinates that collapse one axis (Sunset Infield model 77: u span 4612, v span 0) are rejected and replaced by world-metre grain, which restored its grey grainy asphalt (`degenerate_uv`, `FM2_NO_DEGENERATE_UV=1` = old); (b) Nurburgring's road keeps its two grey grain textures per pixel instead of a flat average (`FM2_NB_GRAIN=0` = old; gain 0.78 inferred); (c) the rubber streaks already existed as a wear blend but were too faint: darkening 0.4 -> 0.2 (`FM2_WEAR_DARK`, inferred).
4. **Scenery models found by hiding models** (`FM2_HIDE_MODELS=a,b`, `data/work/bisect.sh`): Tsukuba model 45 = the start-grid frame marks (a yellow frame texture with a transparent middle, previously drawn as a filled box; now a cut-out, `FM2_NO_BLEND_CUTOUT=1` = old; still speckled/thin because the texture alpha is noisy); model 101 = the lattice pylon (a tiny colour texture + a 512x128 lattice mask, previously a solid tower; now colour + mask-as-alpha, `FM2_NO_MASK_ALPHA=1` = old; fainter than Xenia).
5. **Car:** tail lamps and window glass tuned (lamp level 250, minimum lamp level 0.01, glass alpha 1.5); paint darker and less milky (fill 0.1, live probe off, albedo 0.7, reflectance 0.2, clear coat 0.3); **the car paint now reflects the track's own car cube map** (`staticCarCubemap.xpr`, imported as `car_cube.bin`, 16x16 per face, Fresnel from the game's `Normal.xml`; `FH1_FM2_CAR_REFL`, strength 1; face orientation not verified, an orientation test was inconclusive).
6. **Importer:** `fh1setup import-fm2 <disc> --only maps` also writes `car_cube.bin` per map; new examples `stx_list`, `xpr_cube`; diagnostics `FM2_HIDE_MODELS`, `FM2_SHOW_MODEL` (prints each draw and its texture coordinate spans), `FH1_SHOT_PROBE_PX=x,y;..` in the engine.

### Values that are INFERRED from the Xenia shots, not read from the game (set at program start for `--track fm2/...` in `crates/fh1-engine/src/main.rs`)
- Per-circuit tone-curve blend `t`: Tsukuba / Test Track / Laguna Seca 1 (light curve), Silverstone / Sebring / Road Atlanta / New York 0 (dark), Mugello 0.5, others computed from the frame. Fitted by trying 0 / 0.5 / 1 / computed against road bands and whole-frame brightness percentiles; `FH1_FM2_POST_T` overrides.
- Colour saturation x1.5 about the true luminance (`FH1_FM2_SAT`): our scenes were about 0.7x as colourful as Xenia's on ten of twelve tracks.
- Car settings listed in item 5; road wear darkening 0.2; Nurburgring grain gain 0.78; sun-disc colour; tail floor.

### Tried and REJECTED (do not retry without a new idea; numbers in MODLOG 42 to 75)
Global contrast curve; white sun colour; every sun-direction candidate (the track direction x mirrored, the game's default (1, -1, 0), the sky model's sun-disc direction, the disc for casting only) - the game's own path (`0x825220f0`) uses the XML `ShadowLightDirection`, which the engine already does, so the mismatch is in the shading; `FM2_BC4_SRGB_ALL`; road sheen off (needed for the sun glare on Silverstone / Road Atlanta); squared shadow colour (broke other tracks); car shadow decal without its elevation weight (Test Track better, four tracks too dark: 8.51 -> 10.35); car sun share and environment-light switches (no effect on the car body).

### Still different from Xenia (open)
1. **Car body:** Xenia near black on most tracks, ours bright blue; Xenia's roof/trunk shows bright sky reflections, ours are weak and blurry. Game data found: the body shader (`default.xex` effect @82066f50, techniques body / body_2 / carbon_fiber) takes per-object spherical-harmonic light coefficients (a per-object block rotated into the car's frame by `0x82733ad0`), the track light map under the car, an environment cube x `CubeMultiplier` 12, Fresnel (0.05 / 0.7 / 1.75 / power 5), Phong lobes; the source that fills the per-object block is NOT found (no SH basis constants exist in the executable, so it is loaded data). The car body measurement patch is also rough (it catches the number plate and lamp glow).
2. **Shadows:** Xenia's car shadow on the road is dark and sharp (about 7 % of the lit road on Test Track, where the baked light is full); ours is soft and faint. The projected-shadow decal (`proj_shadow.wgsl`), the sun cascade and the road shader's `min(1-(1-ShadowColor)*S, lerp)` rule all contribute but together are weaker/shorter than Xenia's.
3. **Tsukuba's big clouds** (Xenia x1) are not in the sector photos or any sky file I can read.
4. **Sun glow card** (sky draw 5, texture an 8_8 two-channel format the importer cannot decode) is not drawn.
5. **Start marker** is a speckled thin outline (Xenia: smooth faint line); **Nurburgring's road is too glossy**; **the pylon is fainter** than Xenia's.
6. **Road brightness** outliers (ours / Xenia in linear green, left band | right band, current defaults): Test Track 0.74 | 3.73, Sunset 0.64 | 0.95, Nurburgring 1.26 | 0.22, Road Atlanta 1.48 | 0.81, New York 0.54 | 0.54, Mugello 0.96 | 1.33, Suzuka 0.40 | 0.88. Right bands are often wall / shadow, not clean road.
7. **Contrast:** Xenia's shadows are darker and its highlights brighter than ours outside the road (luminance percentiles 5 / 25 / 50 / 75 / 95: Xenia 9 / 44 / 82 / 127 / 202, ours 22 / 70 / 97 / 127 / 172); a global curve fixes this but breaks the roads, so it needs to be selective.

### How to measure and test (all of it is in git-ignored `data/work` except the engine switches)
```
bash data/work/shoot_some.sh <prefix> tsukuba_00 testtrackinfield_00 sunsetinfield_00 silverstone_00 sebring_00 roadatlanta_00 nurburgring_00 newyork_01 mugello_00 maplevalley_00 lagunaseca_00 suzuka_00
python data/work/cmp12.py <baseline_prefix> <new_prefix>      # road-band sum |ln ratio|, lower is better; current defaults = 8.51
```
Screenshots land in `screenshots/<prefix>_<track>.png`; compare by eye against `screenshots/xenia2/x<n>.png` (crop (1,51)-(1281,771) removes the Xenia window frame). Re-import after importer changes: `./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps` then `./target/debug/fh1setup.exe data/work/disc --only remaster --force`. Build the engine without the DLSS SDK: `cargo build --release -p fh1-engine --no-default-features`. Only Tsukuba, Mugello, Sunset and Silverstone were looked at closely by eye after the later car changes; the rest were measured but not viewed.

### Suggested next steps
1. (Done, opt-in, see 0b) Find what fills the car's per-object light block (search the executable for the writer of the render item's light pointer; the uploader is near `0x8255ec1c`) and rebuild the car paint's diffuse and reflection from it.
2. (Partly done: Test Track only, see 0b) Make the road's cast shadow dark and sharp the way Xenia's is (strength and length of the sun-cascade shadow in `material.rs`, the projected decal, and the combination rule), then re-check all twelve.
3. (Done, see 0b) Keep the car cube as a full-resolution texture (the 16x16 reduction makes reflections weak and soft).
4. (Decoded and drawn opt-in; the sun is in no Xenia shot, see 0b) Decode the 8_8 texture for the sun glow card and verify the sun disc in a shot where it is in frame.
5. Commit in steps when asked (formats + importer, post + sky, roads, car).

Full evidence and every number: `MODLOG.md` entries 42 to 75; the saved memory note `fm2-3d-parity-findings`.

## 0b. The four suggested steps of section 0a, done 2026-10-27 (later; details MODLOG 76 to 82, nothing committed)
1. **Car light block: found, rebuilt, not adopted.** The per-object block is D3DX spherical-harmonic light (hemisphere from `SHTop` / `SHBottom`, one directional light from `SHDir`, env block from `SHEnvMapTop / Bottom`) dotted per vertex with 16 transfer numbers stored in the car file's vertex tail (the body's self-shadowing is baked in; decoded and checked on the Skyline). The engine now does the same on the CPU (`car_prt.rs`, importer `prt.bin`) and the paint uses it: **opt-in `FH1_FM2_CAR_PRT=1`**. Against Xenia (three body patches x twelve tracks, mean display difference): baseline 46.6, PRT 46.6 to 47.8: better on Road Atlanta / Nurburgring / Silverstone / New York rear panels, worse on Maple Valley and Sunset; the game's own shadow term on the car (what makes Xenia's cars near black on those) is the missing piece. Also found: the body shader's reflection is cube rgb x (1 + cube alpha x 12) (`FH1_FM2_CUBE_MUL`, with PRT only).
2. **Road shadows: partly.** Xenia's car shadow on Test Track is the track's ShadowColor in display space (0.31 of the lit road); the engine applied it linearly. `FH1_RM_FM2_SHADOW_GAMMA=2.2` fixes Test Track (default there only) and makes the other eleven worse, so it is a per-circuit INFERRED setting. Road-band score 8.51 -> 7.15. Dark, sharp shadows on the other circuits: not solved (the game's dynamic sun direction is unknown; the sky disc's low sun fits Test Track's length but worsens the numbers).
3. **Car cube: done.** A real 256 x 256 x 6 cube texture with mips (`car_cube_full.bin`, `import-fm2 --only cubes`).
4. **Sun glow card and disc: decoded, drawn opt-in, not verifiable.** Xenos format 10 (8_8) now decodes (G = the flare); the card draws with `FH1_FM2_GLOW=1`; the sun is in none of the 12 Xenia shots (behind a hill, a banner, or the top edge), so its level stays INFERRED. In our own wide-angle shot the disc lands exactly on the logged pixel. Mugello's disc (sky draw 0) is not drawn.
Still open from section 0a: the car's shadow term (see 1), long dark car shadows on eleven circuits, Tsukuba's clouds, the start marker, Nurburgring's gloss, the pylon.

## 0. FM2 UI work (2026-10-26, after this summary was written)
FM2's HUD, minimap and pause screen are drawn from the disc's own UI scenes (details and what is guessed: MODLOG entries 35 to 39). Compared with the Xenia shots the HUD layout, tach, boost gauge and minimap match closely; the 3D scene is covered in section 0a above. The UI work is committed (commits 0f62149, cd619cb, 88d2a47, 3989c1c on branch `ui-elements`); the 3D work of 2026-10-27 is not.

## 1. What this project is

A new PC engine, written in Rust with the Bevy game framework, that plays **Forza Horizon 1** (Xbox 360, 2012) from the user's own disc, and can add content from other Xbox 360 Forza games the user owns: Forza Horizon 2, Forza Motorsport 4, 3, 2, and cars from the original Xbox Forza Motorsport (FM1). It is a reimplementation, not an emulator: the setup tool reads the disc's archives, models, textures, database and audio and converts them; the engine is rebuilt system by system from the game's own data and from reverse-engineering research.

Ground rules (AGENTS.md) that shape everything below:
- No game files, extracted data or decompiled code in the repository. Converted data lives in `data/installations/<id>/assets/private/` (git-ignored). `.gitignore` is a whitelist.
- Game installs are read only. Single-player and offline work only (online free roam exists, see section 3, but this work never touches anti-cheat or DRM).
- Prefer the game's own data over guesses; "log, don't look" (the assistant cannot see the game, so it measures from logs and screenshots); say "not tested" when something is not tested; commit only when asked.

## 2. How it is put together

| Crate | Job |
|---|---|
| `fh1setup` | The importer. FH1 disc -> asset groups (cars, world, scenery, shaders, audio, events, AI lines, remaster tables...); plus `import-fh2`, `import-fm4`, `import-fm3`, `import-fm2`, `import-fm1` for the other games |
| `fh1-formats` | File parsers: Forza zips (XMemCompress LZX), Xbox 360 textures, car models, scenery, PVS (which models are visible from where), FM2 / FM1 formats |
| `fh1-engine` | The game: car physics, cameras, world streaming, races, AI, traffic, HUD, menus |
| `fh1-remaster` | The PBR renderer (physically based lighting): materials, shadows, sky, car paint, post-processing; also draws the imported FM2 scenery with its lightmaps |
| `fh1-render`, `fh1-shaders` | Shared render pieces (particles, quality presets, the car shadows) and the Xbox 360 shader -> WGSL translator |
| `fh1-world` | Track collision meshes and surface types (asphalt, grass, dirt... with the game's grip values) |
| `fh1-audio`, `fh1-radio`, `fh1-video`, `fh1-ui` | Engine sound from the game's FMOD banks, the radio, intro movies, string tables / fonts / HUD scenes |
| `fh1-net` | Online protocol, dedicated server, server list |
| `fh1-launcher` | Installer and launcher (`FH1 Rewrite.exe`) |

Data flow: disc -> `fh1setup` -> `data/installations/<id>/assets/private/...` (imported games under `imported/<game>/`) -> `fh1-engine`. Coordinates: the engine mirrors z against the game files (a file point z is -z in the engine); every importer-to-engine path has to flip it.

Branches (cleaned up 2026-10-26): everything lives on `ui-elements`, which now contains `fm2-lightmaps` (history joined; its tree was already here), `launcher-fm2-fm1`, `festival-venues`, `gpu-selector`, `readme-build-fix`, `fm3-import`, `title-screen`, `camera-dolly-flag` and `controller-deadzones`. `ui-elements` is ahead of `main` and has not been pushed. The old FM2 vertex-shader-scale handoff (`FM2_HANDOFF.md`, question since answered by the per-draw uv work) is kept in git history, commit `429d666`.

## 3. What works (FH1 itself)

From README.md, **not re-checked in these sessions**: the full Colorado map with streaming scenery, props, crowds and traffic; 175 cars with their own handling data, engine sounds and the radio; races with AI drivers, wristband progression and credits; a garage with paint, rims, body kits and upgrades; the remaster renderer with time of day; online free roam on dedicated servers (up to 24 players); graphics presets down to a "Console (720p)" preset for integrated graphics; Windows release builds and an experimental Linux build.

Checked in these sessions, only as a regression test: a Colorado career race (Recaro Rush, 7 AI drivers) starts, runs and finishes (STATUS updates 29 and 32).

## 4. Other games' content

Folder counts in the current install (`imported/<game>/cars` entries; not all are cars, e.g. `index.json`): FH2 222, FM4 506, FM3 420, FM2 311, FM1 232. README's table: FH2 216 cars and its open world; FM4 501 cars and 83 circuit layouts; FM3 311 cars (415 with disc 2) and 52 to 69 layouts; FM2 32 circuit layouts. README still says FM2 cars are "not converted yet"; that is out of date: FM2 cars are imported (`fh1setup/src/fm2_cars.rs`) and the AI raced in them (updates 27 to 32). FM1 cars (231, original Xbox) come from `import-fm1`; their state is in the memory notes and was not re-checked here (paint, glass and lamps work; suspension / aero / brakes come from a donor car; no FM1 engine sounds).

## 5. FM2 tracks: making them look like the real game

Goal: FM2 circuits should look like the user's Xenia screenshots, using the game's own data. Measure: on 12 tracks, the brightness of two road patches near the car (left / right band), ours divided by Xenia; the score is the sum over all 24 patches of |ln(ratio)| (0 = perfect). It went **17.68 -> 9.38**.

Fixed, from the game's own code and data (details in the "STATUS: FM2 road brightness" section below):
- Lightmap crop (a negative tile scale is the shader's clamp flag, not a mirror) and lightmap coordinates read through each draw's own vertex shader (seams on flat ground 0.46 -> 0.10).
- A per-family table saying which textures the console decodes as sRGB, keyed by a hash of the road shader; the importer bakes accordingly and marks materials for the engine.
- The game's shadow rule on lightmap-lit surfaces; the car's own shadow drawn twice like the game (sun shadow plus a projected decal towards the track's ShadowColor); FM2's own follow camera; the real-time shadow cascades.
- Bugs: importer passes overwrote each other's generated textures; the engine cached each scenery texture in one colour space only.

Not solved: Test Track's right band is still 5.2x too bright and Maple Valley's 1.7x; New York's game shader evaluates 2.4x brighter than Xenia (unknown why); a real-time shadow in Laguna Seca's foreground that our sun does not cast; FM2's final colour step cannot be reproduced. The switch table and the test procedure (`shoot_some.sh` + `cmp12.py`, sum near 9.4) are in that section.

## 6. Racing on imported circuits (FM2 / FM4), built 2026-10-26

Before: the engine could only race on Colorado (AI lines came from FH1's own files). Now:

- **AI line** (`ai/line.rs`): the importer's `imported/<map>/ai/line.json` (the game's own optimal line, the centre line and wall distances) becomes the AI's racing line; the rebuilt points match the file within 0.005 to 0.09 m on average on five tracks (test `imported_line_matches_file`).
- **A real race through the same race code as Colorado's events** (`race.rs` `Events::load_imported`): grid from the track's own grid slots, lap gates from `track.json` (two sectors and the finish), 7 of that game's own cars with their real names, countdown, laps, positions, results. Start it with `FH1_RACE=0` (`FH1_RACE_LAPS=n`, default 3); it is not in the menus yet. Test `imported_race_definition` (gates in order, grid behind the line).
- **Dev race** `FH1_AI_RACE=<route>:<cars>:<car>:<laps>`: AI only, with lap counting through the finish gate.
- **AI stay on the tarmac** (`FH1_AI_ROAD`, on by default): an imported line's road width is measured from the collision surfaces (where the game's OffRoadness says dirt / grass starts) instead of wall to wall (Laguna median half width 16.2 -> 5.8 m). AI time off the tarmac fell 95 to 99 %.
- **Fixes found on the way**: AI on FM4's Bernese Alps were reset every physics tick below y = -50 m (a Colorado constant; the limit now follows the line's lowest point); FH1's "keep 2 m from the inside edge" rule bent the game's own apex line once the road was the real tarmac and cost 6 to 8 % lap time (imported lines now skip it); AI crawled into a stopped car and waited to be reset instead of passing it (now they aim at the gap beside it).
- **Measured**: one AI car alone on Laguna Seca 120.2 s, past a parked car 121.3 s (was 134.4 s); 7 AI, 1 lap, parked player: 116 to 169 s (was 151 to 176 s); FM4 Catalunya solo 160.1 s; all 5 AI finish on Catalunya and the Bernese Alps; frame time with 7 AI 6.9 ms on average at a 144 Hz cap.
- **Logging for checking races without looking**: `FH1_RACE_LOG=1` (every gate crossing, results table), `FH1_AI_LOG=1` (every 5 s per AI car: lap, position on the line, offset, speed, resets, grass excursions, and pace = speed against planned and target speed), `FH1_AI_TRAFFIC_LOG=1` (each pass-or-follow decision and each speed cap from a car ahead).

Not solved / not tested here:
- A race **driven by a person**: never tested (every test used a parked, auto-finished player). The F6 event list and the return to free roam after the results: not tested.
- A car alone is still 7 % (Laguna) to 11 % (Catalunya) slower than its own predicted lap; two cars in the 7-car Laguna race still had incidents with resets.
- Per-event lap counts and AI difficulty from the FM2 / FM4 databases are not read (3 laps and FH1's medium AI are used). Passing a stopped car in a corner: not tested.

## 7. Mistakes recorded (so they are not repeated)

- Update 27 claimed the dev race used the track's grid slots; it did not (the spawn list holds display labels "Grid 00", the filter looked for "grid_"). Corrected in update 29; every earlier dev-race lap time was from a line-based grid.
- Update 30's first comparison of `FH1_AI_ROAD` on / off was invalid: both runs stalled behind the parked player for different lengths of time. Re-measured on a clean lap.
- Update 31: the first suspect (the 0.4 m passing clamp) was measured and ruled out (it would have moved 0 points) before the real cause (the 2 m inside-edge rule) was found.
- Earlier FM2 work: several wrong inferences (speed FOV, DropShadowColor vs ShadowColor, a "3x slab", a static shadow map) were corrected in place in the STATUS section below.

## 8. What I would do next (pick one)

1. A race you drive yourself on Laguna Seca (`FH1_RACE=0 FH1_RACE_LAPS=1 FH1_RACE_LOG=1 ./target/release/fh1-engine.exe --track fm2/lagunaseca_00`), telling me what looks wrong; then the F6 list and the return to free roam.
2. Why a lone AI car is 7 to 11 % slower than its own plan (it reaches about 75 to 80 % of its target speed on average: grip, gearing or braking points).
3. Put imported races in the menus (touches the user's UI branch work, so ask first).
4. Back to FM2 brightness: Test Track's right band, Maple Valley, New York.
5. (Done 2026-10-26: README updated for FM2 cars, FM1 and the imported-race note.)

## 9. Where things are

- Plans: `docs/DESIGN.md` (git-ignored, not committed); change log: `MODLOG.md`; this file for status and evidence.
- Test logs and scratch scripts: `data/work/` (git-ignored), e.g. `race*_*.log`, `cmp12.py`, `shoot_some.sh`, `line_surf.py`, `surf_across.py`.
- Engine build used for all tests: `cargo build --release -p fh1-engine --no-default-features` (the default `rtx` feature needs the DLSS SDK). Use the **debug** importer `target/debug/fh1setup.exe` for FM2 imports (an old release build overwrote Laguna once).

---

# STATUS: FM2 road brightness, plain-language summary (updated 2026-10-26)

This note is the short version. The long, dated sections below are the evidence and the dead ends (newest at the bottom of the file).

## Where it stands
Twelve FM2 tracks are compared with your Xenia screenshots on two road patches each (left / right band near the car, linear green, ours / Xenia; 1.0 = equal). The sum of |ln ratio| over all 24 patches (0 = perfect) went **17.68 -> 9.38** over this work. Current (left | right): Tsukuba 1.03 | 1.33, Test Track 1.22 | 5.23, Sunset 0.38 | 0.79, Silverstone 0.89 | 1.09, Sebring 0.64 | 1.11, Road Atlanta 1.44 | 1.00, Nurburgring 1.26 | 0.29, New York 0.56 | 0.84, Mugello 0.98 | 1.63, Maple Valley 0.51 | 1.72, Laguna Seca 0.94 | 1.09, Suzuka 0.47 | 0.94. Ratios differ by about 0.2 between runs of the same data (sun / exposure), so smaller changes are noise. Everything is committed (except `docs/DESIGN.md`, which the whitelist `.gitignore` excludes) as of 2026-10-26.

## What was fixed (all from the game's own code and data)
- **Lightmap crop:** a negative tile scale is the shader's clamp flag, not a mirror (`|c170|` in the road shaders; tile centred in its 48-texel page cell).
- **Lightmap uv:** read through each draw's own vertex shader instead of a guessed offset / unorm decode. Lightmap seams on flat ground dropped from 0.46 to 0.10 (Test Track); baked shadows now sit where Xenia has them.
- **Per-family gamma:** a table of road-shader families (by a hash of the shader code) says which textures the console decodes as sRGB: BC4 grains (Nurburgring, Tsukuba, Silverstone, Test Track, Suzuka) or all colour textures (Laguna, Sunset, Sebring slab). The importer bakes / scales accordingly and marks materials so the engine uploads their colour layers as sRGB.
- **The car's own shadow** is drawn twice like the game does: in the sun shadow (road shader rule) and by a projected car-shadow decal that multiplies the ground towards the track's `ShadowColor` (the game's `RenderProjShadow`; its colour source traced to `CTrackPresentation` +0x5f0).
- **Game shadow rule** on lightmap-lit surfaces (`min(1 - (1 - ShadowColor) S, lightmap light)`), the **car contact shadow** for FM2's glTF cars, **FM2's own follow-camera** settings, the **real-time shadow cascades** read from the road shaders (no static shadow map exists).
- **Bugs:** importer passes overwrote each other's generated textures (shared id ranges); the engine cached each scenery texture in one colour space only.

## What is NOT solved
- **Right-hand bands** still too bright on Test Track (5.2x) and Maple Valley (1.7x): darker shadow areas in Xenia that the double car shadow does not fully reach.
- **New York:** the engine is close (0.76) only because an old importer heuristic darkens it; the game's own shader evaluates 2.4x brighter than Xenia there, cause unknown (not the post-process).
- **Laguna Seca's foreground** is in a real-time shadow in Xenia that our sun does not produce; caster unknown.
- **FM2's final colour step** (`FH1_FM2_POST`) cannot be reproduced: the engine's frame average stays below every track's adaptation limits.
- Tested on 2026-10-26: a 40 s automatic drive on Test Track and Laguna Seca (no errors, frame time 6.94 ms mean either way, car shadow follows the car on dirt and grass). AI races on FM2 circuits now work through the dev hook `FH1_AI_RACE` (update 27: Laguna Seca 2 laps and Test Track 1 lap, 5 AI cars, all finish, no resets, 60 fps cap held). Not tested: a race through the game's menus (the event list does not offer FM2 circuits), the player in that race.

## Decided: no Xenia GPU trace
It does not work in your Xenia build (tried 2026-10-19). Questions that would need it stay empirical.

## Switches (FM2 lighting work; importer `fh1setup import-fm2`, engine `fh1-engine`)
| Switch | Where | Default | What it does / status |
|---|---|---|---|
| `FM2_BC4_SRGB=0` | importer | on | turns the whole per-family gamma table off (old output) |
| `FM2_BC4_SRGB_ALL=1` | importer | off | BC4 rule on every baked road (tested 2026-10-26: worse) |
| `FM2_FAMILY_COLOUR=1` | importer | off | scale every family colour texture to its shader's brightness (worse; only `FAMILY_SCALE` families get it always) |
| `FM2_SHADER_ROAD=1` | importer | off | shader albedo for baked roads without the table |
| `FM2_LM_MIRROR=1` | importer | off | old mirrored lightmap crop |
| `FM2_LM_UV_OLD=1` | importer | off | old guessed lightmap uv |
| `FM2_SPEC_MAT=1` | importer | off | per-draw specular term into the materials (use with `FH1_FM2_SPEC_MAT`) |
| `FM2_TRACKS=a,b` | importer | all | import only these tracks |
| `--only camera` | importer | - | copy FM2's `CameraSettings.ini` |
| `FM2_LM_SEAMS=1` | importer | off | diagnostic: lightmap jumps across tile seams (all / flat ground) |
| `FM2_PROBE_AT=x,y,z;...` + `FM2_PROBE_LERP=d,l` | importer | off | diagnostic: run each draw's shader at a world point (patch check) |
| `FM2_NEAR`, `FM2_FIND_TEX`, `FM2_FIND_SIG`, `FM2_UNDER`, `FM2_ROAD_DBG` | importer | off | diagnostics (draws near a point / using a texture / shader hash) |
| `FH1_FM2_SHADOW_GAME=0` | engine | on | fixed shadow strength instead of the game's rule |
| `FH1_FM2_DROPSHADOW=0` | engine | on | no car contact shadow on FM2 cars |
| `FH1_FM2_CAR_SHADOW=0` | engine | on | no projected car shadow (the second, ShadowColor darkening of the car's shadow) |
| `FH1_FM2_CAMERA=0` | engine | on | FH1's follow camera instead of FM2's |
| `FH1_FM2_CAMERA_FOV=deg` | engine | unset | force the follow camera's FOV |
| `FH1_FM2_POST=1` (`FH1_FM2_POST_T`) | engine | off | FM2's final colour step (worse) |
| `FH1_FM2_SPEC=k`, `FH1_FM2_SPEC_MAT=1` | engine | off | game-derived road specular (mixed results) |
| `FH1_RM_FM2_SHADOW_DIR=full/fz/fx/fxz` | engine | off | track shadow direction for the sun (worse) |
| `FH1_RM_FM2_SHADOW_DBG=1` | engine | off | debug: paint shadow visibility and lightmap light |
| `FH1_SHOT_PROBE=1` | engine | off | log the ground points under the comparison bands at the screenshot |
| `PS_GAMMA`, `PS_WHITE`, `PS_REGS` | `fm2_ps_run` probe | - | per-sampler sRGB / white samplers / register dump |
Removed on 2026-10-26 (rejected or superseded): `FM2_LM_UV_SIGNED`, `FM2_LM_UV_VS`, `FM2_LM_FLIPU`, `FM2_LM_FLIPV`, `FH1_RM_FM2_DYN_LIT`, `FH1_RM_FM2_SHADOW_POW`, `FM2_RM_COLOUR_SRGB`. The many older `FM2_NO_*` / `FM2_ROAD_*` importer switches predate this work and are documented next to their code.

## How to test
```
./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps
./target/debug/fh1setup.exe data/work/disc --only remaster --force
bash data/work/shoot_some.sh check tsukuba_00 testtrackinfield_00 sunsetinfield_00 silverstone_00 sebring_00 roadatlanta_00 nurburgring_00 newyork_01 mugello_00 maplevalley_00 lagunaseca_00 suzuka_00
python data/work/cmp12.py check
```
The last line should print a sum near 9.4.

---

# STATUS: Test Track pale road (FM2), written 2026-10-17

Written because two real attempts at this problem did not work (AGENTS.md rule), instead of trying more variations at random.

## What we want

Test Track's road should look like the Xenia shot: mid-dark grey asphalt with the yellow lines and the chequer. Ours (the engine's `fm2/testtrackinfield_00`) is pale beige.

## What is known (verified)

- The pale surface is not drawn from a colour texture. Painting texture 53 a solid colour turns the whole road that colour: 53 is the draws' baked lightmap tile. So the road is "baked road colour x lightmap".
- The road draws that matter are the ones with pixel-shader constants `c0` = 1, `c1` = -0.05, `c2` = 0.1, `c3` = 0.6, `c4` = 0.4, `c5` = 2.5 (for example model 321), and the ones with `c0` = 0, `c1` = 1.2 (model 16, 43, 516, 546).
- The importer treats `c1` as a colour for the second family; the shader uses `c1..c5` as palette gains and offsets (`v*c5`, `v*c4 + c3`, `v*c2`, `v*c1 - v*c2`) on a grain texture sample, picked between by a mask's luma.
- The final colour is `albedo * lerp(c184, c185, lightmap) + sun term`, then a fog lerp. `c184` / `c185` are the track's dark and light lightmap colours (the importer already applies this lerp).
- Items of a draw map onto pixel-shader samplers by position (item k feeds sampler k); items 12 and 13 are shared shadow inputs with no texture.

## What was tried

1. Read the shader by hand (`fm2_show`): the swizzles make it error-prone; no firm formula came out.
2. Wrote a CPU interpreter for the pixel shader (`crates/fh1setup/examples/fm2_ps_run.rs`, semantics ported from `crates/fh1-shaders/src/wgsl.rs`). It runs and reproduces the structure above, but its numbers are not trustworthy yet:
   - Test Track model 321 (`c0` = 1): albedo 0.54 with a fully lit lightmap, i.e. pale, which agrees with what we see but not with Xenia.
   - Tsukuba model 50 (`c0` = 1, `c1` = 0), a road we know looks right: the interpreter outputs 0 (black). So the interpreter or its inputs are wrong somewhere (which group belongs to which draw, interpolator values, `c168..c171` atlas constants, `c253..c255`, a bool jump), and it cannot be used to decide the road colour until it reproduces a known-good road.
3. Tested a gamma theory: stop gamma-encoding the baked road grain (`FM2_ROAD_ENC=1.0`). Result: Tsukuba and Suzuka roads went near-black (they were right before) and Test Track's did not change, so the theory is wrong, and Test Track's road is not drawn from the baked-road texture at all. The knob is still in the code with its default (1/2.2).

## Not known

- Which draw actually paints the Test Track road under the car (several candidates; the colour-by-texture painting only found the lightmap).
- Why the interpreter gives 0 for Tsukuba.

## Update after option 1 (the interpreter route)

- The interpreter now works once the fog factor register is set to 1 (it differs per shader: `r7.x` on Tsukuba's road shader, `r8.x` on Test Track's). Results with a fully lit lightmap and no sun: Tsukuba road 0.375, Test Track road 0.54.
- The importer's baked road albedo agrees with both (about 0.34 and 0.57). So the importer's road colour is NOT the cause of the pale Test Track road.
- The lightmap tiles are not the cause either (road tile means: Tsukuba 0.45, Test Track 0.47).
- Tried in the engine: doing the lightmap product in gamma space (`FH1_RM_FM2_GAMMA_LM=1`, off by default): made Test Track warmer and more saturated, not greyer, and Laguna Seca yellower. Not adopted.
- Tried a neutral light colour for Test Track (edited `fx_globals.json`, restored afterwards): the road went near-white, so the warm light colour is not the main cause either. Still much brighter than Xenia (about 215 against about 115 per channel).
- Open: the engine renders Test Track's lit road far brighter than the shader's own numbers suggest. Candidates: the sun term the shader adds on top of the lightmap (the interpreter shows it roughly doubles the output), exposure or grading differences, the lightmap crop values. Not checked.

## Suggested next steps (pick one)

- Make the interpreter reproduce Tsukuba model 50's known-good road first (check the group/draw pairing, interpolators and the `c168..c171` constants), then use it on Test Track.
- Or find the exact draw under the car by hiding draws one at a time (the `FM2_HIDE_MODEL` idea) and read only that draw's shader.
- Or set Test Track's road colour from the Xenia shot (a guess, which AGENTS.md prefers against).

## State of the code

- Nothing here is committed. Importer: `crates/fh1setup/src/fm2_scenery.rs` has the uncommitted Suzuka road gain, the narrowed wear-invert rule and the `FM2_ROAD_ENC` knob (default unchanged). New probe: `crates/fh1setup/examples/fm2_ps_run.rs`.
- The installed FM2 data was re-imported with the defaults after the gamma experiment (check `data/work/reimport_done.txt`).

## Update 2026-10-19: per-track road brightness (Sunset Infield and others), two attempts, not solved

Attempt 1, stage readouts (`FH1_RM_FM2_DBG` modes 1-4, flat-value calibration modes 5-9, Sunset Infield road band):
- Display curve of the engine (flat engine value -> screenshot pixel): 0.1 -> 63, 0.2 -> 101, 0.3 -> 131, 0.4 -> 154, 0.5 -> 172.
- Albedo reads ~0.34 (road texture averages ~0.36), lightmap term ~0.37 (matches the shader's dark/light colours), product ~0.126 = the normal render (pixel 73). Floor, exposure, sheen and shadow change nothing here.
- Xenia's pixel (104) needs ~0.2 on the same curve. So every engine stage agrees with its own formula; the gap is not a bug in the engine stages.

Attempt 2, sun-term hypothesis (the shader adds a sun term on top of albedo x lightmap): compared each track's sun height x LightColor (from TrackSettings.xml) with the measured ours/Xenia ratio. No correlation: Sunset 0.74 -> ratio 0.43, Suzuka 0.72 -> 2.06, Laguna 0.74 -> 1.36, Mugello 0.58 -> 0.56. A shared sun term does not explain the spread.

Not known: where the per-track difference comes from. Untested candidates: (a) each track's road shader family needs its own albedo (Sunset model 77 samples a one-channel texture through tf2 and several other textures; the importer's road colour may not match), (b) the Xenia screenshot's own tonemap / colour grade differs per track (sky/exposure), (c) the interpreter's registers for the sun (c101, c105, c108, c109) were never identified, so the sun term was not computed from the shader.
Next step if continued: identify c101/c105/c108/c109 from the FX globals, run the interpreter on Sunset model 77 group 0 with them, and compare the oC0 value with the engine's 0.126.
Nothing committed or release-built.

## Update 2026-10-19 (later): interpreter on Sunset's road draw

Ran `fm2_ps_run` on Sunset Infield model 77, **group 2** (the road draw with texture 252; group 0 is a different shader), draw 3, with the track's real lightmap colours (c184/c185 from fx_globals.json), fog factor r9.x = 1, c104 = 1:
- Shader output: mean (0.166, 0.158, 0.133) = sRGB (113, 110, 102). Xenia's road reads (111, 110, 98). So the shader with real lightmap inputs reproduces Xenia; the importer's albedo and the lightmap are not the problem.
- c101 (direction) and c102 (sun colour) changed nothing in this run (their normal inputs were zero in the probe), so the sun term is still not exercised. Not tested.
- The engine pipeline gives 0.126 linear, but the game's colour grade (FH1's Hable curve + LUT + vignette, `crates/fh1-remaster/src/post.rs`) maps it to pixel 73. With `FH1_RM_TONEMAP=none FH1_RM_LUT=0 FH1_RM_VIGNETTE=0` Sunset reads 96 (Xenia 111).
- Not a global fix: with the grade off, linear ratio ours/Xenia (road bands): Tsukuba 1.05 -> 1.8, Test Track 0.62 -> 1.0, Sunset 0.36 -> 0.74, Sebring 0.38 -> 0.55, Road Atlanta 1.32 -> 1.61, Nurburgring 2.26 -> 3.5. (Silverstone is the black-patch band; New York shot failed because the layout is newyork_01; Mugello onward not computed.) So the grade explains the dark tracks (Sunset, Test Track, Sebring) but worsens the already-bright ones: those need their own albedo/exposure fix.
- Next step if continued: give FM2 tracks their own display grade (identity-like) and re-fit the bright tracks (Nurburgring, Road Atlanta, Tsukuba) individually.

## Update 2026-10-19: Nurburgring road too bright (3.5x Xenia), not solved

Facts:
- Road draws near the grid: model 118 (draw 0, group 0: lightmap tile 122 flags 12, textures 123/124/126/127, mask 125) plus model 261 (2-triangle patch) and many model 151 draws without lightmap (not identified which of them is under the camera: `FM2_UNDER` found nothing at heights 547..551.4 step 0.8 at 2011.5,-1910.3; frame or tolerance not understood).
- Interpreter on model 118 group 0 draw 0 with the real lightmap (c170 = 1,1,0,0, lightmap uv r4 swept over the tile, fog r7.x = 1, c184/185 = DarkColor/LightColor = 0.079 / 1.105, c104 = 1): road = 0.21 linear = sRGB 125. The engine (grade off) renders 123. So engine == shader here; Xenia's pixel is 70 (0.06 linear), 3.5x darker.
- Earlier interpreter runs left the lightmap uv register at 0 (reading the tile's corner texel) and c170/c171 at 0. Re-run on Sunset with the grid (`/tmp/lmgrid.sh`-style sweep: r7 = lightmap uv, c171 = 1,1,0,0): 105,102,93 vs Xenia 111,110,98 (the earlier 113,110,102 was luck).
- Tile mean lightmap value 0.36; TrackSettings `TrackLightMapTransition MidPoint 0.36 Width 0.15 Color 0.45` for Nurburgring (Sunset 0.55 / 0.11 / 0.51, Test Track 0.64 / 0.15 / 0.35). The shader contains a hard step on the lit term at 0.45 (c253.w) times 10; that only gates the sun term. Not tested: whether Xenia's lightmap values are lower than our tile crop (wrong tile / page for this draw), i.e. the road could be sampled from a darker region in the original.

Not known: why Xenia's Nurburgring road is 3.5x darker than the shader with our inputs. Candidates: the road under the camera is a different draw (model 151, no lightmap), our lightmap tile for model 118 is wrong, or the Xenia run has different lighting state.
Next step if continued: find the draw under the camera by painting every texture a solid colour (the trick in DESIGN.md) at the spawn view, then run the interpreter on that draw.

## Update 2026-10-19: Nurburgring painting test

Painted textures 122 (red), 123 (green), 186-192, 284, others solid colours in the installed Nurburgring data (backed up to `data/work/nb_tex_backup`, restored and verified by md5; materials.json restored too), shot the grid view:
- The whole road, including under the car and ahead, is drawn from **texture 123** (512x1024 BC1 colour) = model 118 draw 0, the draw the interpreter was run on. The chequer strip is texture 284 (white). The red edge lines are 122. The shot with everything painted shows no other texture on the road.
- Grass at the right of the view matches Xenia (ours 104,104,80 vs Xenia 102,105,78), so the lighting, lightmap colours and display path are right in general. Only the road is 3.5x too bright.
- So the road shader (4 colour/mask textures + mask 125) gives, in our interpreter and engine, 0.21 linear but Xenia shows 0.06. Untested causes: shader constants other than c0/c1 (c2..c8 are not in the draw's list; the interpreter leaves them 0; the engine's importer ignores them), sampler-to-texture mapping for tf3..tf7 (which of 123/124/125/126/127 is the grain, mask, colour), a gamma decode of 123 in Xenia (the .bix word says linear: 0x1a200152; forcing the gamma flag in materials.json changed nothing because the FM2 path does not read it at runtime; not tested through a remaster rebuild).
Next step if continued: print the shader's intermediate registers for this draw and check which texture drives the final colour (`PS_REGS=1`, tf3..tf7 one at a time set to 0 / 1) to see which sample must be darker.

## Update 2026-10-19: Nurburgring road shader, sampler sensitivity (interpreter, model 118 group 0 draw 0)

Replaced each sampler by an all-white or all-black texture and read the road mean (baseline 0.211 linear = sRGB 125):
- tf3 (texture 123, the BC1 colour): white 0.100, black 0.284. Its luminance is a blend weight, not the colour (shader: `dp3 lum(tf3)`, then `lerp(tf5, tf4, lum)`).
- tf4 (124): white 0.531, black 0.170. tf5 (125): white 0.581, black 0.040. These two grey BC4 textures are the real road colours (mean value 124 ~0.11, 125 ~0.35).
- tf6 (126) and tf7 (127): no effect on the colour (white and black both 0.211).
So the road albedo is lerp(125, 124, luminance of 123); the importer/engine treats 123 as the colour texture and 124/125 as masks. Engine and interpreter still agree (123 vs 125), so the importer's baked colour is close to the shader's, and Xenia's darker road (0.06) is not explained by the albedo mix. Not tested: gamma decode of the grey BC4 textures in Xenia (would give about 56 instead of 125; Sunset's baked road matched without it, so not adopted).
Stopping here per the two-attempts rule.

## Update 2026-10-19: Nurburgring road, shader read and gamma test (interpreter, model 118 group 0 draw 0)

The interpreter source was missing from this checkout (it lived uncommitted in `fh1_rewrite_fm3`); copied `ps_cpu.rs` and `fm2_ps_run.rs` here (plus `pub mod ps_cpu;` in `fh1-shaders/src/lib.rs`). Reproduced the 0.211 / sRGB 125 baseline with `/tmp/lmgrid.sh` (args: fog reg 7, lightmap uv reg 4, c170).

What the pixel shader does (read from the disassembly):
- albedo A = lerp(tf5, tf4, lum(tf3)) (tf3 = 123, tf4 = 124, tf5 = 125; `dp3` with the luma literal 0.114/0.299/0.587).
- light = 2 * min(lerp(c184, c185, lightmap tf2), 1 - (1-c105)*shadow); the x2 is real (`add r3, r3, r3`).
- out = fog lerp (r7.x, c114) of A*light + sun term (c102 colour). tf6 and tf7 only feed a sun/spec term.
- Check: at lightmap value 0.618 the output is 0.255, so A is about 0.21; at lightmap 0.079 it is 0.033 (A about 0.41 x ... consistent).

Sample that must be darker: **tf4 and tf5 (textures 124 and 125, the grey BC4 road colours)**. Probe-only switch `PS_GAMMA=<sampler list>` (sRGB decode of those samplers' colour):
- none: 0.211 (sRGB 125) | tf3 only: 0.258 (137) | tf4,tf5: 0.049 (64) | tf3,tf4,tf5: 0.066 (73). Xenia: pixel 70 = 0.058 linear.
- So gamma-decoding the road textures lands on Xenia (73 vs 70) for Nurburgring.
- BUT the same switch on Sunset (model 77 group 2 draw 3, textures 249/93/94/252) gives 0 (black) with gamma, while no gamma gives 105,102,93 against Xenia 111,110,98. So gamma is not a global rule: some textures/draws are gamma-decoded on the console and others are not.

Where that could be stored (not found): the `.bix` header word (dword 1: 0x1a20017b for BC4, 0x1a200152 for BC1) has no gamma bit, and it is identical in kind for Nurburgring 124/125 and Sunset 94. The Xenos puts the sign/gamma in fetch-constant dword 0, which the game sets at run time (so it may come from the draw or material data, not the texture file). Not tested: any other source.
Not tested: the engine (nothing changed in the engine or importer), Sunset with gamma on only one sampler, other tracks.
Next step if continued: find what sets gamma per draw (compare the draw/material records of Nurburgring 118 draw 0 against Sunset 77 group 2), or try gamma on single samplers for Sunset (tf2 / tf4 / tf5 separately).

## Update 2026-10-19 (later): what sets gamma per draw? Not the draw records; the texture format fits best

New probe `crates/fh1setup/examples/fm2_bind_dump.rs` (prints stream records) and `fm2_ps_run` now also prints each draw's state records.

Draw records compared (Nurburgring model 118 draw 0 vs Sunset model 77 draw 3):
- State records (`0x05`/`0x08`/`0x0c`): cull/render state only, nothing texture related.
- Texture binding record `0x2d`: each binding is `item, (slot<<16 | flag)`. Flag 1 = lightmap, 2 = colour/grey texture, 3 appears on Sunset's grey BC4 texture 94. Nurburgring's grey BC4 textures 124/125 have flag 2, exactly like Sunset's BC1 colour texture 252. So this flag does not separate "gamma" from "linear". The PVS flags (4/12/20) and the `.bix` format word do not either (checked, all alike).
- Result: no per-draw or per-binding gamma bit found in the files we parse. Not tested: the fetch-constant dword 0 itself (not stored in anything we read).

Single-sampler gamma tests (`PS_GAMMA`, interpreter, sRGB value of the road):
- Sunset (Xenia 111,110,98; no gamma 105,102,93): gamma on tf5 (252, BC1 colour) -> 43,38,28 (wrong); on tf4 (94, BC4 grey) -> 107,104,96 (still fits); tf2 -> no effect.
- Nurburgring (Xenia ~70): gamma on BC4 tf4 only 115, tf5 only 83, tf4+tf5 64; flag-4 rule (tf3,tf4) 134 (wrong).
- Tsukuba model 50 draw 0 (all four samplers BC4, c170 route, Tsukuba lightmap colours; engine/Xenia ratio earlier 1.8 too bright): no gamma 113; gamma on BC4 samplers 76.

Best-fitting rule (a hypothesis, not proven): **single-channel BC4 (DXT5A) textures are gamma (sRGB) encoded and must be decoded; BC1 colour textures are not.** It fits all three tracks: the too-bright ones (Nurburgring 3.5x, Tsukuba 1.8x) are exactly the roads built from BC4 greys; Sunset's road uses mostly BC1 and was already right. Caveat: the importer comment says "BC4 -> linear" (`crates/fh1setup/src/fm2_scenery.rs` near "Keep the page's fetch word"), and Tsukuba's Xenia value is only inferred from the earlier ratio, not measured on the same pixel. Not tested in the engine: nothing was changed in the importer or engine.
Next step if continued: apply the rule only to the road/ground grain path (decode BC4 grey as sRGB) behind an env switch, reimport one track, and compare the road bands for Nurburgring, Tsukuba, Sunset and Test Track against the Xenia shots.

## Update 2026-10-19 (end): BC4-as-sRGB switch implemented and tested on Nurburgring only

Code: `FM2_BC4_SRGB=1` (off by default) in `crates/fh1setup/src/fm2_scenery.rs`, just before the baked-road loop. For a draw with exactly one BC1/BC2/BC3 colour texture and at least two grey BC4 textures (the first two square, <= 256, same size) it bakes a replacement colour texture: `gain * lerp(mean_sRGBdecode(grey2), mean_sRGBdecode(grey1), luma(colour))`, gain = `FM2_ROAD_GAIN` (2.0, the shader's x2 light term). `FM2_BC4_SRGB_W=1` also sRGB-decodes the luma weight. Greys use their mean (their own tiling uv is not the draw's). Nurburgring: 4 draws affected (colour 0x7b, 0xab, 0xa49 with greys 0x7c / 0x7d).
Measured (release engine, grid view, road bands left and right of the car, median, sRGB / linear G): switch OFF 129,130,130 / 111,111,110 = 0.192; ON 0.042 (61,62,65 / 58,59,61); ON+W 0.0415 (no difference). Xenia road pixel about 70 = 0.0575 (value from the earlier notes, not re-measured on these exact bands). So ours / Xenia: 3.3 -> 0.73 (slightly too dark now, no longer 3.5x too bright).
Not tested: other tracks (Tsukuba and Test Track roads go through the separate constants-only baked path, which this switch does not touch), a real race, the per-draw detail tiling of the greys (the mean is used), the same switch on the full set of tracks, whether the 0.73 remainder is the flat grey means or the missing sun term. The installed Nurburgring data was re-imported with the switch OFF afterwards (the baked `ffff4000..` textures from the test may remain as unused files in `scenery/textures`). Shots: `screenshots/bc4_nb_on.png`, `bc4_nb_onw.png`, `bc4_nb_off.png`. Backup of the pre-test textures/materials: `data/work/nb_import_backup`.

## Update 2026-10-19 (end 2): Test Track contradicts the BC4-as-sRGB rule; extension NOT implemented

Before extending the switch to the constants-only baked-road path, ran the interpreter on Test Track's road draw (model 321 group 0 draw 0: tf3 = 507, tf4 = 11, tf5 = 507, all grey BC4; fog r8.x, lightmap uv r4, c170, Test Track lightmap colours from `fx_globals.json`):
- no gamma: sRGB 133,126,121 (0.21 linear); gamma on tf3,tf4,tf5: 78,74,71 (0.066 linear).
- Xenia's Test Track road is about 115 per channel (earlier notes; the engine's current ratio ours / Xenia is 1.0 with the colour grade off). So no gamma fits Test Track (133 vs 115) and gamma would make it about 0.4x too dark.
- Scoreboard for "BC4 greys are sRGB": Nurburgring yes (matches), Tsukuba plausible (76 vs an inferred ~87), Sunset indifferent, Test Track NO. A format-only rule does not hold, so I did not extend the importer; making Test Track darker on a rule one track contradicts would trade one error for another.
- Not tested: the engine for Tsukuba / Test Track under the switch (nothing changed); Tsukuba's Xenia value was inferred from an earlier ratio, not measured; what differs between Nurburgring's greys (flags 4, 20) and Test Track's (507 flags 4, 11 flags 20) is not known (draw binding flags are 2 for both).
- Open idea: gamma may depend on the texture itself (e.g. which greys are authored as sRGB masks vs data), not its format; compare the pixel content of 124/125 (Nurburgring) against 507/11 (Test Track).

## Update 2026-10-19 (end 3): comparing the grey BC4 textures (pixel content, role, per-sampler gamma)

Probe: `/tmp/bc4stats.py` (BC4 level-0 decode + statistics; scratch, not in the repo). Byte values 0..255:
- Nurburgring 124: 256x256, mean 27, range 1..57, neighbour-difference 5.8. 125: 256x256, mean 85, range 5..193, nbr-diff 22.6. Both dark, narrow, used as the two direct colour endpoints of `lerp(tf5, tf4, luma(tf3))`. (126: mean 123, full range, high contrast; 127: 1024x512 smooth mean 94; these do not feed the colour.)
- Test Track 507: 1024x512, mean 186, range 83..255, smooth (nbr-diff 4.3), bright. 11: 512x512, mean 145, range 0..255, nbr-diff 43 (high-frequency grain). Their shader (c0..c5 palette) feeds them into gain/offset expressions, not as albedo.
- Tsukuba 130 (smooth 1024x512, mean 86), 144 (grain 512x512, mean 87), 145 (mean 21, mostly 0). Sunset 94 (1024x512, mean 25, mostly dark).
No pixel property (mean, range, contrast, size, format, flags, binding flag) separates "needs gamma" from "does not": the Nurburgring greys are not unusual in any measured way.

Per-sampler gamma on the interpreter (road sRGB; Xenia: Nurburgring ~70, Test Track ~115, Tsukuba unknown, inferred ~87):
- Nurburgring: tf4+tf5 (the greys) 64, +tf3 73 -> matches.
- Test Track (tf3 = 507 smooth, tf4 = 11 grain, tf5 = 507 again): none 126; tf3 only 111 (matches ~115); tf4 only 86; tf3+tf4 74; tf5 only no change.
- Tsukuba (tf3 = 130 smooth, tf4 = 144 grain, tf5 = 145, tf6 = 130): none 112; tf4 only 65; tf3 only 129 (brighter); tf5/tf6 no change.
So with "gamma on the sampler that is the main albedo-like input" Nurburgring (tf4, tf5), Test Track (tf3) and Tsukuba (tf4: 65, a bit dark) are each explained by a different sampler, so a per-sampler rule is needed; a format rule is not enough. Which sampler is decoded per shader family is not derived from anything we read.
Not found: where the console stores this (fetch-constant dword 0 is not in any file we parse). Not tested: the engine under any of these, Tsukuba's Xenia value measured directly, Sunset and others per sampler beyond the earlier runs, reading the game's own D3D texture-setup code (the XEX) for the gamma bit.
Next step if continued: look for where the game sets the texture sign/gamma (fetch constant dword 0): check the `.rmb.bin` `.stream` / `.data` for six-dword fetch constants (the `0x2d` records have only item and slot), or read the XEX code path that fills them.

## Update 2026-10-19 (end 4): searching the model files for the texture gamma/sign bit (fetch-constant dword 0): not found

New probe `crates/fh1setup/examples/fm2_data_dump.rs` (hex dump of a model's `.data` / `.gpu`). Looked in every place a per-binding gamma setting could live, for Nurburgring 118, Test Track 321, Tsukuba 50, Sunset 77:
- `.bix` headers: only fetch-constant dword 1 (`1a20017b` BC4, `1a200152` BC1; the same for every texture of the same format); no dword 0.
- PVS texture records: `flags` 4 / 12 / 20 identical for the grey that needs gamma (Nurburgring 124 flags 4, 125 flags 20) and the ones that do not (Test Track 507 flags 4, 11 flags 20).
- Stream records: `0x2d` binding = `item, slot<<16 | flag` (1 lightmap, 2 colour/grey, 3 seen once); `0x05` / `0x08` / `0x0c` state records are plain offsets into `.data` (zero-filled regions at the places they point to in Nurburgring 118); nothing texture-specific.
- The pixel-shader containers in `.data` (header `102a1100`, a D3D constant table): samplers are `s_sampler2..7`, `s_lightMap`, `s_shadowMap*`; every sampler entry points at the same type descriptor (`0x2d4` in Nurburgring), so no per-sampler flag (no sRGB marker) there either.
- Material parameter names in `.data`: `xfcolourMap__<n>_..._colour` (Nurburgring `..._422_colour`, Test Track `..._colour_1`, Tsukuba `..._colour_2`, Sunset `..._colour_0` plus `xfbumpMap__..._bump`). A "colour" vs "bump" suffix exists, but each model lists one colour map name while its draws bind five or six textures, so I could not tie it to the samplers; the values behind the names (the `pool` table, from `0x1690`) are not decoded. Not tested.
Conclusion: no dword 0 / gamma bit exists in the model files as parsed. The game most likely sets texture gamma in its own code (a D3D `SRGBTEXTURE`-style sampler state) from material or shader family, which is not in the data we read. The remaining places to look: decode the `pool` / reflection table after `0x1690` (material parameters) and the game's executable (XEX) code. Not done.
Practical consequence: per-sampler gamma stays a hand decision per shader family (Nurburgring greys yes, Test Track tf3, Tsukuba tf4), tested only in the interpreter, not in the engine.

## Update 2026-10-19 (end 5): decoding the `.data` pool table / material names, gamma still unlocated

- The table after `0x1690` (Nurburgring 118) is a pointer-fixup (relocation) list: runs of `.data` offsets (`0x126c, 0x1270, 0x1278, ...`) and small counts, then pointer slots. It holds no parameter values and no texture or gamma flags. The structure just before it (`0x1200..0x12b0`, a `.data` header with offsets and counts) is not decoded.
- The material parameter names carry a suffix: `xfcolourMap__<id>_colour[_K]` and `xfbumpMap__<id>_bump[_K]`. Across each whole track the suffix is nearly constant: Test Track `colour` (389 models) / `colour_1` (285), Tsukuba `colour_2` (65) / `colour` (32), Sunset `colour_0` (105) / `bump` (131), Nurburgring `colour` (1411) / `<n>_colour` (431; the number differs per model: 10, 4, 2, 422, 400..405 ... so it is an id, not an index).
- Coincidence worth noting, NOT proven: Test Track `_colour_1` and Tsukuba `_colour_2` equal the PVS item index of the texture that the interpreter needed gamma on (Test Track item 1 = texture 507 = tf3; Tsukuba item 2 = texture 144 = tf4). Sunset `_colour_0` is untestable (item 0 is not bound in the probed draw; gamma on tf2 changed nothing). Nurburgring model 118 (`_422_colour`) does not fit, since its needed items are 2 and 3 and 422 is an id. Two matches out of three testable cases with a suffix that is the same for hundreds of models per track is weak evidence.
- Not tested: whether `_colour_K` means "texture K is sRGB" on other models, decoding the `.data` header / relocation targets, the XEX code.
Conclusion: after the texture headers, PVS, stream records, shader container and this table, nothing parsed gives the gamma setting. Two real attempts at the same question have ended without a source, so per AGENTS.md I am stopping this line here.

## Update 2026-10-19 (end 6): BC4-sRGB rule gated to the Nurburgring road shader family

The first version of `FM2_BC4_SRGB` matched by texture mix only and also hit one Sebring draw (model 2006, colour 0xd + greys 0x13/0x14; Sebring is already too dark, so that would have been wrong). Now the draw's pixel-shader microcode is hashed (`DrawUv::ps_sig` in `crates/fh1setup/src/fm2_uv.rs`, FNV-1a over the shader's dwords) and the rule applies only to hashes in `NB_ROAD_SIGS` (`crates/fh1setup/src/fm2_scenery.rs`). Nurburgring's family is `4aa0eec057a0743d` (models 118, 122, 85, 2837); Sebring's draw is `a515f7c7179c2b42`. `FM2_BC4_SRGB_SIG=1` prints every candidate's hash (to add a family later).
Result with the switch on and every track re-imported: only Nurburgring changes (4 draws); every other track prints "0 draws use a baked blend". Nurburgring road band 0.0415 linear (61,61,65 / 58,59,61) against 0.192 off and Xenia ~0.0575 (ours/Xenia 0.72). Sebring shot switch on vs off: mean pixel difference 0.43 (below the 0.3-4.8 noise baseline), so unchanged.
Default is off, and the installed data is back to the default import. Other families (Test Track, Tsukuba) are left alone; they would each need their own entry in `NB_ROAD_SIGS`-style lists and their own sampler choice.
Not tested: a real race on Nurburgring, the remaining 0.72 gap (flat grey means instead of the detail textures, missing sun term), other layouts of the same track.

## Update 2026-10-19 (end 7): is the missing sun term the Nurburgring 0.72 gap? Not decidable yet

Read the Nurburgring road PS tail (model 118 group 0): the sun term is `r2.x = min(r0.w, r5.w)` (a gate times a specular-style power of `dot(normalize(c101 - view), normal)`), times `c102` (light colour), `c0` and `c104` (= `GlobalSpecularLevel`, 1.7 on this track in `TrackSettings.xml`), added after `albedo * light`. The gate contains `step(lightmap > c253.w = 0.45)`, so it only acts in lit lightmap regions.
Interpreter runs (scratch scripts `data/work/sunsweep.py`, `data/work/sun2.py`; texture data as installed, no gamma): with c102 = 1.1 and zero interpolators the output rises from 0.29 to 0.75 where the lightmap is lit (0.1, 0.7, 0.9) and does not change where it is dark (0.3, 0.5). A sweep of 48 normal / view / light-direction combinations (c101 = the track's ShadowLightDirection, both signs) changed nothing (all 0.754), so my choice of which registers carry the normal and view vector (r3, r2) does not drive the term; the result is not meaningful. The term would be large (up to +0.46) if it fired, while Xenia's road is only 0.0575 in total, so in the real game it must be near 0 on a flat road seen from behind, and its real value cannot be read off with guessed inputs.
Conclusion: the sun/specular term is additive and gated, so it can in principle supply the missing 0.016 (ours 0.0415 vs Xenia 0.0575), but nothing here shows that it does. To decide I need the real interpolants: run the model's vertex shader on a road vertex (the world normal and view vector it outputs) and feed them in. Not done. No code changed this turn.

## Update 2026-10-19 (end 8): first look into the game's own files for the gamma and sun-term questions

- Decompressed FM2's `default.xex` (read-only; `crates/fh1-shaders/examples/xeximage`) to `data/work/fm2_xex/image.bin` (git-ignored, 11.7 MB). It also holds embedded effect files (cars, post, particles; a 165-technique effect at `0x82066f50`).
- Strings in the image: no `sRGB` / `SRGBTEXTURE` sampler-state names (only libpng text), `Gamma` only as the tone-mapping curve (`LightCurveGamma`, `DarkCurveGamma`), `GlobalSpecularLevel` as a track setting. The scenery shaders' constant names (`c_light0dir`, `c_light0colour`, `c_globalSpecularLevel`, `c_lightMapDarkColour`, ...) are NOT in the executable; they live only in the model files' shader containers. So the engine binds those registers (c101, c102, c104, c105, c108, c109, c114, c184, c185) by fixed register number in code, and the gamma of scenery textures is also not named anywhere in the strings.
- Model-side data (done earlier) has no gamma bit either (textures, PVS, stream records, shader containers, pool table).
Not done: reading the PowerPC code (no PPC disassembler in the repo) to see which values are written to registers 101..114 and which texture state is set per draw.

## Update 2026-10-19 (end 9): reading the game's PowerPC code (FM2 `default.xex`), first results

Tools (scratch, git-ignored under `data/work`): `data/work/fm2_xex/image.bin` (decompressed image), `data/work/fm2_xex/text.asm` (capstone disassembly of the whole `.text`, 1.9 M instructions; capstone cannot decode the Xbox 360 VMX128 vector ops, they show as `.word`), `data/work/ppc_xref.py` (finds `lis`/`addi` references to a string), `data/work/ppc_dis.py`. Image base 0x82000000.
- **TrackSettings loader** at `0x82524xxx`: reads `GlobalSpecularLevel` (string 0x820458b8, referenced at `0x82524620`) into a settings object at offset `0x620..0x62c` (a vec4), `LightColor` at `0x82524bd8`, `TrackLightMapTransition` at `0x82525a18`, `TrackLightMapColor` at `0x82525af0`, `StaticShadowColor` at `0x825244c8`.
- **Pixel-shader constant upload** at `0x82507960..0x82507c94` (vtable slot `+0x4c` = SetPixelShaderConstantF(device, start, data, count)), reading the renderer object `r31`: c100 <- `+0xec0`, c101 <- a normalised vector computed in the function (the sun direction), c102 <- `+0xeb0`, c103 <- `+0xed0`, c104 <- `+0xef0`, c184 <- `+0xf10`, c185 <- `+0xf20`, c188 <- `+0xf50`, c121 <- derived from `+0xf54/0xf58`, c119 <- `+0xf30..0xf38`. This confirms the register numbers used by the interpreter (c101 light direction, c102 light colour, c104, c184/185 lightmap dark/light) are fixed engine registers filled from the renderer object.
- **Renderer lighting setter** at `0x82277a18` (a virtual method, vtable `0x8200f654` slot 15 = offset 0x3c; also listed in a method table at `0x8217a838`), which stores its vector arguments at `+0xe90, 0xea0, 0xeb0, 0xec0, 0xe70, 0xe80` and floats at `+0xe60/0xe64/0xed0..0xedc`; `0x82277c28` copies a 0x90-byte block between `+0xe60` and `+0xef0` (save / restore). So c102 comes from the setter's 4th vector argument, c104 from the second block (`+0xef0`), and the mapping to TrackSettings fields is NOT yet traced (the caller of the setter through the vtable is not found).
- Not found yet: whether c104 really is `GlobalSpecularLevel` (assumed from the shader's constant name order), the values of c104.z/w, and the code that picks per-draw texture gamma.
Next step if continued: find the caller of vtable slot `+0x3c` (it passes the TrackSettings values), then read which fields become c102 / c104; and, for gamma, look for the code that writes the fetch constant's sign/gamma bits (it sets `0x...` masks on dword 0 of the fetch constants in the draw-state replay).

## Update 2026-10-19 (end 10): which TrackSettings fields become the shader constants (from the game's code); gamma still not found

Found `ApplyTrackSettings` at `0x8252a590..0x8252a864` (settings object `r31`, renderer object `R = [r31+0x2c8]`). TrackSettings field offsets in the settings object (from the loader at `0x82524xxx`): ShadowColor `0x5f0`, StaticShadowColor `0x600`, DropShadowColor `0x610`, GlobalSpecularLevel `0x620`, DarkColor `0x630`, LightColor `0x640`, ShadowLightDirection `0x550` / `0x650`, TrackLightMapTransition `0x6c0`, TrackLightMapColor `0x6d0`. It copies vec4s settings -> renderer, and the pixel-shader constant uploader (`0x82507960`) then sends the renderer fields:
- **c104** (`R+0xef0`) = `GlobalSpecularLevel` (settings `0x620`). CONFIRMED (Nurburgring: 1.7, 1.7, 1.7, 1). The interpreter's `c104 = 1` was therefore too low for the sun term; real value 1.7 (per track, see `TrackSettings.xml`).
- **c184** (`R+0xf10`) = `DarkColor` (`0x630`), **c185** (`R+0xf20`) = `LightColor` (`0x640`): CONFIRMED, as the importer assumed.
- **c103** (`R+0xed0`) = `StaticShadowColor` (`0x600`); `R+0xee0` = `DropShadowColor` (`0x610`, not part of the c100..c105 uploads seen); the lightmap transition (`0x6c0`) goes to `R+0xf30`, `TrackLightMapColor` (`0x6d0`) to `R+0xf40`.
- **c102** (light colour, `R+0xeb0`) and c100 (`R+0xec0`) are NOT TrackSettings values: `0x825061e0` sets them from two vectors read out of the scene's directional-light list (entries of `0x98` bytes, type 1: direction at +0x20..0x28, colour at +0x68..0x70), falling back to defaults when there is none. `c101` (sun direction) is the normalised vector the uploader computes from the renderer's light data. So the real sun colour and direction come from the track's light list, not from `LightColor`/`SHDir`; where that list is stored (a track file) is not yet identified. My earlier interpreter runs set c102 to `LightColor`, which is a guess.
- Gamma: no code found that sets it. The stream record type `0x2d` is not a plain D3D command (its words are offsets into `.data`); searching for the sign bits (0xFC) was not productive (0x2d compares appear hundreds of times). Not found.
Consequence for the Nurburgring sun term: c104 = 1.7 makes the additive highlight larger (not smaller), c102 is unknown, so the question (does it fill the 0.016 gap) remains open and needs the light-list values plus the real interpolants. Nothing changed in the repo.
Next step if continued: identify the scene light list (which file; search the track data for the 0x98-byte light entries) to get c102 / c101; then run the vertex shader for interpolants.

## Update 2026-10-19 (end 11): the scene light list is `<track>out.pre_00.rmb.bin`; Nurburgring has no directional light, so c102 is the default

- The light list is the "lighting" model `nurburgringout.pre_00.rmb.bin` inside the track's `bin.zip` (also `nurburgringout.sky.rmb.bin` is the sky). Its `.data` (79,468 bytes) holds 351 light entries of 152 (0x98) bytes each (name pointer to the string "Ambient" at +0, type at +4, direction at +0x20, colour at +0x68; matching what `ApplyTrackSettings` reads). 350 entries have type 3 (all named "Ambient"; the first reads direction (0, -1, 0), colour (1, 1, 1)); one more (type 43). **None has type 1**, which is the type `ApplyTrackSettings` looks for as the directional (sun) light.
- With no type-1 entry the game takes its default branch (`0x8252a720`): **c102 = (1, 1, 1, 1)** (white, from the constant `1.0` at `0x820014a8`), c100 (`R+0xec0`) = the settings' `LightAmbient` (settings `+0x530`: 0.3, 0.3, 0.3, with w forced to 1), and the vector passed to the direction setter is (1, -1, 0, 1) (constants `1.0`, `-1.0`, `0.0`). The uploader then derives c101 from the renderer's stored vector; whether `OverrideShadowLightDirection` / `ShadowLightDirection` (0.85, -1, -0.70 on Nurburgring) replace it was not traced.
- So for Nurburgring the sun term is `spec * (1,1,1) * c0 * c104` with `c104 = 1.7` (GlobalSpecularLevel). My earlier runs used c102 = LightColor 1.105 (wrong by a small factor) and c104 = 1 (wrong).
- Still open: the actual spec value (needs the world normal and view vector for a road vertex, i.e. the vertex shader's outputs, and the true c101), and so whether the sun term fills the 0.72 gap. Nothing changed in the repo.
Not tested: the other tracks' pre_00 models (they may contain a type-1 light).
Checked every track's `pre_00` model: Laguna Seca 300, Mugello 301, Road Atlanta 251, Sebring 301, Sunset 251, Suzuka 300, Tsukuba 301, Maple Valley 201, New York 401, Nurburgring 351, Test Track 350 light entries, all type 3 (plus one type 43 on most); none is type 1 (Silverstone: entry names differ, not decoded). So on these tracks the default branch applies and c102 = (1, 1, 1, 1) everywhere; the per-track difference in the sun term can only come from `GlobalSpecularLevel` (c104) and the geometry, not from a track-specific sun colour.

## Update 2026-10-19 (end 12): Nurburgring vertex shader read, sun term now computed with realistic inputs

Vertex shader of model 118 group 0 (`fm2_show`): it outputs `o2` = the vertex position in view space (`dp4` with `c0..c2`, the model-view matrix) and `o3` = the normalised view-space normal (normal times `c33..c35`, then `rsq`), plus shadow coordinates (`o0`, `o1`), two uv pairs (`o4`, `o5`), a third uv (`o6`) and fog (`o7`). In the pixel shader `r2` = that position (so `r2/|r2|` is the eye-to-pixel direction) and `r3` = the normal. Specular = `dot(normalize(c101 - V), N)` raised to `c2.x` (`log`, `mulsc c2.x`, `exp`), times gate x `c102` x `c0` x `c104`, added on top of `albedo * light`.
Why my earlier sweeps never changed: **`c2` is not set by this draw** (its constant list is only `c0` = 1 and `c1` = 0.5), so the interpreter used exponent 0 and `pow(x, 0) = 1` (the sun term was a constant). In the real game `c2` keeps whatever an earlier draw left in the GPU constant register, which our parser does not see.
Re-run (`data/work/sun3.py`: view-space position (0, -1.3, 8/20/40), normal (0, 1, 0), c101 = the track's ShadowLightDirection rotated by 12 camera headings, `c102` = (1,1,1,1), `c104` = 1.7, lit lightmap point 0.288 without sun, sRGB-decoded textures not applied here): added green value by exponent: 8: min 0.001, median 0.005, max 0.46; 20: median 0.000, max 0.24; 50: max 0.046; 100: max 0.003. So the sun term is near zero for most sun/camera headings and large (up to +0.46) only when the camera looks toward the sun's highlight with a low exponent. It can supply the missing 0.016 only for particular headings and exponents; with `c2` and the heading unknown it neither confirms nor rules this out.
Open: the real `c2` for this draw (state left by the previous draw, or the material's `eccentricity`) and the camera heading relative to the sun in the Xenia shot.
Nothing changed in the repo.

## Update 2026-10-19 (end 13): INCIDENT, installed FM2 data regressed by my re-imports, then restored; Tsukuba not started

What happened: the `ui-elements` branch importer (`crates/fh1setup/src/fm2_scenery.rs` here) lacks the 17 FM2 importer commits of the `fm2-lightmaps` branch (and the uncommitted work in `D:\Projects\slop\fh1_rewrite_fm3`: placeholder-texture filter, black-road-constant -> grain texture rule, wear invert, decal cut-out, `shader_albedo`, ...). The installed FM2 data was built with the `fm2-lightmaps` importer. My re-imports today (Nurburgring tests, then the all-track runs to check the BC4 rule's reach, including the "restore defaults" runs) used the `ui-elements` importer and so overwrote the installed data of all FM2 tracks with an older road/material generation. Symptom found when starting Tsukuba: its road rendered black (`screenshots/bc4_tsu_off.png`). Other tracks were affected the same way and were not individually checked.
Repair: built the `fm2-lightmaps` worktree's importer (read-only use of that folder; build output in `target-fm3/` inside this project, source untouched) and re-imported all FM2 tracks plus the remaster tables with it, no switches: Tsukuba's road is back (left band 115,114,111, right 90,89,89, `screenshots/restore_tsu.png`) and Nurburgring is back at its earlier default (127,127,128 / 111,111,110 against 129,130,130 / 111,111,110 before). Not tested: every other track against its earlier shot (no saved shots to compare), the FM1/FM3/FM4/FH1 data (not touched).
Consequences: (1) the `FM2_BC4_SRGB` switch (done in the `ui-elements` importer, hash-gated to Nurburgring's road shader) is NOT in the importer that produced the installed data; the installed Nurburgring is the default (pale, 0.19). (2) Any further road work should be done on top of the `fm2-lightmaps` importer, so that branch has to be merged into `ui-elements` (a 17-commit decision for the owner) or the switch ported there. (3) Do not re-import FM2 with `target/debug/fh1setup.exe` of this branch until that is decided; use `target-fm3/debug/fh1setup.exe`.
Xenia reference for Tsukuba: `screenshots/xenia2/x10.png` (x5 = Sebring, x9 = Mugello, x11 = Laguna Seca, x12 = Suzuka). A first look shows the camera differs from ours, so band medians are not comparable without a consistent method (not done).

## Update 2026-10-19 (end 14): fm2-lightmaps merged into the working tree (nothing committed), BC4 switch ported; Tsukuba not changed on purpose

Merge, done as working-tree changes only (no `git merge`, nothing staged, nothing committed):
- Engine side: `track.rs` already held exactly the branch's changes; `main.rs` already held them except one line the tree replaced with its own FM2 grade logic (kept); the engine `fm2_sky.rs` in the tree is newer than the branch's (kept).
- Added from `fm2-lightmaps`: `crates/fh1setup/src/fm2_sky.rs` (+ `mod fm2_sky;` in `fh1setup/src/main.rs`), `crates/fh1-formats/examples/{fm2_pvs_unused,fm2_sky,fm2_state}.rs`, the `fm2_show.rs` `sky` argument.
- `crates/fh1setup/src/fm2_scenery.rs` and `fm2_uv.rs`: replaced by the `fm2-lightmaps` worktree's working copies (branch HEAD plus its uncommitted work: placeholder filter, black-road-constant rule, wear invert, `shader_albedo`, `PsRun`, ...). My previous versions are saved in `data/work/premerge/` (`fm2_scenery.mine.rs`, `fm2_uv.mine.rs`, `mine.patch`).
- Ported my `FM2_BC4_SRGB` switch into that `fm2_scenery.rs` (hash of the draw's `PsRun` words instead of the old `DrawUv::ps_sig`; same Nurburgring hash `4aa0eec057a0743d`; also skips placeholder textures). `ps_cpu.rs` was already identical. Builds (`cargo build -p fh1setup --features fm2`).
- Checked: with `FM2_BC4_SRGB=1` all tracks re-imported, only Nurburgring's 4 draws change; Nurburgring road 0.0407 linear (61,61,65 / 58,58,61) against 0.187 with the switch off (127,127,128 / 111,111,110); installed data is back to switch OFF. The old `target-fm3/` build folder is no longer needed.
Tsukuba (decision, with numbers): the Xenia reference `xenia2/x10.png` shows another spot (pit-lane concrete and strong shadows), so only a crude comparison is possible: the far-road patch at the same screen fraction reads ours 60,61,60 (0.043 linear) against Xenia 83,76,80 (0.070): ours / Xenia 0.62. After this merge Tsukuba is already a little DARK against Xenia, not 1.8x bright (that figure came from the older importer). The interpreter's gamma-on-tf4 result for Tsukuba (65 against 113) would make it darker still (about 0.2x), so the rule was NOT applied to Tsukuba. Not tested: a like-for-like Xenia spot on Tsukuba's start straight, other tracks after the merge.
Untracked/modified files stay uncommitted for you to review (`git status`).

## Update 2026-10-19 (end 15): Tsukuba like-for-like attempt, no matching spot found, no Tsukuba change

Tried to place our car at the Xenia `x10.png` spot. `FH1_TELEPORT` with the importer's `pit_box_1` coordinates put the car on grass beside the pit gantry (the spawn file's coordinates are not engine space, one try, `screenshots/tsu_pit_67.4.png`); `FH1_SPAWN=11` and `13` (other grid slots) show the main straight from further back / another view, none matching `x10` (which has the pit canopy gantry overhead, a yellow pit line right and pale pit-lane concrete at the lower left).
Readings from our three views (far road patch at the same screen fraction, grade off): 0.043, 0.0385 and (slot 13, sky in the patch, ignore) against Xenia `x10` far road 0.0697: ours / Xenia about 0.55 to 0.62; the near bands (left 0.167..0.187, right 0.085..0.092) cannot be compared with `x10` (its left band is sunlit pit-lane concrete at 0.52, a different surface). So the only comparable measure says Tsukuba's road is somewhat dark, not bright; the gamma-on-BC4 rule would make it much darker, so nothing was changed. Not tested: an exact same-spot comparison; this needs the Xenia car's position (not recorded in the shot).
Option for the owner: take a new Xenia shot of Tsukuba at the engine's default spawn (grid_00) so the same road patch can be compared.

## Update 2026-10-19 (end 16): Tsukuba compared with the user's new Xenia shot (`screenshots/xenia2/x13_tsukuba_rolling.png`, rolling start, 57 mph)

Pixel readings (median sRGB, then linear G), Xenia: sunlit asphalt left of the car (97,92,81) = 0.106; mid-left (105,100,88) = 0.128; far right of car (97,92,80) = 0.106; just ahead (103,99,89) = 0.125; the car's shadow (44,42,36) = 0.019. Ours (merged importer, grid_00 view, same screen fractions): left near band (115,113,111) = 0.167; shadow band (87,86,84) = 0.092 (not comparable: our shadow is much lighter than Xenia's 0.019).
So on sunlit asphalt ours / Xenia = 0.167 / (0.106 to 0.128) = about 1.3 to 1.6 (too bright, and neutral grey against Xenia's warm R > G > B by about 16). This agrees with the old 1.8 reading, not with my crude `x10` reading (which was a different surface).
Interpreter candidates for Tsukuba road (model 50 draw 0, lightmap-grid mean, linear G): no gamma 0.165, gamma on tf3 0.227, tf4 0.050, tf3+tf4 0.069, all four 0.069. Target about 0.11 to 0.13: no candidate lands near (0.165 is 1.4x high, 0.069 is 0.6x low), so no gamma assignment is supported for Tsukuba; the rule is not applied. A plain scale (about 0.7x on the baked road colour) would fit, but it is a fit to one screenshot, not a game value.
Not tested: any Tsukuba change in the engine; the warm tint (Xenia's sun colour / grading vs our neutral light).

## Update 2026-10-19 (end 17): data from the real game: FM2's own post-process (`FinalCombineBloomTonemap`) and per-track settings

Probes `crates/fh1-shaders/examples/fm2_fxlist.rs` and `fm2_fxdump.rs` (the output is game code, so it stays in git-ignored `data/work/fm2_fx/final_bloom_tonemap.txt`). The embedded effect at `0x8205a310` in FM2's `default.xex` has the post technique `FinalCombineBloomTonemap` (and variants with motion blur / DOF / all effects). Its pixel shader does, per pixel:
1. `c = scene + bloom` (two textures).
2. Desaturate toward luminance: `L = dot(c, desatLuminosity)`, `c = L + (c - L) * desatAmount`.
3. Tint: `c *= desatPostMod` (rgb).
4. Tone curve on luminance `Y = 0.2125 R + 0.7154 G + 0.0721 B` of that colour: `Y' = ((max(Y - P.x, 0)) / (P.y - P.x)) ^ (1 / P.z) + P.w`, `out = c * Y' / Y`. `P` = (black point, white point, gamma, boost) is computed in the vertex shader as a blend between the DARK and LIGHT curve parameter sets (`tonemapCurveParamsDark/Light`) by `t = saturate(trackTonemapLimits.w * (some function of trackTonemapLimits.x and the adapted scene luminance))`: an auto-exposure style blend driven by the average luminance of the frame (a small luminance texture, `tf17`). The exact sign convention in the limits term is not verified.
Per-track inputs are in each `TrackSettings.xml`: `DesaturateAmount`, `DesatLuminosity`, `DesatPostMod`, `ToneMappingLevels` (Light/Dark: black point, gamma, white point, boost), `bloomCutoff`, `bloomScale`, `skyGain`. Examples: Tsukuba desat 0.83, tint (1.03, 1.01, 0.94), curve black 9 / light gamma 1.11 white 261 / dark gamma 1.51 white 260, boost 1; Nurburgring desat 0.85, tint (1, 1, 1), black 4 / light gamma 1.01 white 287 / dark gamma 1.21 white 270; Sunset desat 0.94, black 7; Test Track desat 0.76 tint (1.04, 0.96, 0.92); Sebring desat 0.75 tint (0.92, 0.92, 0.95).
What this explains and what not (my arithmetic, not tested in the engine): (a) the warm cast of Xenia's Tsukuba asphalt (R > G > B by 16) is partly this tint (blue x 0.94) and our neutral grey lacks it; (b) the tone curve on a grey road of linear 0.167 (assuming the 0..255 values are divided by 255) returns about 0.169 with the Light curve or 0.27 with the Dark curve, so it does NOT darken Tsukuba's road from 0.167 to Xenia's 0.11; (c) the per-track spread of the other tracks can be partly due to the Light/Dark blend, which depends on the frame's average luminance. The engine currently skips all of this for FM2 (`FH1_RM_TONEMAP=none`, LUT 0, vignette 0).
Still open for Tsukuba's brightness gap: not the post curve; candidates remain the road's shader albedo (the interpreter says 0.165 at the lightmap mean, matching ours), the gamma handling of its grey textures (no gamma 0.165, gamma on tf3 + tf4 0.069, Xenia 0.106 to 0.128, between the two), or Xenia's display gamma.

## Update 2026-10-19 (end 18): FM2's final combine implemented in the engine behind `FH1_FM2_POST=1` (off by default); first comparison

Code: `crates/fh1-remaster/src/post.rs` (`Fm2Post` parsed from `TrackSettings.xml`, a new branch in the grade shader: desaturate by `desatAmount` toward `desatLuminosity`, multiply `desatPostMod`, luminance tone curve `Y' = ((Y - BP)/(WP - BP))^(1/gamma) + boost` with the dark / light parameter sets blended by `t`; black / white points and boost are divided by 255 (UNVERIFIED), `t` is a fixed `FH1_FM2_POST_T`, default 1 = light curve, because the game derives it from the frame's average luminance, which is not reproduced), `crates/fh1-engine/src/main.rs` (reads `imported/<id>/tracks/TrackSettings.xml` for FM2 tracks when the env var is set, inserts the resource before the remaster plugin). The grade pass stays off for FM2 unless the switch is set. Built with `cargo build --release -p fh1-engine --no-default-features` (the default build needs the DLSS SDK).
Road bands against Xenia (crude: our grid_00 views against the user's Xenia shots, same screen fractions; poses differ, so only large moves mean something). Ours / Xenia, linear G, switch off -> on:
- Tsukuba (x13, rolling start): left 1.57 -> 1.6 (118,115,110 against Xenia 97,92,81: a little warmer, not enough; Xenia R/B 1.20, ours 1.07); far patch 0.043 -> 0.027.
- Suzuka (x12): left 3.1 -> 1.4, right 9.0 -> 3.8 (closer).
- Mugello (x9): left 0.92 -> 0.52, right 1.53 -> 0.84 (left goes too dark).
- Sebring (x5): left 0.40 -> 0.20 (worse), right 1.05 -> 0.71.
- Laguna Seca (x11): left 4.1 -> 3.9, right 2.5 -> 2.4 (unchanged, still far too bright).
Reading: the game's tone data changes brightness by track in plausible directions but does not by itself reconcile the tracks, and the uncertain parts (the /255 unit, the fixed `t`) move the result a lot; with poses this different I would not decide anything from these numbers.
Not tested: `t` from a real average luminance, the unit assumption, the bloom term (`bloomCutoff` / `bloomScale` are not applied), a like-for-like Xenia pose for any track but Tsukuba.
Test: see the MODLOG entry.

## Update 2026-10-19 (end 19): VERIFIED in the game's PowerPC code: the unit and the blend of the final combine

Function at `0x825b2c08` sets the effect parameters (strings `tonemapCurveParamsDark` / `...Light` / `trackTonemapLimits`); its only caller (`0x82284cec`) builds the vectors from the renderer object (`R`, fields `+0xe30..0xe54` hold the loaded `ToneMappingLevels` and `TrackLumLimits`) and multiplies by the constant at `0x82002e4c` = 0.003921569 = **1/255**:
- `tonemapCurveParamsDark` = (Dark black point / 255, Dark white point / 255, Dark gamma (NOT scaled), Dark boost / 255); `...Light` likewise with the Light values.
- `trackTonemapLimits` = (min / 255, max / 255, (max - min) / 255, 1 / ((max - min) / 255)) from `TrackLumLimits` (Tsukuba 85..380, Sebring 94..270, Suzuka 115..310, Laguna Seca 150..215, Mugello 20..296).
- The vertex shader blends: `t = saturate((L - limits.min) * limits.invRange)` where `L` is the average of two texels of a luminance (adaptation) texture, so t = 1 (light curve) for a bright frame, 0 (dark curve) for a dark one (sign convention of the subtract read from the intent).
So the 255 unit I assumed is correct. Implementation updated (`post.rs`): `Fm2Post` carries `lum_min` / `lum_inv_range`; the shader estimates `L` as the mean luminance over an 8 x 6 grid of the frame; `FH1_FM2_POST_T` (0..1) forces `t`.
Result (grade off -> `FH1_FM2_POST=1`, ours / Xenia linear G, left band): Tsukuba 1.60 -> 2.64, Sebring 0.40 -> 0.63, Suzuka 3.1 -> 3.6, Laguna Seca 4.1 -> 5.8, Mugello 0.92 -> 1.45. With the computed blend almost every frame lands at t about 0 (the dark curve, gamma 1.2 to 1.5, which BRIGHTENS mids) because the engine's average luminance (linear 0.13 to 0.33, sRGB-space 0.37 to 0.58) is below every track's `min` (0.08 to 0.59). The game's frame average sits between min and max, so the engine's frame is darker than the game's in the game's units, or the adaptation texture stores a different quantity; not resolved. Forcing `t = 1` (the earlier test) darkens instead and was mixed.
Conclusion: the formula and units are now verified, but the blend input is not reproducible with the engine's frame luminance, so the switch stays off and no setting of it makes the FM2 roads match Xenia. Not tested: what the adaptation texture really holds (its render target and the pass that fills it), bloom (`bloomCutoff`, `bloomScale`).

## Update 2026-10-19 (end 20): what fills the luminance texture (`tf17`) of the final combine

The same embedded post effect (`0x8205a310`) holds the chain, dumped with `fm2_fxdump` (output in git-ignored `data/work/fm2_fx/`): `DownSample16X` = four taps of the scene texture, each offset by one texel, times 0.25 (a plain average); `AveragedDownSample` = a read of a chosen mip level; `DownSample16XAndAccumulate` = the same four-tap average blended with the previous frame's result: `out = lerp(average, previous, previousFrameAmount)` (temporal smoothing). So `tf17` is a small texture of the AVERAGE SCENE COLOUR, in exactly the units of the texture the final combine reads (`tf0`), temporally smoothed; the final combine's vertex shader takes the luminance of two of its texels (weights 0.0721 / 0.2125 / 0.7154) and `t = saturate((L - min) / (max - min))`. There is no hidden exposure or log scale in that path.
Consequences: (1) the blend is computable exactly if the engine's HDR frame carries the game's scene values; (2) the game's tracks put their average inside `TrackLumLimits` (Laguna Seca 0.59 to 0.84, Tsukuba 0.33 to 1.49, in scene units where the curve's white is about 1.0), while our frames average 0.13 to 0.33 (linear luma), so the game's scene buffer holds clearly more light overall than ours (most likely HDR values above 1 in the sky and sunlit surfaces, which our clamped display path never shows); (3) that cannot explain Xenia's darker roads: with Tsukuba's road at 0.167 before the curve, the light curve returns 0.165 and the dark curve 0.27, never the 0.106 to 0.128 the Xenia pixels imply, for any `t`. So the roads' shading before the post (albedo or light term) is still too bright by about 1.4x; the post cannot fix it.
No code change this turn. Not tested: the engine's frame average with a sky matching the game's HDR sky; which render target format and gamma the game's scene buffer / final output really use (the swap-chain encoding is assumed to be sRGB).

## Update 2026-10-19 (end 21): lit-texel comparison: BC4 textures are sRGB on all four checked road families; importer switch generalised, results mixed

The earlier "Test Track and Tsukuba contradict the rule" came from comparing the interpreter's GRID MEAN over the lightmap tile (lit and shadowed texels mixed, 0.165 / 0.069) with Xenia's SUNLIT asphalt. Comparing the lit texels only (`data/work/lit_all.py`, interpreter, maximum over a 10 x 4 lightmap grid, G channel, linear):
| family | Xenia sunlit road | no gamma | gamma on its BC4 samplers |
| Nurburgring (118) | 0.058 | 0.300 | 0.070 (tf4..tf7) |
| Tsukuba (50) | 0.106 to 0.128 | 0.273 | 0.114 (tf3..tf6) |
| Test Track (321) | about 0.17 (115 per channel, rough) | 0.404 | 0.125 (tf3..tf5) |
| Sunset (77 g2) | 0.16 | 0.134 | 0.141 (only tf4 is BC4; indifferent) |
So "single-channel BC4 (DXT5A) textures are sRGB-decoded; BC1 colour textures are not" fits all four, within 0.74x to 1.2x, where the no-gamma result is 2.3x to 5x too bright (except Sunset). The pre-post roads were too bright because of this (the post curve was not the cause).
Importer: with `FM2_BC4_SRGB=1`, `shader_albedo` (the interpreter run on the draw's pixel shader with neutral light) now sRGB-decodes BC4 samples (`decode_rgba` marks them, `sample_rgba` decodes per texel) and is used for every baked road (it also turns `FM2_SHADER_ROAD` on). Results against Xenia (our left band / Xenia, linear G, before -> switch): Tsukuba 1.60 -> 0.98 (92,91,89 against 97,92,80; the sunlit asphalt now matches); Sebring 0.40 -> 0.40 and Laguna Seca 4.1 -> 4.1 (unchanged, not baked roads); Suzuka 3.1 -> 0.32 (too dark now); Mugello 0.92 -> 1.83 (worse); Nurburgring unchanged by this part (its hash-gated blend gives 0.0407 as before).
Reading: the gamma rule is right for the families checked, but `shader_albedo` is wrong or incomplete for the Suzuka and Mugello road shaders (other constant sets, extra samplers or gamma on other textures, not checked), so the generalisation is NOT adopted as a default. The switch stays off; installed data is back to the default import. Not tested: Suzuka's and Mugello's shaders against their Xenia lit values, Silverstone, Road Atlanta, New York, Maple Valley, Sebring and Laguna Seca roads (colour-texture draws).

## Update 2026-10-19 (end 22): Suzuka and Mugello road draws in the interpreter; per-sampler gamma table

Debug print extended (`FM2_ROAD_DBG=1` now also prints the shader hash, triangle count, constants and texture ids of each baked road draw). Dominant baked-road shader families by triangles: Suzuka `52ff81cbba901906` (9514 tris; model 867 group 0 draw 0: tf2 840 lightmap, tf3 841 BC4 512x1024 flags 4, tf4 116 BC4 512x512 flags 20; draw constants c0 1, c1 .06, c2 .02, c3 1.55, c4 .34, c5 2.58, c6 .01, c7 .05) and Mugello `4319799a24a8f18f` (4893 tris; model 3290 group 0 draw 0: tf2 1812 lightmap, tf3 1687 BC4 512x1024 flags 4, tf4 71 BC4 512x512 flags 20; c0 0, c1 .08, c2 .05, c3 .2, c4 .6, c5 .65, c6 .5, c7 .1). Scratch scripts `data/work/lit_per.py`, `lit_all.py`. Lit-texel value (max over a 10 x 4 lightmap grid, G, linear; track lightmap colours from `fx_globals.json`; no sun term; Xenia from sunlit asphalt patches of the user's shots):
| family | Xenia lit road | none | gamma tf3 only | gamma tf4 only | gamma on all BC4 |
| Nurburgring | 0.058 | 0.300 | tf4 0.249 | tf5 0.121 | 0.070 |
| Tsukuba | 0.117 | 0.273 | 0.377 | 0.083 | 0.114 |
| Test Track | 0.17 | 0.404 | 0.306 | 0.178 | 0.125 |
| Suzuka (867) | about 0.051 (rough: shaded patches 0.02 to 0.05) | 0.081 | 0.069 | 0.020 | 0.017 |
| Mugello (3290) | about 0.133 | 0.081 | 0.083 | 0.059 | 0.060 |
Reading: "gamma on all BC4 samplers" lands within 0.74x to 1.2x on Nurburgring, Tsukuba and Test Track, but on Suzuka it gives 0.33x of the Xenia value and on Mugello 0.45x (and without gamma Mugello is 0.6x, Suzuka 1.6x). So the Suzuka and Mugello road draws are not explained by texture gamma alone: either the draw I analysed is not what the Xenia patch shows (the importer's per-track road pick is by triangle count, not by what is under the car), or those families add light the interpreter omits here (the sun/specular term, ambient/SH light, the shadow term), which is the same missing piece as in the Nurburgring gap. This is why `FM2_BC4_SRGB` over-darkens Suzuka and brightens Mugello.
Not done: confirming which draw is under the Xenia camera for Suzuka and Mugello (painting-textures test from DESIGN.md), the sun term for these two families. The switch stays off by default.

## Update 2026-10-19 (end 23): painting test on Suzuka and Mugello, the analysed road draws are the right ones

Scratch tools `data/work/paint_tex.py` (all textures, backup / restore) and `data/work/paint_some.py` (chosen texture ids; restored and md5-verified after each run). Method: paint the importer's baked road textures `ffff2000..` solid colours and shoot the spawn view.
- Suzuka (`screenshots/paint_suz_baked.png`): the whole road under and ahead of the car is `ffff2000` (painted red). That baked road is the first road key, built from the dominant shader family `52ff81cbba901906` (model 867 and its siblings, constants c0 1, c3 1.55), which is the draw analysed. (An all-texture paint, `paint_suzuka.png`, showed the same road region as one dark colour.)
- Mugello (`screenshots/paint_mug_baked.png`): the whole road is `ffff2002` (painted blue), built from grain 0x47 and road colour 111 and dominated by shader family `4319799a24a8f18f` (4893 triangles, model 3290), the draw analysed.
So the Suzuka and Mugello lit-texel numbers (previous section) are for the right draws. Mugello: the shader gives 0.081 (no gamma) or 0.060 (gamma on both BC4 samplers) against Xenia's lit road 0.133 (x9 left cells, G 100 to 105): too low either way. This family has `c0 = 0` and the shader multiplies its sun term by `c0`, so there is no sun term in it; the missing light (about 0.05 to 0.07) is not the sun. Candidates not yet checked: the lightmap texels at the real road (our crop of the lightmap atlas could be dimmer than the real one; at lightmap value 1.0 the light term would be about 0.79 against about 0.5 now, which alone gives 0.128), the shader's other light inputs (c100 ambient `LightAmbient` 0.3, c103, c105), the fog colour added at distance. Suzuka: the Xenia `x12` shot is mostly in shadow (its lit road patch near the horizon is hazy), so there is no reliable Xenia value (0.02 to 0.054 range); no conclusion.
Both tracks' textures restored (md5 verified), installed data is the default import (stale `ffff3xxx`+ files from older imports remain unreferenced).
Not done: the lightmap-texel check for Mugello (real tile values against the crop, atlas offsets), a new Xenia Suzuka shot in sun.

## Update 2026-10-19 (end 24): Mugello's lightmap texels and the missing light

Mugello road draw (model 3290): lightmap tile texture `0x714` (1812), a 32x16 crop of its atlas page (`convert_lightmaps`, `fm2_scenery.rs` ~1316: per output texel it samples the page at `offset + scale * t`, `su` negative = mirrored). Texel values of the crop (0..255): min 0, max 239, mean 57.9; most of the tile is 0 (shadow / unlit), the lit strips are 140 to 239 (typically 198 to 222 = 0.78 to 0.87), so lightmap values near 1.0 are not present in the crop (max 0.94). Effect on the road: `light = lerp(c184, c185, lm)` with Mugello's dark -0.10 and light 0.795: 0.68 at lm 0.87 and 0.795 at lm 1.0 (x1.17). With the interpreter's lit value 0.081 (no gamma) the best case at lm = 1.0 is 0.095, and with gamma on the BC4 greys 0.060 -> about 0.07, against Xenia's 0.133 (x9 left cells): so the lightmap crop cannot explain a 1.4x to 2.2x gap. The PS of this family (read in full, `fm2_show 3290 0`): output = `2 * min(shadow-ambient term, lerp(c184, c185, lm)) * albedo` plus `sun * albedo` (`c0` = 0 here, so no sun term); albedo = palette blend `c1..c7` of two grain samples (`tf4` twice) chosen by the luma of `tf3`. Nothing in it adds ambient or sky light, so with these inputs the shader cannot reach 0.133. Remaining possibilities: other values of the draw's constants than the ones recorded (a constant not set by the draw keeps the previous draw's value in the game, e.g. `c2` of the Nurburgring draw), the Xenia patch not being this surface (sunlit road under other conditions), or the display transfer function (if Xenia's picture were decoded with exponent 2.0 instead of 2.2, all Xenia lit values rise by about 1.2x: Mugello 0.133 -> 0.16, Tsukuba 0.117 -> 0.14, Nurburgring 0.058 -> 0.075, which moves most families closer to the no-gamma side, so that must be tested together with the gamma rule).
No code change. Not tested: constants persisting across draws (needs the whole-stream replay of the constant records), the display exponent.

## Update 2026-10-19 (end 25): do inherited constants change the lit-texel predictions? No

New probe `crates/fh1-shaders/examples/fm2_ps_consts.rs` (constant table of each pixel-shader container: names and registers; none of the road shaders carries default values) and scratch `data/work/c_records.py` (scans every model's `0x06` constant records of a track).
- `parse_model` already carries constants across the draws of a model (it keeps one running table), so the draw constants shown for the first draw of a model are what the model's own stream sets.
- Each road shader reads exactly one constant its draw never sets: the LAST `c_constantN` of the container: Nurburgring 118 `c2` (a specular-exponent use: `mulsc c2.x * log(...)`), Mugello 3290 `c8`, Suzuka 867 `c8`, Tsukuba 50 `c4`, Test Track 321 `c6`. The container names are `c_constant0..N` (plus `c_specularLevel` = c5 on Tsukuba's), the model strings name the material parameters `specular0_gain`, `eccentricity`, `colour0_offset`, `colour0_gain`, so these registers are the specular exponent (eccentricity) and similar, fed by the engine outside the model's constant records.
- Across Nurburgring's models `c2` is set to many different values (0.91 x315, 0.4 x268, 0.93 x138, 1.0 x106, ...) because `c_constant2` means different things in different shader families, so what a draw inherits in the game depends on the previous draw in the frame and is not recoverable from the files.
- Effect on the predictions: the unset constant is used only in the specular term, which is multiplied by `c0` (Mugello 0: killed; Suzuka 1, Nurburgring 1, active). For Nurburgring the earlier sweep (STATUS end 12) shows the term is near zero for exponents of 20 or more and large only for small exponents, and the Xenia road's darkness argues for a large exponent. For Mugello no value changes anything. So the lit-texel table does not change; Mugello's 1.4x to 2.2x shortfall is NOT explained by inherited constants.
Not tested: a frame-order replay of draws across models (would need the game's draw order).

## Update 2026-10-19 (end 26): the material block of each model holds the constants the stream does not set; the sun term is negligible

Each model's `.data` has a material parameter block: 4-float vectors with a 0x14 stride (vector + one pad word), holding `c_constant0..N` in order (scratch tool `data/work/mat_consts.py` finds it by matching the draw's own constants). The stream's `0x06` records repeat the first constants but omit the LAST one, the specular exponent, which is only in this block:
| model | block at | exponent register | value |
| Nurburgring 118 | 0x146c | c2 | 9.6848 |
| Mugello 3290 | 0x14a8 | c8 | 9.6848 |
| Suzuka 867 | 0x16b4 | c8 | 8.4715 |
| Tsukuba 50 | 0x15c0 | c4 | 12.3144 |
| Test Track 321 | 0x15f4 | c6 | 7.7777 |
(The block also shows Mugello's `c0` = 1 as the material default while the draw's record sets it to 0 for this draw, which is what kills the sun term there.) With these exponents, `c102` = white (no directional light in `pre_00`), `c104` = `GlobalSpecularLevel` of the track, and the sun direction from TrackSettings `ShadowLightDirection` rotated into the view space of the spawn camera (spawn facing from `spawns.json`, camera 1.3 m high, road points 6 to 50 m ahead, both signs of the light vector; `data/work/sun_real.py`), the sun term adds at most 0.004: Nurburgring 0.070 -> 0.074 at 6 m (else 0.070), Tsukuba 0.114 -> 0.114, Test Track 0.125 -> 0.127, Suzuka 0.017 -> 0.017. So it does not explain the remaining differences (Nurburgring 1.2x above Xenia's 0.058, Test Track 0.74x of Xenia's about 0.17; Suzuka's Xenia value is unreliable). This closes the "sun term" lead for these families. Not tested: the game's actual `c101` (its own default direction (1, -1, 0)), but exponents of 8 to 12 make any direction give only a narrow highlight.
Still open: Mugello 0.06 to 0.08 against Xenia 0.133 (no sun, lightmap and inherited-constant explanations left), Suzuka reference, the display exponent, and the texture-gamma code itself.

## Update 2026-10-19 (end 27): what the game's code says about texture gamma (PowerPC, `fm2_xex/text.asm`)

- **Texture loading** (`0x82511200..0x82511580`): reads the 28-byte `.bix` header (`BIX0`, width, height, mips, format word) and hands the width, height, mip count and the file's format word to the texture-header builder (`0x823c6000`; the same words as in the file). The header's dword 0 sign bits (bits 2 to 9, mask 0x3FC) are copied from bits 9 to 16 of that format word (`0x8236cb9c: rlwimi r9, r10, 0x19, 0x16, 0x1d`). All `.bix` format words have these bits 0 (`0x1a20017b`, `0x1a200152`), so every texture is created with no gamma sign. **The texture format alone does not decide gamma.**
- **Draw-stream replay** (`0x82530fc8`): the record type byte at +2 selects the handler: types 1 and 0x2f..0x3a (draw / generic) go to the generic command path (`0x82722808`, `0x8253109c`), type 5 to `0x82522848`, **type 0x2d to `0x825303b8`**. Record length is the halfword at +0.
- **The `0x2d` handler is not a plain texture binding**: it sets textures by item (`0x8236c208`) and then, per binding `(item, slot << 16 | index)`, applies a SAMPLER STATE BLOCK: the index (my "flag" 1/2/3) selects an entry of a per-model table (`r19 = [r6 + 4]`), each entry a count plus a list of `(state, value)` pairs passed to per-state handlers (`device + 0x1d0 + state`, with `SetSamplerState(slot, value)` signature); some handlers write the sampler's fetch-constant words in the device shadow (`device + slot * 0x18 + 0x400`: dword 0 clamp bits 10 to 18 from three states, dword 3 filter bits, dword 5 anisotropy bits). A second routine (`0x82376988..0x82376a4c`) merges a precomputed field at +0x2c of a state block into dword 0 bits 2 to 9 (mask 0x3FC), i.e. the **sign / gamma bits are part of the sampler-state block** that the binding's index selects.
- So gamma is a per-(model, binding) sampler-state choice stored in the model's state table, not a property of the texture or its format. The indices are model-local (1, 2, 3 mean different blocks in different models), which is why a flag-only rule fails across families: Nurburgring's five textures all carry index 2; Test Track's best single fit is the index-3 sampler (gamma on `tf4` alone gives 0.178 against Xenia 0.17); Tsukuba's index-3 samplers alone give 0.083 (0.71x); Mugello's give 0.059 (0.44x); Suzuka has only indices 1 and 2.
- The table is in the model's `.data` (Nurburgring 118: a count and three pointers at `0x218`, entries of 0x24 bytes with hashed state names, e.g. `0x00a6d434`, `0x00fd1f34`, `0x0385125f`, and values `0x45`, `0x6f`, `0x81`); I could not yet map the hashes to sampler states or read a gamma value out of them. Hash identification (a state-name hash table in the game image) would give a definitive per-binding gamma.
- Check done: Nurburgring with gamma also on its BC1 sampler (`tf3`) predicts 0.093 against Xenia 0.058, worse than the BC4-only 0.070, so "gamma on every index-2 binding" is not it either; the BC4 rule remains an empirical proxy that fits Nurburgring, Tsukuba and Test Track and fails Suzuka / Mugello.
Next step if continued: find the hash function / table of sampler-state names in the game image (search for the three hashes as constants), decode each model's state table to get the SRGB value per index, and replace the BC4 proxy with it.

## Update 2026-10-19 (end 28): decoding the per-model state tables, correction to end 27

What the tables look like (Nurburgring 118 at `.data` 0x218, Tsukuba 50 at about 0x254, same layout): a count (3), three pointers, then 0x24-byte entries. Entry 0 holds three `(begin, end, capacity)` triples, which are three std::vector-like lists that are EMPTY (begin == end: Nurburgring `0x45,0x45,0x1a0`, `0x6f,0x6f,0`, `0x81,0x81,0`; Tsukuba `0x59,0x59,0x1a0`, `0x6d,0x6d,0`, `0x7f,0x7f,0`). Entries 1 and 2 are three `(hash, 0, pointer)` triples pointing at those same three lists: hashes `0x00a6d434`, `0x00fd1f34` are common to the models, the third differs per model (`0x0385125f` on Nurburgring, `0x0ab49fe9` on Tsukuba), so it looks like a per-shader or per-material key. In both models the entries that bindings use are IDENTICAL to each other (Nurburgring indices 1 and 2; Tsukuba indices 2 and 3), and the lists they reference are empty.
Hash search: no match of the three hashes against about 75,000 strings of the game image with 12 hash functions (FNV-1/1a, djb2 variants, sdbm, times 31/33/65599, ELF, Jenkins one-at-a-time, CRC32, Adler) at 32/28/26/24 bits, in original, lower and upper case; none of the hashes occurs as a constant in the image. So I could not identify them.
Correction of end 27: it said gamma is a per-binding sampler-state block chosen by the `0x2d` index. The tables I decoded do not support that: the blocks the bindings select do not differ and contain no states, so the index cannot be what separates the BC4 textures from the BC1 texture in one draw. The code path I found that merges sign bits (`0x82376a14`) writes a texture dword (`+0x2c` of a texture-header struct), not a sampler state; my reading of that function was too strong. The only solid facts remain: the `.bix` files carry no gamma sign bits, and the per-binding index is not the switch. The real source of gamma (a format default in the hardware / `0x823c6000`, or something done at texture-set time) is still unknown; the BC4 proxy keeps fitting only three of five families.
Not done: reading `0x823c6000` (texture header builder) and `0x8236c208` (set texture) fully for format-dependent sign handling.

## Update 2026-10-19 (end 29): the texture header builder and set-texture code: no format-dependent gamma

Read `0x823c6000` (a thin wrapper that fixes the texture type and calls `0x823c5e00`) and `0x823c5e00` (the `XGSetTextureHeader`-style builder: computes the layout with `0x8236b6d0` and copies a 0x34-byte header). In `0x8236b6d0` the fetch-constant dword 0 is assembled as type 2 plus `format >> 9` shifted into bits 2 to 9 (sign x, y, z, w), clamp and pitch bits from elsewhere; the sign comes ONLY from bits 9 to 16 of the format word it is given. There is no table, default or special case by data format (no DXT5A rule): the data format (low 6 bits) only selects layout. The format word of every `.bix` file in the road shaders (`0x1a20017b` DXT5A, `0x1a200152` DXT1) has those bits 0, so the headers say "unsigned, no gamma". The only other writer of those bits that I found (`0x8236cb9c`, copying the sign from a device-global default at `device + 0x33b0` into a header copied from another texture object) belongs to device-created textures, not to model textures.
So in the game's code the road textures are NOT gamma-decoded at the fetch. The "BC4 = sRGB" rule therefore stands in for something else: the data say the roads built from DXT5A grains are darker than "linear texture, sRGB display" predicts (Nurburgring 5x, Tsukuba 2.3x, Test Track 2.4x), but not in a way the texture code explains. Candidates (not tested): the display transfer function of the game (the game image has `VIDEOBRIGHTNESS` / `VIDEOCONTRAST` / `exposure` options, so a gamma ramp or brightness default may be applied to the front buffer), or the grain textures being authored in a different encoding than the shader assumes. As a check, treating the Xenia pixel as an unencoded shader output (display gamma 1) fits Nurburgring (0.93) and Test Track (1.14) but not Sunset (2.39), Suzuka (1.86), Mugello (2.74); fitted display exponents per family: 0.93, 1.14, 1.27 (Tsukuba), 2.39, 1.86, 2.74. No single exponent works.
Static analysis has reached its limit here. The decisive data would be a Xenia GPU trace of one frame (it records every draw's fetch constants including the sign bits and the pixel-shader constants); if you can capture one at a track start, that settles the gamma and the display transfer question directly.

## Update 2026-10-19 (end 30): a VMX128-aware listing, and what it showed about the display / gamma question

Tool: `data/work/vmx128.py` (git-ignored scratch) rewrites the `.word` entries of the capstone listing that are Xbox 360 vector instructions into text (`data/work/fm2_xex/text_v.asm`). Decoded: `lvx128` / `stvx128` (load / store with the 7-bit register numbers), the opcode-5 arithmetic (`vmulfp128`, `vmaddfp128`, `vmsum3fp128`, `vmsum4fp128`, `vaddfp128`, logic and pack forms, names from my memory of the console's instruction tables), opcode-6 forms only partly (marked `?` or left as `vmx128_op6 xo=...`). 14,036 of the 53,233 `.word` entries decode; most of the others are real data words inside code (literal pools, jump tables), not instructions. Check: the constant-uploader code now reads correctly, e.g. at `0x825079b8..0x82507a38` the light direction is rotated by three matrix rows (`vmaddfp`) and normalised (`vmsum3fp128`, `vrsqrtefp`, one Newton step, `vsel` for the zero case): this is the view-space light direction of `c101`, the construction I had assumed.
What it says about gamma and display: only 20 `vlogefp` / `vexptefp` sites exist in the whole game (generic polynomial `pow` helpers and a scale-by-power-of-two), none that builds a gamma or display ramp; the strings `brightness`, `contrast`, `exposure` belong to the photo-mode camera effects (with `sepia`, `aperture`, `shutterSpeed`) and `VIDEOBRIGHTNESS` / `VIDEOCONTRAST` to the UI's video widget properties, not to a display setting. So the code gives no sign of a display gamma stage in software; if the console applies one, it is in hardware state set by the graphics library, which the PowerPC listing does not show.
Net: the static route is exhausted for the gamma / display-transfer question. A Xenia GPU trace of one frame (fetch constants, the framebuffer gamma state, and the pixel-shader constants per draw) is the next step that can decide it.

## Update 2026-10-19 (end 31): all twelve tracks, same-pose Xenia shots, BC4 switch on / off

Correct mapping of the user's Xenia shots (`screenshots/xenia2`): x1 Tsukuba (start line, the same view as our default spawn), x2 Test Track, x3 Sunset Infield, x4 Silverstone, x5 Sebring, x6 Road Atlanta, x7 Nurburgring, x8 New York, x9 Mugello, x10 Maple Valley (not Tsukuba: my earlier Tsukuba comparison against x10 was wrong), x11 Laguna Seca, x12 Suzuka, x13 Tsukuba rolling start. Scripts `data/work/shoot_all.sh` (shoots every track at the default spawn) and `data/work/cmp12.py` (left and right near-road bands, ours / Xenia in linear G). Default data (switch off) against the same data re-imported with `FM2_BC4_SRGB=1` (ours / Xenia, left band | right band; the right band is often shadow or wall, so trust the left one):
| track | off | on |
| Tsukuba | 1.73 / 3.30 | 1.08 / 2.10 |
| Silverstone | 2.46 / 1.34 | 1.00 / 1.15 |
| Nurburgring | 3.44 / 0.48 | 0.67 / 0.11 |
| Suzuka | 3.12 / 9.00 | 0.32 / 0.77 |
| Mugello | 0.92 / 1.53 | 1.83 / 3.06 |
| Maple Valley | 0.97 / 3.37 | 1.16 / 6.62 |
| Test Track, Sunset, Sebring, Road Atlanta, New York, Laguna Seca | unchanged (their roads do not go through the baked-road path) |
Sum of |ln ratio| over both bands and all twelve: 17.68 -> 16.42. So the BC4 rule clearly helps Tsukuba (1.73 to 1.08), Silverstone (2.46 to 1.00, a new confirmation) and Nurburgring; it over-corrects Suzuka, makes Mugello worse and Maple Valley slightly worse. A hash-gated whitelist of the validated road shaders (Nurburgring, Tsukuba, Silverstone) would keep the gains and leave Mugello / Suzuka / Maple Valley untouched, but the untouched ones remain 1.0x to 3x off; Laguna Seca (4.1x), Sebring (0.4x), Road Atlanta (1.6x), New York (1.5x), Test Track (right band) are all on other paths and still unexplained.
Installed data was re-imported with the switch OFF again afterwards.

## Update 2026-10-19 (end 32): the shadow term: small lever, not the main error

Checked two things against the twelve same-pose Xenia shots.
1. The engine's light floor `FH1_RM_LM_LIGHT_FLOOR` (0.12; the game's shader has no floor: its dark lightmap colours are 0.08 or below on Nurburgring and negative on Tsukuba, Test Track and Mugello): all twelve tracks shot with the floor at 0 (`screenshots/f0_*.png`) give the same left and right band values as the default to within 3 levels (sum of |ln ratio| 17.68 -> 17.77), so the floor is not what makes our shadowed road bands too bright.
2. The game's shadow strength: in the shader a pixel in shadow gets `min(1 - (1 - c105) * S, lerp(c184, c185, lm))`, with `c105` = TrackSettings `ShadowColor`, so shadow / lit light = ShadowColor / lit light. Per track (lit light taken at lightmap 0.85): Tsukuba 0.43, Test Track 0.41 to 0.51, Sunset 0.49, Silverstone 0.38 to 0.40, Sebring 0.42, Road Atlanta 0.43, Nurburgring 0.32 to 0.36, New York 0.17 to 0.22, Mugello 0.25, Maple Valley 0.31 to 0.33, Laguna Seca 0.33 to 0.35, Suzuka 0.52 to 0.56. The engine uses one constant for the car and prop shadows (`FH1_RM_FM2_DYN_SHADOW` = 0.6, i.e. shadow = 0.4 of lit), which is within 0.2 to 0.55 of the game's values on every track; a per-track value (1 - ShadowColor / lit) would refine it (New York and Mugello should be darker, Suzuka lighter) but it cannot explain the 2x to 9x differences of the right-hand bands, which mostly mix lit road, walls and the car's shadow shape, so those bands are not a clean shadow measurement.
Conclusion: shadow darkness is a small refinement (about 0.2 to 0.5 in ratio), not the main error; the larger errors are the unanalysed road families (Laguna Seca 4.1x, Sebring 0.4x, Road Atlanta 1.6x, New York 1.5x) and Mugello / Suzuka / Maple Valley. Nothing changed in code.

## Update 2026-10-19 (end 33): the six untouched road families, first pass

**What is under the spawn point** (importer `FM2_UNDER=x,z,y`, now with `FM2_UNDER_TOL` for the height tolerance and the shader hash printed): Sebring model 182 (`h_blnd2_ao_lm`, blend of colour textures 402 and 403 by mask 479; hash `f4222c96033cdd0e`), New York model 7 draw 1 (`h_diff_1`, colour texture 33, hash `a851b0ab5755db49`), Tsukuba model 501 (`h_diff_1_lm`, colour texture 148). Laguna Seca, Road Atlanta, Sunset and Test Track: no triangle found under the spawn coordinates within 3 m (the spawn file's coordinates or the road mesh edges do not line up; not resolved).
**New York's road shader** (read in full): `out = 0.6 * tex.rgb * 2 * lerp(c184, c185, lm)` (the literal 0.6 on the texture sample, the doubling on the light) plus a sun term that this draw switches off (`c0` = 0). In the interpreter (`tf1` = texture 33, BC3 colour, with New York's lightmap colours dark (-0.03, 0.007, 0.068), light (1.80, 1.42, 1.22)): with the texture decoded as sRGB (gamma) the lit texel is 0.138 and the tile mean 0.048, without gamma 0.379 and 0.131; Xenia's New York road bands read 0.043 and 0.032. So for BC3 colour textures the ordinary sRGB decode is right (as the engine already does), and the shader's prediction (mean 0.048) matches Xenia (0.043); the engine renders 0.065 (1.5x Xenia).
**The engine's sun "sheen"** (`FH1_RM_FM2_SHEEN`, an added Blinn-Phong highlight on flat grey surfaces, power 24, gain 0.1) matters on four tracks: with it off the left band changes Road Atlanta 1.56x -> 0.46x, Maple Valley 0.97x -> 0.21x, Nurburgring 3.44x -> 2.82x, Silverstone 2.46x -> 2.11x (right band 1.34x -> 0.30x); the other eight tracks are identical. Sum of |ln ratio| 17.68 -> 20.36 (worse): the sheen is compensating for something the game really has. Road Atlanta's and Maple Valley's Xenia shots look into a low sun (visible glare), and those shaders have `c0` = 1 and a real specular term (exponent 8 to 12 from the material block, `GlobalSpecularLevel` 0.6 to 1.7, `c102` white), which only gives a visible highlight when the sun is in front of the camera (my earlier sweep: near zero with the sun behind). So the glare is real, and the engine's single fixed sheen (power 24, gain 0.1, every track) is a stand-in for it.
Candidate next steps: replace the sheen with the game's own term per track (exponent from the material block, `GlobalSpecularLevel`, sun direction from TrackSettings, only where `c0` is 1), and apply the New York finding (texture x 1.2 x light) to the `h_diff_1` family; then re-run the twelve-track comparison. Nothing changed in the engine or importer (only the `FM2_UNDER` diagnostics).

## Update 2026-10-19 (end 34): game-style specular term in the engine (`FH1_FM2_SPEC=<k>`), result: mixed, not adopted

Measured the game's specular term in the interpreter first (Nurburgring road draw, `c0` = 1, lit lightmap texel): `delta = K * GlobalSpecularLevel * c102 * c0 * (sat(dot(H, N)))^p`, linear in the level and in `c102`, zero where the lerp light is below the shader's 0.45 gate, independent of the light amount above it; `K` is a texture sample inside the shader (depends on the BC4 sampler values: 0.157 on Nurburgring with gamma on, 0.421 without) and differs per family: Nurburgring 0.156, Tsukuba 0.106, Test Track 0.401, Suzuka 0.013, Mugello about 0 (K per unit of level and `c102`, aligned geometry, material exponent). So a single constant cannot represent the game; a faithful version needs `K` and the exponent per draw from the importer.
Engine switch (`crates/fh1-engine/src/main.rs`, off unless set): `FH1_FM2_SPEC=<k>` sets the existing sun sheen to power 9.7 and gain `k * GlobalSpecularLevel` (TrackSettings, green channel), explicit `FH1_RM_FM2_SHEEN` / `_POW` still win. Calibration and test with the BC4 switch data installed, ours / Xenia (left band | right band):
| track | current sheen (power 24, gain 0.1) | `FH1_FM2_SPEC=0.05` | no sheen |
| Road Atlanta | 1.56 / 1.47 | 1.05 / 1.63 | 0.47 / 1.47 |
| Maple Valley | 1.16 / 6.62 | 1.02 / 7.35 | 0.42 / 6.62 |
| Nurburgring | 0.67 / 0.11 | 1.36 / 0.24 | 0.15 / 0.04 |
| Silverstone | 1.00 / 1.15 | 2.28 / 1.21 | 0.70 / 0.10 |
| Tsukuba | 1.08 / 2.10 | 1.10 / 2.10 | 1.08 / 2.59 |
Sum of |ln ratio| over the twelve tracks: current sheen 16.42, `FH1_FM2_SPEC=0.05` 16.15, no sheen 22.72 (BC4-off data: 17.68, 17.77 and 20.36 for floor 0 and no sheen). So the game-style highlight helps Road Atlanta and Maple Valley (to 1.05 and 1.02), hurts Nurburgring and Silverstone (high `GlobalSpecularLevel`: 1.7 and 1.5, but their needed highlight is smaller), and overall is a wash (2% better, within the noise of the bands). Removing the sheen is clearly worse. The reason a single level-scaled constant fails is the per-family `K` above and the gate; it needs the per-draw `K` and exponent from the importer.
Decision: not adopted; the switch stays off. Installed data re-imported with the BC4 switch off afterwards.
Next step if continued: have the importer's `shader_albedo` also compute the aligned specular delta per road draw (`K`) and read the material exponent, store both in `materials.json`, and use them in the engine's sheen.

## Update 2026-10-19 (end 35): per-draw specular term from the interpreter + material block, behind switches; result mixed, not adopted

Built the full chain (all off by default):
- Importer `FM2_SPEC_MAT=1`: `fm2_uv.rs` reads each draw's specular exponent from the model's material block (`PsRun::spec_exp`: the one constant register the shader reads and the draw does not set, value found by matching the draw's constants at the 0x14 stride); `fm2_scenery.rs` computes K (`shader_spec_k`: the interpreter's output rise with `c102` and `c104` at 1 and the half vector on the normal; the view and normal interpolator registers are found by trying every pair of non-texture registers and taking the maximum; K must be below 0.7 and the exponent in 3..40; cached by shader code, constants and textures) for EVERY draw, and writes `ps[8] = (K, exponent)` into the material (also in the material key). Counts per track: Nurburgring 731 materials, Sunset 267, New York 215, Sebring 141, Maple Valley 132, Road Atlanta 111, Mugello 109, Suzuka 93, Silverstone 83, Tsukuba 82, Laguna Seca 60, Test Track 22. Exponents come out as 6.4 (Maple Valley), 7.8, 8.5, 9.7, 12.3, 20.3 (Silverstone), 21.6 and 30.3 (Road Atlanta), not one number.
- `remaster.rs` hands them to the shader (p0.y = K, p1.w = exponent) for FM2 lightmap materials; `material.rs` has the term (K x `GlobalSpecularLevel` x (n.h)^exponent x gate where the lightmap light is above 0.45, scaled by `FH1_RM_FM2_SPEC_UNIT` = 0.32 to this renderer's units) used instead of the fixed sheen when `FH1_FM2_SPEC_MAT=1` (engine `main.rs` sets the switch and reads `GlobalSpecularLevel`); materials without data keep the old sheen. `FH1_RM_FM2_SPEC_DIR=x,y,z` replaces the engine sun in this term by a fixed direction.
Results (ours / Xenia, linear, left | right band; sum of |ln ratio| over twelve tracks; BC4 data):
- current fixed sheen 16.42; fixed sheen retuned to the game's exponent and level (`FH1_FM2_SPEC=0.05`) 16.15; per-draw terms on baked roads only 16.70; per-draw terms on every draw (engine sun) **15.92**.
- With the engine sun, Nurburgring's road overshoots (left 4.32x against 0.67x with the old sheen). With the game's own default light direction from its code (the fixed vector (1, -1, 0), i.e. toward-light (-1, 1, 0); `FH1_RM_FM2_SPEC_DIR=-1,1,0`), Nurburgring becomes 0.70 | 1.09 (the best of any variant), but Silverstone drops to 0.70 | 0.10 and Road Atlanta, Maple Valley are unchanged (their road materials carry no usable term: their glare is not reproduced), so the four sun-sensitive tracks sum to 5.95 against 5.57 for the old sheen and 5.14 for per-draw terms with the engine sun.
Reading: the game's own term is now computed from its data per draw, and the light direction literal from the game's code fixes Nurburgring, but one engine-wide setting that improves all four glare tracks does not exist yet: Road Atlanta's and Maple Valley's glare is not explained by their draws' own terms (K may be zero for their road shader family or my pair search misses its registers), and Silverstone's shadowed right band disagrees. Left as switches (`FM2_SPEC_MAT`, `FH1_FM2_SPEC_MAT`, `FH1_RM_FM2_SPEC_DIR`, `FH1_FM2_SPEC`); the default behaviour is unchanged. Installed data is re-imported with all switches off.
Next step if continued: check why Road Atlanta's and Maple Valley's road draws give no term (print K per draw for the road under the camera, using `FM2_UNDER`), and decide the light direction question with a Xenia trace (what `c101` really holds).

## Update 2026-10-19 (end 36): BC4 sRGB shader albedo restricted to validated road shaders (hash whitelist)

`crates/fh1setup/src/fm2_scenery.rs`: under `FM2_BC4_SRGB=1` the shader-derived road albedo is now applied only to road draws whose pixel shader hash is in `BC4_ROAD_SIGS` (Nurburgring `4aa0eec057a0743d`, Tsukuba `19026bd2593921ac` = model 50's road, Silverstone `5cd0e87d36cd8908` = its dominant road, 6,565 triangles); `FM2_BC4_SRGB_ALL=1` lifts the restriction. The Nurburgring colour-blend rule stays hash-gated as before. Twelve-track comparison with the same-pose Xenia shots (ours / Xenia, linear G, left | right band; `wl` = whitelist):
- Tsukuba 1.73 / 3.30 (off) -> 1.08 / 2.03; Silverstone 2.46 / 1.34 -> 0.97 / 1.15; Nurburgring 3.44 / 0.48 -> 0.67 / 0.11 (the right band is mostly wall and shadow).
- Suzuka, Mugello and Maple Valley are back to exactly the default (3.12 / 9.00, 0.92 / 1.53, 0.97 / 3.30); the six other tracks never change.
- Sum of |ln ratio| over the twelve: default 17.68, unrestricted switch 16.42, whitelist **16.24** (the best BC4 variant: it keeps all gains and drops the two regressions). Remaining big errors: Laguna Seca 4.09 / 2.48, Suzuka 3.12 / 9.00, Test Track right band 7.41, Maple Valley right band 3.30, Sebring 0.40, Road Atlanta 1.56, New York 1.48.
Still off by default. Installed data re-imported with the switch off afterwards. Not tested: a race.

## Update 2026-10-19 (end 37): the whitelisted BC4 sRGB road albedo is now the importer's DEFAULT

`bc4_srgb_on()` in `crates/fh1setup/src/fm2_scenery.rs`: on unless `FM2_BC4_SRGB=0`. Effect: the Nurburgring colour blend (hash-gated) and the shader-derived road albedo of the three whitelisted road shaders (Nurburgring `4aa0eec057a0743d`, Tsukuba `19026bd2593921ac`, Silverstone `5cd0e87d36cd8908`); all other draws are exactly as before. Re-imported all FM2 tracks with no environment variables, rebuilt the remaster tables, and shot all twelve tracks (`screenshots/defon_*.png`): same result as the whitelist run (sum of |ln ratio| 16.19 against 17.68 for the old default; Tsukuba 1.05 / 1.97, Silverstone 0.97 / 1.15, Nurburgring 0.67 / 0.11; the other nine unchanged within noise). To get the previous output: `FM2_BC4_SRGB=0` on the import (then rebuild the remaster tables).
Side effect: `FM2_SHADER_ROAD=1` (the old opt-in) now also decodes BC4 textures as sRGB unless `FM2_BC4_SRGB=0`.
The installed data in `data/installations` is the default-on import. Nothing is committed. Not tested: a race, other layouts of the same tracks (their data came from the same import).

## Update 2026-10-19 (end 38): Laguna Seca's road (4.1x too bright): what draws it, first findings

Method: the binary-code painting test (`data/work/bitpaint.py <track> x0 y0 x1 y1`: 12 shots, each paints the textures whose index has one bit set red; the patch's red / not red pattern gives the texture file; restores and md5-checks at the end). It found the lightmap tile `0x200` (512) as the texture painting the road, which only reveals the lighting tile of one draw (model 291, 252 triangles, centred 770 m from the spawn, so not the road at the spawn; its lightmap is mostly 0.13). New importer diagnostics `FM2_FIND_TEX=<hex id>` and `FM2_NEAR=x,z,radius` (print draws using a texture / near a point with shader hash, lightmap tile and bounding box) located the draws within 12 m of the spawn instead: model 81, three draws: (a) 21 triangles, shader hash `34e8d111b0b2f980` (the plain diffuse shader `h_diff_1_lm`: `out = tex * 2 * lerp(c184, c185, lightmap)`, same hash as Mugello's biggest road), colour texture 6 (BC1, 1024x1024, pale), lightmap tile 175 (32x32, values about 0.52 over most of it); (b) 33 triangles and (c) 7 triangles, hash `e1ba186ea8211ca9` (a blend: colours 6, 132 / 141 and mask 176, same tile 175). Box of (a): x -390..-194, y -3..-1, z 203..357 (the straight under the camera).
Interpreter on (a) with the real tile and Laguna's lightmap colours (dark -0.14, light 1.47): texture 6 averages 0.585 linear, so `2 * tex` is 1.17; lit light about 0.70; result lit max 0.787, mean 0.716. Xenia's road near the car reads 0.055 (left) and 0.122 (right); the engine renders 0.22 (albedo 0.45 to 0.47 at the road, lightmap sample 0.51, light term 0.64, product 0.28 after the engine's missing x2).
Reading: draw (a) alone predicts a pale road (0.7), which is not what Xenia shows there (0.055), so either the blend draws (b) and (c) cover the visible road with a dark asphalt (a mask-selected dark layer) and the engine mixes them wrongly, or (a) is not visible at that spot in the game (an overlay drawn above it). Both point at the blend `e1ba186ea8211ca9` (cands 6, 132 / 141, mask 176), not at lighting or gamma. Not done: run the blend shader in the interpreter (it needs the mask texture 176 and its layer order) and look at the mask texture; check Mugello / Silverstone / Maple Valley for the same `e1ba186e` blend.
Also noticed: the engine's FLAG_LM_LIGHT path has no `x 2` on the light term for the plain diffuse family (the game's `h_diff_1_lm` doubles it); not changed.

## Update 2026-10-19 (end 39): Laguna Seca, the blend shader and a contradiction I could not resolve

- **The `e1ba186ea8211ca9` blend** (Laguna model 81, group 1, 57 dwords, samplers: tf0 lightmap tile, tf1 = mask 176 (BC4 512x256), tf2 = layer A texture 6, tf3 = layer B texture 132): `albedo = B + (A - B) * mask` (mask = the BC4 value weighted by the luma literal, which sums to 1), then `out = albedo * 2 * lerp(c184, c185, lightmap)`, fog lerp. Mask 176 averages 0.5 (flat grey on average), texture 6 (1024x1024 BC1) is pale (bytes 160,151,129, linear G 0.32), texture 132 (128x128 BC1) is mid grey (126,124,122, linear 0.21). Interpreter with raw bytes (no sRGB decode of the BC1 colours) gives 2*albedo 0.9 to 1.1 and 0.55 to 0.65 with the lightmap; with the correct sRGB decode of colour textures about 0.2 to 0.35, still well above Xenia's 0.055.
- **Same shader elsewhere:** `FM2_FIND_SIG=<hex>` (new) counts draws per track: this blend has 174 draws on Laguna Seca, 6 on Maple Valley, 4 on Tsukuba, 2 on Nurburgring, 2 on Test Track, 1 on Mugello, none elsewhere, so it is essentially Laguna's road / ground blend.
- **What paints the near road**: binary-code painting (`bitpaint.py`, now skipping the small lightmap tiles) says texture file `0xbe` (190, a 128x128 BC1, linear G 0.22) paints the road left of the car; painting textures 6 and 132 only paints far grass. But the importer's draws within 60 m of the spawn point (model 81: textures 6, 132, 141, 176, tile 175) do not use texture 190, and the draws that do use it (hash `1eb344c3a9086634`, a palette family with c2 0.45, c3 0.55, c5 1.2 and eight textures, models 376, 978, 94, 467, ...) are elsewhere. So the draw-to-screen mapping for Laguna is not understood: either the engine replaces some material's colour texture (blend / colour choice), or the spawn point's coordinates and the draws' frame differ. Not resolved; this needs the material choice printed per draw near the spawn (the `colours` / `masks` the importer picks for each candidate list), which I have not done.
- **Lightmap**: Laguna tile 175 reads about 0.52 over most of its texels while the game's value at Xenia's dark road would have to be about 0.14 (tile 0x200's body is 0.13); 32x32 tiles on Laguna are generally bright (most means 96 to 128), so this is not a crop error in general. The Xenia Laguna shot (x11) is taken under the large overhead Mazda / Coca-Cola banner: the road there is in the real-time shadow of that structure, which the engine does not reproduce (its sun shadow only darkens lit lightmap areas by 0.6), so part of the 4x is shadow, not shading.
Importer diagnostics added (printing only): `FM2_FIND_TEX`, `FM2_NEAR=x,z,radius`, `FM2_FIND_SIG`. Nothing else changed; default imports unchanged.
Next step if continued: print, for the draws near the Laguna spawn, which textures the importer finally binds (material id and its textures) and compare with the painting result; separately, measure the engine's shadow under the banner against Xenia's.

## Update 2026-10-22: Laguna Seca, the banner shadow: not the explanation

- Sun: `ShadowLightDirection` (1.0, -0.85, 0.9) = 32 degrees elevation, travelling opposite to the camera's forward direction at the spawn (spawn facing (-0.60, 0, -0.80); horizontal cos -0.98): the sun is IN FRONT of the camera and low, so the overhead banner (about 25 m ahead of the car, a few metres above the road) casts its shadow toward the camera, a few metres to about 10 to 15 m long (1.6 x height), not over the whole near road.
- The engine's real-time sun shadows work: with `FH1_RM_FM2_DYN_SHADOW=1.0` the left band falls from 5.13x to 3.36x of Xenia (a shadowed patch), the right band does not change. Switching off the small-caster culling (`FH1_SHADOW_SMALL_CULL=0`), or making the cascades much larger (`FH1_RM_SHADOW_DIST=900`, `FH1_RM_SHADOW_NEAR=30`) changes nothing at this view, so neither the caster flags nor the cascade extent hides a banner shadow. The engine's cap on the shadow strength (0.6 default) is the only limit, and even strength 1.0 leaves the road 3.4x too bright on the left.
- So the overall brightness gap on Laguna's road is not a shadow problem. Our road here renders pale (albedo 0.45, engine sRGB (122, 118, 105) on the left band) where Xenia shows warm dark grey asphalt (76, 68, 57). The painting test says texture 190 (`0xbe`, 128x128 BC1) paints the road left of the car, and the draws that use it belong to the shader family `1eb344c3a9086634` (c2 0.45, c3 0.55, c5 1.2, eight textures with BC4 grains 191, 193, 1274 and a 1x1 placeholder 73; 109 triangles in model 978 and similar ones); that family is exactly the kind of BC4-textured road the BC4-is-sRGB rule fixed on Test Track, Tsukuba and Nurburgring, but it is not in the whitelist yet. The interpreter run on model 978 returned 0 with my generic setup (`lit_model.py` does not set this family's constants and `c102`), so the lit value is not measured yet.
Next step if continued: run model 978's pixel shader with the real constants and the family's registers (its `tf2` is a BC3 lightmap tile with flags 9, `tf3` a 1x1 placeholder), check the lit value with BC4 gamma against Xenia (0.055 left, 0.122 right), and add the hash `1eb344c3a9086634` to `BC4_ROAD_SIGS` if it fits.

## Update 2026-10-22 (2): the 1eb344c3a9086634 family (Laguna model 978): does NOT fit the BC4 rule; per-family gamma masks fit everything

Interpreter run of Laguna model 978 (group 0, draw 0; fog `r7.x`, lightmap uv `r4`, atlas `c170`; the specular exponent register `c4` set to 9.7; Laguna lightmap colours dark -0.14, light 1.47; the runtime placeholder `tf3` (texture 73, 1x1 black, flags 1) replaced by the solid white texture, because with it black the whole output is 0): lit value (max of the lightmap grid, G, linear) for sampler gamma choices: none 0.127; BC1 colour `tf4` (texture 190) as sRGB only 0.055; BC4 `tf5 tf6 tf7` as sRGB only 0.052; BC1 and all BC4 as sRGB 0.022. Xenia: 0.055 (left band, partly shadowed) and 0.122 (right band). So the hash is NOT whitelisted: the rule "BC4 textures are sRGB" with ordinary sRGB colour textures gives 0.022 (0.4x of the left band), where "colour sRGB only" or "BC4 sRGB only" give 0.055 and 0.052 (the left band), and "none" gives 0.127 (the right band): the family is consistent with either, so it needs the real per-sampler setting.
Summary of what fits the Xenia sunlit values in the interpreter, per shader family (sampler indices that must be sRGB-decoded): Nurburgring 118 `tf4..tf7` (BC4 grain greys; its BC1 `tf3` is a weight and stays linear); Tsukuba 50 `tf3..tf6` (all BC4); Test Track 321 `tf3..tf5` (all BC4); New York `h_diff_1` `tf1` (BC3 colour); Laguna `1eb344c3` `tf4` or `tf5..tf7`; Sunset 77 none (BC1 colour texture 252 must stay linear: gamma gives 0.02 against 0.16); Silverstone fits the same BC4 rule as Nurburgring / Tsukuba. The only consistent statement is "textures that carry the surface colour are sRGB in most families, with Sunset's BC1 as an exception", i.e. a per-family table of gamma samplers (hash -> sampler mask), which the importer could hold (`BC4_ROAD_SIGS` is already a hash list, only without the per-sampler mask).
Not changed in code. Not tested: the engine on Laguna with a per-family albedo (the family has a colour texture, so it does not go through the baked-road path; it would need the shader albedo applied as a brightness scale to texture 190).

## Update 2026-10-22 (3): per-family gamma table in the importer

`crates/fh1setup/src/fm2_scenery.rs`: `FAMILY_GAMMA` (shader hash -> sampler mask to decode as sRGB; `BC4` = every BC4 sampler) replaces the three-hash whitelist: Nurburgring `4aa0eec057a0743d`, Tsukuba `19026bd2593921ac`, Silverstone `5cd0e87d36cd8908`, Test Track `21d6c8618fa05dd2` (all BC4 samplers), New York `a851b0ab5755db49` (tf1), Laguna Seca `1eb344c3a9086634` (tf4), Sunset `bef0b122d3daf8b8` (none; the hash is the neighbouring model 188's draw of the same layout, not model 77's own, unverified). The interpreter run (`shader_eval`) applies the mask per sampler (explicit bits and the BC4 flag), reads runtime placeholder textures as white (`WHITE_TEX`), and `family_gamma(p)` gates the baked-road shader albedo. `FM2_BC4_SRGB=0` still turns everything off, `FM2_BC4_SRGB_ALL=1` still applies the BC4 rule to every road draw.
New (opt-in, `FM2_FAMILY_COLOUR=1`): draws of these families that carry a COLOUR texture get it scaled to the brightness of the shader (per channel: shader colour with neutral light divided by the texture's mean sRGB-decoded colour; new textures `ffff5000+`). Result, with it on: 55 Laguna, 11 New York, 66 Sunset, 1 each on two other tracks; twelve-track sum of |ln ratio| 16.19 (table only) -> 16.27: no gain. New York's scales come out 1.1 to 1.3 (brighter, the wrong way), and the visible roads at the spawn of Laguna (model 81, other hashes), New York and Sunset were not in the affected draws (bands unchanged), so the switch stays off.
Default state after this change (colour scaling off): twelve-track sum 16.30 against 16.19 for the previous whitelist build: the same results to within the measurement noise (Tsukuba right band 1.97 -> 2.16, Maple Valley right 3.30 -> 3.17). Installed data is the default import of this build. Not tested: a race.
Remaining large errors (ours / Xenia, left | right): Laguna Seca 4.09 / 2.48, Suzuka 3.12 / 9.00, Test Track right band 7.41 (shadow), Maple Valley right 3.17, Tsukuba right 2.16, Mugello right 1.53, Road Atlanta 1.56 / 1.41, New York 1.48 / 1.62, Sebring 0.40 / 1.05. The Laguna road at the spawn is made of draws whose hashes are not in the table (`34e8d111b0b2f980` plain diffuse and `e1ba186ea8211ca9` blend); the next step is to measure those two the same way and add them.

## Update 2026-10-22 (4): Laguna's two spawn-road shaders measured: no gamma setting can fit them

Shader code (`fm2_show` on Laguna model 81): `34e8d111b0b2f980` (group 0, 39 dwords) is `out = colour(tf1 = texture 6) * 2 * lerp(c184, c185, lightmap tile 175) `, fog lerp. `e1ba186ea8211ca9` (groups 1 and 2) is the same with `albedo = B + (A - B) * luma(mask 176)` (textures 6 and 132/141/128). The tile `0xaf` (175) is 32x32 and reads about 132/255 (0.52) over the whole region these draws use (uv 0.69..0.96 x 0.16..0.35; its only dark patch, 56..77, is at v 0.65..0.8, not used). With Laguna's lightmap colours (dark -0.14, light 1.47) the lerp is 0.70, so the lit G is 2 x 0.70 x albedo: texture 6 gives 0.45 with sRGB decode and 0.82 without (linear G of its mean 0.32 / 0.59), blend with texture 132 about 0.3 to 0.4. Xenia reads 0.055 to 0.12. The gamma choice (sRGB or not) moves the answer by 1.8x; the gap is 4x to 8x. So neither hash is added to `FAMILY_GAMMA` (that would change nothing useful). What would be needed is a lerp of about 0.09 (lightmap about 0.14), i.e. the road is far darker in Xenia's frame than the baked tile plus the shader say: the remaining candidates are a dynamic shadow strength under the overhead banner (the engine's reproduces only 0.6 of the lit value; the game's `ShadowColor` ratio is 0.33 to 0.35) and the post-process luminance adaptation, neither verified against Xenia. Not tested: Xenia's own shadow under the banner.
Side note: the first run of this step used `target/release/fh1setup.exe` (built Oct 15, older than the source) with `FM2_TRACKS=lagunaseca`, which re-imported Laguna with the old importer; it was redone at once with the current `target/debug/fh1setup.exe` (same 2790 materials, same counts). Use the debug exe (or rebuild release with `cargo build --release -p fh1setup`) for imports.

## Update 2026-10-22 (5): is there a missing ambient term (c100 / LightAmbient) in Mugello's and Suzuka's road shaders? No.

The full pixel-shader listings of Mugello model 3290 and Suzuka model 867 (group 0) read these constants and no others: c0 (sun scale, 0 on these draws), c1..c7 (the draw's palette), c101 (sun direction), c102 (light colour), c104 (GlobalSpecularLevel), c105 (shadow colour), c108/c109 (shadow fade), c114 (fog colour), c170 (lightmap atlas), c184/c185 (lightmap dark/light), c253..c255 (literals). **c100 (LightAmbient, 0.3) is not read by either shader**, so there is no ambient/SH term to add from the game's constants; the 0.05 to 0.07 missing on Mugello (0.060..0.081 against Xenia 0.133) is not from that. Suzuka has no clean Xenia reference (shot is in shadow). Remaining candidates stay as listed in "end 24": constants inherited from earlier draws, the display exponent (2.0 vs 2.2 would lift all Xenia values by about 1.2x), or the Xenia patch not being this surface. No code change; not tested beyond reading the listings.

## Update 2026-10-22 (6): Xenia decoded at exponent 2.0 instead of 2.2

Xenia lit G (linear, 2.2) re-decoded at 2.0 is value^(2.0/2.2): Nurburgring 0.058 -> 0.075, Tsukuba 0.117 -> 0.142, Test Track 0.170 -> 0.200, Mugello 0.133 -> 0.160, Suzuka 0.051 -> 0.067. Interpreter lit values from `lit_per.py` with the family gamma table (Nurburgring all-BC4 0.070, Tsukuba all-BC4 0.114, Test Track all-BC4 0.125, Mugello BC4 0.060, Mugello none 0.081). ln(ours / Xenia) at 2.2 vs 2.0: Nurburgring +0.19 / -0.07, Tsukuba -0.03 / -0.22, Test Track -0.30 / -0.48, Mugello (BC4) -0.80 / -0.98. Sum of |ln| 1.32 at 2.2 against 1.75 at 2.0, so 2.0 does NOT fit better; every family moves by the same 1.2x, so the choice cannot separate the families' gamma masks (they were fitted at 2.2) and it does not close Mugello's gap (0.081 against 0.160 is still 0.5x). Result: keep 2.2. Not tested: engine renders re-decoded (this is arithmetic on the lit-texel numbers only).

## Update 2026-10-22 (7): car-shadow strength, right bands (experiment, no default change)

The right bands (car shadow in Xenia: 0.06 to 0.3 of the lit road) make up about 6 of the 16.3 total. Game data: `ShadowColor` gives shadow/lit of 0.3 to 0.5; `DropShadowColor` is black on most tracks (Suzuka 0.06) and is stored at R+0xee0 by ApplyTrackSettings, but I did not find its consumer in the PowerPC code; the `dropShadow` technique (`fm2_fxdump ... dropShadow`, saved in `data/work/fm2_fx/dropShadow.txt`) is a blob with a falloff (`c_dropShadow` c184: edge params + opacity in w), its blend state is not in the effect, so how it multiplies the road is not known. Experiment: `FH1_RM_FM2_DYN_SHADOW=0.9` (default 0.6), right band ours / Xenia: Tsukuba 2.16 -> 1.28, Test Track 7.41 -> 1.70, Maple Valley 3.17 -> 3.23 (no change), Suzuka 9.00 -> 9.00 (no change); left bands unchanged; four-track sum 7.41 -> 5.44. Not adopted as a default (the 0.9 is fitted to Xenia, not read from the game). Maple Valley and Suzuka right bands must be something else than the engine's dynamic car shadow (not a shadow receiver there, or a different surface). Not tested: other tracks with 0.9, a race.

## Update 2026-10-22 (8): why Suzuka's and Maple Valley's right bands ignore the shadow strength

Looking at the shots: Xenia's Suzuka and Maple Valley frames hold large dark shadows (Suzuka: long strips either side of the car and ahead; Maple Valley: the overhead bridge's shadow and a wide dark area at lower right) that ours does not have; Suzuka has no car shadow at all in ours. The engine casts shadows from the Forza Horizon time-of-day sun azimuth by default (`light.rs`, `FH1_RM_SUN_DIR=shadow` switches to TrackSettings `ShadowLightDirection`). Tried `FH1_RM_SUN_DIR=shadow` on Suzuka, Maple Valley and Test Track (ours / Xenia, left | right): Suzuka 3.12 | 9.00 -> unchanged (still no visible car shadow), Maple Valley 0.97 | 3.17 -> 0.09 | 1.35 (left band collapsed: the whole road went dark), Test Track 0.96 | 7.41 -> 2.72 | 15.89 (worse). Not adopted. So the game's shadow direction is not simply the XML vector in our conversion, or the engine's single-light shading ties the light to the shadow direction (the comment in `light.rs` says exactly that), so the direction cannot be changed without changing the lit shading. Not tested: whether the Suzuka / Maple Valley road draws are flagged as shadow receivers (the engine only darkens lit lightmap areas by the strength x lightmap), the x-sign of ShadowLightDirection, a separate shadow-only direction in the engine.

## Update 2026-10-25: shadow-only light direction (`FH1_RM_FM2_SHADOW_DIR`), result: no variant helps; Suzuka's road shows no real-time shadow at all

`light.rs`: new env `FH1_RM_FM2_SHADOW_DIR=full|fz|fx|fxz` sets the sun light's transform (the light that casts shadows and lights non-lightmap things) to the TrackSettings `ShadowLightDirection` (elevation included; fz / fx / fxz mirror z / x / both of the converted vector); shading values (`sun_dir` state, lux, colour) are untouched. Off by default. Left | right band, ours / Xenia: default Suzuka 3.12 | 9.00, Maple Valley 0.97 | 3.17, Test Track 0.96 | 7.41 (sum 6.56); full 3.12 | 9.00, 0.08 | 1.35, 2.79 | 14.71 (9.91); fz 3.12 | 9.00, 0.18 | 1.35, 0.93 | 34.49 (8.96); fx 3.26 | 9.00, 0.37 | 1.53, 0.96 | 8.23 (6.93); fxz 3.12 | 8.79, 0.21 | 1.44, 0.94 | 18.73 (8.22). Every variant is worse than the default, mostly because Maple Valley's left road goes dark (the whole road lands in shadow).
Suzuka: no shadow on the road in any variant (the car body's lighting changes, the road does not), so Suzuka's road draws do not take the engine's real-time shadow at all: either they are not flagged as shadow receivers or they go through a material path without the `RM_FM2_DYN_SHADOW` term. That, not the direction, is why its right band never moves. Not tested: the receiver flag and the material path of Suzuka's road draws (next step).

## Update 2026-10-25 (2): why Suzuka's road shows no real-time shadow: the lightmap gate

The scenery pipeline always sets the receiver bit (`rm_mesh_flags`, bit 29), so the receiver flag is not the problem. The dynamic term in `material.rs` is multiplied by `lit_lm = smoothstep(0.35, 0.75, mean lightmap)`, and Suzuka's road lightmap is dim, so the term is 0 there. New switch `FH1_RM_FM2_DYN_LIT=1` (default 0) forces `lit_lm = 1`. With it and `FH1_RM_FM2_DYN_SHADOW=0.9`: Suzuka now shows the car's shadow on the road at the lower left of the car, where Xenia also has a dark patch (the default sun direction was right all along); band ours / Xenia left | right: 3.12 | 9.00 -> 0.34 | 9.00 (the left band now catches the shadow; the right band is unchanged), Maple Valley unchanged (0.97 | 3.23). So the long dark area right of the car in Xenia's Suzuka frame is not the car's shadow: it is the large building / structure shadow (static, from the game's own lightmaps or shadow) that our lightmap does not hold at that spot. Not adopted as default (the game's own shadow formula is `min(1 - (1 - ShadowColor) * S, lerp(dark, light, lightmap))`, i.e. it darkens only where the lightmap light exceeds the shadow level; Suzuka's ShadowColor 0.5 against its dim lightmap would barely darken, so forcing it is not the game's rule). Not tested: the game's formula in the engine's shader (use `lerp` and `ShadowColor` instead of the smoothstep gate).

## Update 2026-10-25 (3): the game's own shadow rule in the FM2 lightmap shader (`FH1_FM2_SHADOW_GAME=1`)

`material.rs`: with `FH1_RM_FM2_SHADOW_GAME=1` (set by the engine's `FH1_FM2_SHADOW_GAME=1`, which reads the track's TrackSettings `ShadowColor` green into `FH1_RM_FM2_SHADOW_COL`) the real-time shadow factor is `min(1 - (1 - ShadowColor) * (1 - sh), lerp(dark, light, lm)) / lerp(dark, light, lm)` per channel (shadow value one scalar for all channels) instead of `1 - 0.6 * smoothstep(lm) * (1 - sh)`. Off by default. Twelve shots (`screenshots/sg_*.png`), ours / Xenia left | right, against the default: New York 1.48 | 1.62 -> 0.76 | 0.80, Laguna Seca left 4.09 -> 3.06, Silverstone 0.97 | 1.15 -> 0.95 | 1.13, Tsukuba 1.08 | 2.16 -> 1.05 | 2.44, Test Track right 7.41 -> 8.87 (worse), Road Atlanta right 1.41 -> 1.47; Sunset, Sebring, Nurburgring, Mugello, Maple Valley and Suzuka unchanged (their roads are dim enough, or not in shadow, that the min() does not bite). Sum |ln ratio| 16.30 -> 15.94. So the game's rule (data from its shader) beats the fixed 0.6 overall, mostly on New York (ShadowColor 0.17 to 0.22: much darker shadow) and Laguna; it makes Test Track's car shadow lighter (Xenia's is darker still, 0.011 against 0.184 lit), which means the real car shadow also has a second darkening (the game's `dropShadow` pass, not understood). Not adopted as default yet; not tested in a race.

## Update 2026-10-25 (4): game shadow rule is now the default

`FH1_FM2_SHADOW_GAME` is on by default for FM2 tracks (`crates/fh1-engine/src/main.rs`); `FH1_FM2_SHADOW_GAME=0` restores the fixed strength (`FH1_RM_FM2_DYN_SHADOW` 0.6 with the lightmap gate). Reason: the rule is the game's own (its road pixel shader) and the twelve-track sum improved 16.30 -> 15.94 (New York, Laguna). Known cost: Test Track's right band 7.41 -> 8.87 and Tsukuba's 2.16 -> 2.44 (the game's car shadow darkens more than this rule gives; the `dropShadow` pass is not understood). Checked after the change: one New York shot with no env set (left 0.76 as the `sg` shot, right 0.88 against 0.80, so the rule is active; the info log line did not show up in the log, not investigated). Not tested: a race, other tracks after the default change.

## Update 2026-10-25 (5): the game's car contact shadow (`CDropShader` / `dropShadow` technique) now exists for FM2 cars

Findings (game data): the `dropShadow` technique (`fm2_fxdump ... dropShadow`, `data/work/fm2_fx/dropShadow.txt`) is a top-down blob of the car whose opacity falls off past a soft edge (`c_dropShadow` c184: edge start/stop, falloff scale, opacity w); the road shaders' own shadow comes from the shadow map and `ShadowColor` only (previous update). FM2's `GlobalCarAttribs.xml` `DropShadowSettings`: MaxHeightAboveGround 0.30, FallOffCurveStart 0.85 / Stop 0.60 / StopScale 0.15, body width scale 1.06 to 1.09, length 1.05 to 1.10, opacity 0.80 (on the ground) to 0.60 (at 0.3 m), wheels scale 0.65 / offset -0.075 / opacity 0.85. So the blob darkens the road under and around the car to about 0.2 of its light. The engine already has a port of this shader (`fh1-render` `car_shadow/drop_shadow.rs`, FH1 values, almost the same as FM2's body values), but it only collected body triangles from game-shaded car parts, so FM2's plain glTF cars never got one ("drop shadows 0" in the census). Change: `collect_silhouettes` also takes glTF body parts (`Added<Aabb>`, standard material, skipping nodes named wheel / lexan / light / seat / steering / interior / mirror / glass / window / brake / tire / rim); `FH1_FM2_DROPSHADOW=0` turns it off. Log shows a silhouette of 14480 triangles on Suzuka.
Result: a dark fringe appears under and behind the car; right / left band ours / Xenia: Tsukuba 1.05 | 2.44 -> 1.05 | 2.30, Test Track 0.96 | 8.87 and Suzuka 3.12 | 9.00 unchanged, Maple Valley 0.97 | 3.17 -> 0.97 | 3.23. The blob is only a thin fringe around the body, so it does not reach the wide dark area right of the car in Xenia's frames: the 0.06 to 0.3 shadow / lit ratios measured there are NOT explained (the rule `min(c105, lerp)` gives about 0.4 of the lit light in linear terms; Xenia shows 0.06 on Test Track and 0.08 on Maple Valley). Not tested: whether the game multiplies this term in gamma space (0.4 ^ 2.2 = 0.13, closer, but the lit-texel calibrations treated the shader output as linear), FM2's own wheel values (the engine uses its defaults 0.55 / -0.12 / 0.8 because no `FxCarBody` supplies GlobalCarAttribs), AI cars.

## Update 2026-10-25 (6): shadow factor in gamma space (`FH1_RM_FM2_SHADOW_POW`, default 1.0 = linear)

`material.rs`: the game-rule shadow factor `lit / lerp` is raised to `FH1_RM_FM2_SHADOW_POW` (env, default 1; 2.2 = the shadow multiplied in display space). Six tracks, left / right ours / Xenia, linear (`ds1`, with the car drop shadow) -> power 2.2 (`sp`): Test Track 0.96 | 8.87 -> 0.96 | 3.59, Tsukuba 1.05 | 2.30 -> 1.05 | 1.23, Laguna Seca 3.06 | 2.48 -> 1.63 | 2.48, Maple Valley 0.97 | 3.23 -> 0.95 | 3.17, Suzuka unchanged (3.12 | 9.00), New York 0.76 | 0.80 -> 0.08 | 0.08 (collapses: its ShadowColor is 0.17 to 0.22 and its whole road is in shadow). Sum of |ln| over these six: 10.16 -> 12.58; without New York 9.67 -> 7.52. So gamma-space shadows fit Test Track, Tsukuba and Laguna clearly better but wreck New York, and do nothing for Maple Valley and Suzuka (their dark areas are not this term). No single exponent fits; not adopted (default stays linear). Possible reading: the game's road is shaded in display space, but the lit-texel calibrations treated the output as linear; New York's roads would then be darker than Xenia's for another reason (its lightmap, ShadowColor use). Not tested: an intermediate exponent (would be a fit, not game data), why New York differs.

## Update 2026-10-25 (7): New York vs Test Track shadow numbers: no single rule

Debug switch `FH1_RM_FM2_SHADOW_DBG=1` (default off, `material.rs`) paints the road with `(sh^3, lerp.g / 2)` where `sh` is the engine's shadow-map visibility and `lerp` = `lerp(dark, light, lightmap)` (green), decoded from the shots `sdbg_*`. Band medians: New York spawn road: sh 0.03 to 0.04 (fully shadowed, left and right), lerp.g 1.07; TrackSettings ShadowColor g 0.24, light colour g 1.425. So the game's rule gives f = min(0.24, 1.07) / 1.07 = 0.22 of the lit light there: linear output is 0.76 / 0.80 of Xenia (road absolute 0.043), power 2.2 gives 0.08 (f^2.2 = 0.035). New York therefore says the shadow factor is applied LINEARLY (and its road is in full shadow).
Test Track spawn: left band sh 0.99 (lit), lerp.g 0.65; right band sh only 0.19 (partly shadowed in our render); ShadowColor g 0.31, so a fully shadowed pixel has f = min(0.31, 0.65) / 0.65 = 0.48 (linear) or 0.20 (power 2.2), against Xenia's right / left of about 0.06. Neither reaches 0.06, and our band is only partly in shadow, so the right band mixes lit and shadowed road and cannot be read as a shadow ratio.
Conclusion: the New York numbers (fully shadowed road, linear fits) contradict the gamma-space idea; the Test Track right band is a poor measure (partial coverage, shadow footprint differs from Xenia's because of the sun direction / camera pose) and still unexplained by either exponent. The default stays linear (`FH1_RM_FM2_SHADOW_POW` 1). To settle it, a Xenia frame with a fully shadowed lit-comparable road patch of known lightmap is needed (e.g. under a bridge on Maple Valley or New York's shaded streets against the same road lit). Not tested: that comparison.

## Update 2026-10-25 (8): Sebring's 0.40 is a framing difference, not a shading error

Looking at `screenshots/xenia2/x5.png` against our `sg_sebring_00.png`: the left near-road band in Xenia (screen x 8 to 30 %, y 78 to 94 %) is pale concrete (byte G 180), while at the same screen position our frame shows the dark asphalt strip (G 119) with the pale concrete only further left and lower. The cars sit at different distances / positions relative to the track layout (our camera is further back and lower; the pit-lane concrete in our frame is as bright as Xenia's). So the 0.40 ratio compares different surfaces. The pale concrete itself looks right in both. The same check should be made for Road Atlanta (1.56), New York (1.48 before the shadow rule) and Suzuka's left band before treating any of them as a shading error: they are not like-for-like patches either. A fair comparison needs the same car position (the old note "Teleport our Tsukuba car to match the Xenia x10 spot" is the method: `FH1_TELEPORT`). Not done for Sebring.

## Update 2026-10-25 (9): matching our car to Xenia's spots: not workable automatically

`FH1_TELEPORT=x,y,z,yaw` works on FM2 tracks (scripts `data/work/tp_shot.sh`, `data/work/ncc.py`), with these facts: the spawn file's `position` uses the XML's left-handed z, so the engine point is (x, y, -z) (the raw value put the car inside a tent on Sebring); yaw 270 faces along the grid direction (+x) on Sebring, 90 the opposite way, 0 across the track. Spawn data per track: grid slots (12), pit box, and a `flying_start` slot (Sebring: x -358, 60 mph) in `world/spawns.json`. Xenia's frames were taken during the race clock (Sebring x5 at 01:04, Test Track x2 at 00:40, Suzuka x12 at 00:53, Maple Valley x10 at 00:52), so the Xenia car had driven an unknown distance and lateral offset from any grid slot: the position is not in any file and has to be found by looking. A first automatic attempt (zero-mean correlation of 128x72 greyscale images with car / HUD / sky masked) preferred the reversed view (0.34) over the right direction (0.09 to 0.13), so it cannot find the spot. Not done: any like-for-like comparison. Useful for a future attempt: stand the car still at a known grid slot in Xenia (no rolling start) and shoot there, or pick positions by eye with `tp_shot.sh`.

User correction (2026-10-25): the Xenia shots were referenced against the engine's default spawn (rolling start, so the Xenia car had moved only a little), so they should be near like-for-like. That makes the "Sebring 0.40 is a framing difference" reading in update (8) and the "positions unknown" reading in update (9) too strong: the remaining differences are small position offsets and, more likely, CAMERA differences: the horizon sits at 48 % of the frame height in Xenia's Sebring shot (x5) against 41 % in ours, so Xenia's chase camera is lower / pitched differently; the car's width is similar (29 % against 33 %). The next like-for-like step is to compare our chase camera (height, distance, pitch, FOV) with the game's own camera data (`Media/Camera/*.xml` on the FM2 disc) rather than to move the car. Not done.

## Update 2026-10-25 (10): FM2's follow-camera data against the engine's (FH1) camera

FM2 `Media/Camera/CameraSettings.ini` (disc, read only), `FollowLowCam` / `FollowHighCam`: HeightOffsetGlobal -2 / +2, RadiusOffsetGlobal 0.05 / 0.15, TargetYOffsetGlobal -2 / +2, HeightMin/MaxAboveGround 1.78 / 2.00, near clip 0.3, far 5000, FOV 53 at standstill rising to 62 (100 mph?) and 64 with speed (`FOVSpeed0/1/2` 20 / 100 / 150), `FOVSpringK` 100, `FOVCompRatio` 0.95, yaw spring K 4.5 / D 4.4 / car damp 0.76 / max speed 2, height spring K 30 / D 11.85. The engine's follow camera (`camera/chase.rs`) holds FH1's values: Low: HeightOffset 0.9, RadiusOffset -0.15, TargetY 0.8, ground clearance 1.8, FOV 48.5 (High 58), FOVRatio 0.5, car damp 0.67, with base height / radius 0.35 m / 2.95 m (Low) from FH1's camera setup code; those two bases are FH1 code values, FM2's equivalents are not read (they sit in FM2's camera setup virtuals, not looked up).
Reading: the camera's rest height above ground is nearly the same (1.78 against 1.8) and the look target works out near 1.4 m either way (FM2: -2 offset plus the clamp delta of about 3.4; FH1: 0.8 plus 0.55), so pitch should be close; the differences are FOV (FM2 53 against 48.5 at rest, so FM2 shows about 9 % more of the world), radius (about 3.0 against 2.8 if the bases are equal) and the speed FOV behaviour. That fits the horizon sitting at about 44 % of the frame height in Xenia's Sebring shot against 41 % in ours, and Xenia's car looking a bit smaller. Not tested: an engine run with FM2's values; whether the ini FOV is vertical (assumed); FM2's base height / radius; which of the two follow cams the Xenia run used (the default `FollowCam` is assumed to be the Low one).
Next step if wanted: for FM2 tracks load FM2's `CameraSettings.ini` (copy it to the imported FM2 data in the importer) and use it for the follow camera, behind a switch; then redo the band comparison.

## Update 2026-10-25 (11): FM2's follow-camera settings in the engine (`FH1_FM2_CAMERA=1`, off by default)

Importer: `fh1setup import-fm2 <disc> --only camera` copies the disc's `Media/Camera/CameraSettings.ini` to `imported/fm2/camera/CameraSettings.ini` (done on this install). Engine: with `FH1_FM2_CAMERA=1` on an FM2 track, `Settings::parse_fm2` (fh1-formats) aliases FM2's keys (`FOV\FOV0` as `FOV`, `Spring\m_YRotSpringK` etc.) to the names the FH1-era follow camera reads, and main.rs replaces the camera settings with them (FH1's speed-FOV effect stays; base height / radius 0.35 / 2.95 m are still FH1's; gamedb camera overrides of the car still apply).
Twelve shots each, current default build (`base_*`) against `FH1_FM2_CAMERA=1` (`cam_*`): twelve-track sum |ln ratio| 15.96 against 16.02: the band brightness is unchanged within noise (Maple Valley 0.97 | 3.23 -> 1.00 | 3.10, Silverstone 0.92 -> 0.87, Nurburgring left 0.67 -> 0.60). The picture changes a little (`screenshots/cam_cmp_sebring.png`: base | cam | Xenia). Reading: the FOV and offsets matter much less than the DISTANCE: in Xenia's Sebring frame the car is clearly smaller (about 23 % of the frame width in the panel against 32 % in ours), i.e. Xenia's follow camera sits further back (and a little higher) than ours with either settings file; the missing piece is FM2's base radius and height (the camera class's setup code, not read: FH1's are 2.95 m / 0.35 m). Not done: decoding FM2's camera setup for those two numbers (the ini strings `RadiusOffsetGlobal` / `HeightOffsetGlobal` are the starting point for the cross-reference).

## Update 2026-10-25 (12): FM2's camera base values are the same as FH1's; the Xenia frames look like speed FOV 62

Game code (FM2 image): the two follow-camera setup functions at `0x821db150` (Low) and `0x821db530` (High) store the base height and radius into the camera object at +0x54 / +0x5c from constants `0x82001afc` / `0x82001af8` (Low: **0.35 m, 2.95 m**) and `0x82001b04` / `0x82001e70` (High: **0.60 m, 3.10 m**), then the ini's Height / Radius / TargetY offsets (tune variables `0x829c21dc..` Low, `0x829c21e8..` High) go to +0x58 / +0x60 / +0x64 and the car's own camera overrides replace them when non-zero (`+0x3034..`). These are exactly the numbers the engine's FH1 camera already uses, so nothing to change there.
So the car in Xenia's frames looks smaller because of speed, not the base distance: FM2's FOV widens from 53 at standstill to 62 (100 mph?) and 64, and the Xenia shots are rolling starts. New env `FH1_FM2_CAMERA_FOV=<deg>` (with `FH1_FM2_CAMERA=1`) forces the follow cameras' FOV. Sebring at FOV 62 (`screenshots/camf_cmp_sebring.png`: 53 | 58 / 62 | Xenia): car width about 25 % of the frame against Xenia's 27 %, horizon at about the same height. Good framing match.
Twelve tracks at FOV 62 (`cam62_*`) against the base build: the band sum goes 15.96 -> 17.47 (worse): Laguna left 3.06 -> 4.23, Tsukuba right 2.51 -> 3.73, Nurburgring 0.67 -> 0.45, Silverstone right 1.13 -> 0.92. The fixed screen bands move over different surfaces when the view widens, so the band metric is not a stable measure across camera changes: it should be used with the camera and position of the Xenia frame, not as a score to tune against. Not tested: driving the car at 60 mph for the frames (the real speed effect also changes the camera distance through `FOVCompRatio` 0.95, engine side uses FH1's dolly).

User correction (2026-10-25, later): the Xenia shots are NOT rolling starts (the car stands still, apart from the Tsukuba rolling-start shot x13). So the "speed FOV" explanation in update (12) is wrong: at standstill FM2's FOV is 53. What is still true: the base height / radius in the game code equal FH1's. Test of the other follow camera (`FH1_CAMERA_MODE=1`, FM2 settings, `screenshots/camhi_cmp.png`: Low | High | Xenia, Sebring and Test Track): the High camera makes the car too small (about 18 % of the frame width against Xenia's 23 to 27 %), the Low camera at FOV 53 too large (about 28 to 30 %); Xenia sits between them, and a Low camera at FOV 58 to 62 matches the car size and the horizon. So something in the game widens the standstill view by about 10 to 15 % compared with a plain vertical FOV of 53: the ini has `FollowCam\FieldOfView\m_bFOVComp On` (an aspect compensation switch); the code that applies it (and whether `FOV0` is a 4:3 value that is widened for 16:9) is the next thing to read. Not done.

## Update 2026-10-25 (13): FM2's FOV compensation code (read), and what it says about the standstill view

FM2 code: the camera's FOV tune variables sit in globals at `0x82984f60` (`m_bFOVComp`), `0x82984f7c` (FOV0), `0x82984f98` (`FOVCompRatio`), `0x82984f94` (`FOVSpringK`); the follow-camera update at `0x821df248` (Low) / `0x821df310` (High) calls `0x821dee10` (speed-to-FOV table with spring) and then `0x821d7be0` to get the camera distance:
`dist = L + base_radius + radius_offset` (L = a z-difference of the car box read through a virtual call, `0x68 - 0x58` of its 32-byte result) and, with `m_bFOVComp` on, `dist' = dist * (1 + FOVCompRatio * (tan(FOV0/2) / tan(FOV/2) - 1))`, i.e. the camera dollies in as the FOV rises above FOV0 so the car keeps its size. At standstill FOV = FOV0 and the factor is exactly 1: **the compensation does not change the standstill view**, so it is not the reason for Xenia's smaller car. (The engine's dolly formula for FH1 is the same shape.)
One open reading: `L` in the FM2 code is a difference of two z values (0x68 - 0x58), which could be the car's whole box length or `rear - centre of gravity` as in the engine (`rear = bb_max.z - cg.z`). Test: with the whole box length (4.4 m) as the radius term (temporary env, `screenshots/camlen_cmp.png`) the car shrinks to about 13 % of the frame width against Xenia's 23 to 27 %: too far. So the engine's rear-from-cg reading is closer, and Xenia's distance lies between (about 1.2x ours). Experiment code removed.
Where that leaves the framing: base height / radius equal, FOV compensation neutral at standstill, High camera too far, Low camera at FOV 53 too near; a Low camera with a vertical FOV near 58 to 62 fits. What differs is not found in the camera constants. Remaining candidates: the FM2 car's bounding box as the game sees it (the imported car's `bbox` in the engine may differ from the console's), the Xenia frame's own aspect handling (field of view applied as horizontal for 4:3 content?), or `FOV0` meaning something else. Not tested.

## Update 2026-10-25 (14): the imported Skyline's size and the camera: FM2's camera settings already match Xenia's framing

Car data (imported FM2 Skyline `physics.json` / `model.gltf`): bounding box z -2.299 .. +2.304 (4.6 m), x +-0.95, wheelbase 2.665 m, hubs at z +-1.332, mesh and PristineBoundingBox agree (mesh-space bounds -2.299 .. 2.304), centre of gravity about z 0, so the engine's `rear = bb_max.z - cg.z` is 2.30 m. The car's own camera overrides (gamedb `CameraOverrides`, FM2's): `CamFollowLowHeightOffset 0.1375`, `RadiusOffset -0.15`, `TargetYOffset -0.0025`, High all 0; a non-zero car value REPLACES the global one, so the Low camera's global offsets (-2 / 0.05 / -2) never apply to this car. Low radius = 2.30 + 2.95 - 0.15 = 5.10 m (the same as with FH1's settings, since FH1's own -0.15 is the same number).
Measured on one shot pair (`screenshots/cam_vs_x5.png`, Sebring, ours with `FH1_FM2_CAMERA=1` at FOV 53 above Xenia's x5 below, both scaled to 960 wide): car body width 274 px against 278 px, rear wing 252 against 244, horizon at the same height. **The FM2-camera framing already matches Xenia's at standstill**; my earlier reading in updates (11) to (13) ("Xenia's car is smaller", FOV 58 to 62, radius) came from comparing the panels by eye against the FH1 camera and is wrong. FOV 62 and the whole-box-length radius are not needed. `FH1_FM2_CAMERA=1` is the like-for-like setting; the engine default (FH1's camera, FOV 48.5) shows the car about 10 % larger.
What the matched frame shows on Sebring: the left foreground in ours has a white concrete slab and a dark diagonal asphalt stripe; Xenia's left foreground is one grey surface without the stripe. So the 0.40 ratio is a real surface difference (the blend of colour textures 402 / 403 by mask 479, shader `f4222c96033cdd0e`, is drawn too contrasty / wrong mask placement), no longer a framing problem. Next lead: the Sebring blend mask and its UV / layer order.

## Update 2026-10-25 (15): FM2 camera is now on by default; Sebring's road shaders measured, no match yet

`FH1_FM2_CAMERA` is on by default for FM2 tracks (`=0` gives FH1's camera; `FH1_FM2_CAMERA_FOV` stays opt-in). Needs `fh1setup import-fm2 <disc> --only camera` once (done on this install); without the ini the engine logs a warning and keeps FH1's camera.
Sebring: textures 402 / 403 / mask 479 (the blend `f4222c96033cdd0e` I had noted "under the spawn point") are the dirt / grass terrain blend (mask 479 is a track-wide terrain map), not the road. The draws near the grid spot (engine point -231.5, 211.3; `FM2_NEAR=-231.5,211.3,6`, note z positive) are: model 49 hash `ded4e49d405ba241` (3 draws, 60 to 70 triangles, a 80 m strip along z 206, lightmap tile 152, textures 153 BC1 + 154), model 49 / 50 hash `f9fbef3ac3660c4c`, models 85 / 95 hash `396097bc9fb8d028` (colour texture 214 + tile 159 / 165, 70 to 114 triangles, z 208 to 220), models 153 / 158 hash `f1fe1fd4779c1604` (the 195-dword asphalt-like family: tf3 / tf6 / tf7 BC4 grains 397 / 410 / 163, tf4 texture 161 and tf5 texture 162 BC1 colours, tile 221 / 219; z 204 to 235, x -305 to -164), none of them in the gamma table. Interpreter lit values (max over the lightmap grid, G, linear, `data/work/seb_lit.py`) against Xenia's left / right band 0.465 / 0.488: model 49 group 0 `ded4e49d`: no gamma 0.912, tf3 (texture 153) sRGB 0.563, tf4 no change; models 153 / 158 `f1fe1fd4`: no gamma 0.653 (158: 0.694), tf5 (texture 162) sRGB 0.234, all colours 0.25, BC4 sRGB 0.70; model 50 group 0: none 1.255, all 0.574. The first family's 0.563 is the closest (1.2x), the main slab family brackets Xenia's value (0.653 and 0.234) without matching it, and which draw covers the left foreground is not established (the painting test, `bitpaint.py`, would tell). Not added to `FAMILY_GAMMA`. Also still true: the engine's colour-texture road path has no x2 on the light term (shader: albedo x 2 x lerp(dark, light, lightmap)), so Sebring-like colour roads come out about half as bright in the lit parts; New York, Road Atlanta and Laguna scale the other way, so a global x2 is not safe.

## Update 2026-10-25 (16): Sebring's left foreground: painted by texture 161 (family `f1fe1fd4`); the engine is 2.5x darker than the interpreter says

Painting test (`bitpaint.py sebring_00 100 625 190 685`, the dark diagonal strip in the left foreground; relaxed red threshold on the saved shots because the patch is dark): bits 1, 2, 5, 6 red, so texture index 102 = file `0xa1` = texture 161 (BC1 colour 512x512), the `tf4` colour of the 195-dword asphalt family `f1fe1fd4779c1604` (models 153 / 158, material shader `h_blnd2_ao_lm`). Engine pixel of that strip: (118, 120, 116) = linear G 0.185. Interpreter (`seb_lit.py`, lit max over the lightmap grid): no gamma 0.653, texture 162 (`tf5`) sRGB 0.234; Xenia's left band 0.465. With the typical Sebring lightmap at about 0.7 (lerp(dark 0.02, light 1.17, 0.7) = 0.83 of the max 1.17, factor 0.71) the no-gamma value becomes 0.46 = Xenia, whereas sRGB on `tf5` gives 0.17 = what the engine draws. So for this family the console does NOT decode the colour textures as sRGB, and the engine's value is 2.5x too low. (The materials' texture `word` sign byte for FM2 textures is 1, not 0x3F, so `remaster.rs` already uploads them linear: the factor is not a colour-space decode but the missing x2 (shader: `albedo * 2 * lerp(...)`) and a further 1.25.) Table: `FAMILY_GAMMA` gained `f1fe1fd4779c1604 -> 0` (no sRGB sampler); it only gates the baked-road albedo and the opt-in colour scaling, so the engine's picture is unchanged by it (these draws have colour textures, so no scaling: "0 colour draws scaled").
Test of the missing x2 (`FH1_RM_LM_LIGHT_EXPOSURE=2`, five colour-texture tracks, left | right ours / Xenia, base -> x2): Sebring 0.40 | 1.06 -> 0.80 | 2.05, Sunset 0.76 | 1.54 -> 1.54 | 3.15, Road Atlanta 1.56 | 1.36 -> 2.02 | 2.77, New York 0.76 | 0.80 -> 1.67 | 1.84, Laguna 3.06 | 2.48 -> 6.21 | 4.91. Only Sebring's left band improves; a global x2 is wrong. What the colour-texture families need is the importer's per-draw shader result (as the baked roads get), not a constant; the `FM2_FAMILY_COLOUR` scaling only covers draws with exactly one colour texture, so the mixed-layer families (Sebring's `f1fe1fd4`, Laguna's blends) stay unfixed. Next lead if continued: extend the colour scaling to draws with two colour layers and BC4 masks (scale each colour by shader brightness / its own mean).

## Update 2026-10-25 (17): two-colour-layer brightness scaling tried: not useful; the Sebring strip's darkness is in its lightmap tile

Correction: the table entry committed in update (16) had a mistyped hash (`f1fe1fd4_7791_c604`); the real signature is `f1fe1fd4779c1604` (fixed now; it only gates the baked-road albedo and the opt-in colour scaling, so no picture changed).
Experiment (code removed again): `FM2_FAMILY_COLOUR=2` scaled up to two colour layers of a family draw to the shader's brightness. On Sebring's `f1fe1fd4779c1604` draws (42 of them) the shader brightness is 0.79 / 0.78 / 0.73 while the two colour layers are textures 161 (mean linear 0.97, nearly white) and 162 (0.16, dark): scaling each to the draw's overall level (x0.82 and x4.8) makes both layers the same brightness and destroys the blend's pattern (the BC4 mask then selects between two equal levels), so a per-layer scaling to the combined shader result is the wrong operation for blends. Not tested in the engine (the data would be meaningless); Sebring restored to the default import.
What the dark strip really is: texture 161 is a white concrete layer (0.97); the strip is dark because of the lightmap of its draw, not its albedo. The draws near the grid spot with textures 161 / 162 / 163 and a dark tile belong to model 49 / 85 hash `f9fbef3ac3660c4c` / `396097bc9fb8d028` with tiles 159 / 165 (`0x9f`, `0xa5`: values 0 to 125 of 255, mean about 0.3), while the `f1fe1fd4` draws (tile 221 / 219, values about 195 = 0.76) are lit. So Xenia's uniform grey surface there against ours dark strip + white slab points at the lightmap of the strip draws (shadow baked into tiles 159 / 165 that Xenia does not show at this spot, or a tile / uv orientation error as suspected for Mugello): the tile-to-surface mapping of those draws is the next thing to check (compare the tile's dark region with the strip's world position, mirrored u / v, crop offsets). Not done.

## Update 2026-10-25 (18): Sebring strip and slab: the tile mapping is fine; the slab is drawn too white

Tile crop check (temporary page dump, removed): tile 152 (`0x98`) is cropped exactly (page 796, 512x512, tile 32x32, su -0.0625 mirrored, sv 0.0625, offsets 0.484 / 0.391, first row at page y 200); the page rows above the tile repeat the tile's first row (edge padding), so no half-texel or gutter error. The strip draws (hash `ded4e49d405ba241`, model 49, 3 draws) use lightmap uv u 0.203 to 0.277 and v 0.000 to 0.004: columns 6 to 9 of the first row, values 93 to 94 (0.36), against 147 to 153 (0.58) in the middle of the same row, so the bake put a shadowed texel column there; the game samples the same texels, so the strip is dark in the game's data too. (Xenia's strip is only subtly darker than its surroundings; not compared texel by texel.)
The white slab beside it is model 49 draw 3, hash `f9fbef3ac3660c4c` (195-dword shader group 1, tile 159: textures `tf3` 160 BC4 1024x512 mask, `tf4` 161 white concrete 0.97, `tf5` 162 dark 0.16, `tf6` 163 BC4 grain): tile 159 at the draw's uv (u 0.71 to 0.78, v 0.21) reads about 135 (0.53), lerp(dark 0.02, light 1.17) = 0.63. Interpreter, lit max over the grid: no gamma 0.356, texture 162 sRGB 0.127; at the real lightmap level (0.54 of the max) the shader gives about 0.19 (about 120 in bytes) for this slab, while the engine draws it as near-white concrete (about 200 in bytes, 0.6 linear). So the engine shows only the white layer 161 where the shader blends 161 and 162 (and the grain) through the masks: the layer blend of this family (`h_blnd2_ao_lm`: base 162, layer 161, mask 160 / grain 163; material 3369 lists textures [162, 161, 3276, ..., 3275]) is not reproduced; the slab is about 3x too bright and the strip about right, which is what makes the Sebring left band a mixture. Next lead: how the remaster material for this family combines roles A / B / mask (`remaster.rs` roles for `h_blnd2_ao_lm`) against the shader's own blend (mask = BC4 `tf3`). Not tested.

## Update 2026-10-25 (19): the Sebring slab: engine and importer-shader agree; my "3x too bright" was a measuring inconsistency

The slab draws (model 49 draw 3 and similar, hash `f9fbef3ac3660c4c`) are NOT the `h_blnd2_ao_lm` blend: the engine's material for them is `h_diff_1_lm` with one colour texture (161, white concrete, mean 0.97) and the lightmap tile (159 / 165 / 168); the game's pixel shader additionally fetches 160 (BC4 1024x512), 162 (dark BC1, flags 20) and 163 (BC4 grain) which the engine's material ignores (roles `h_diff_1_lm`: colour + lightmap only). `FAMILY_GAMMA` now also lists `f9fbef3ac3660c4c -> 0` and `396097bc9fb8d028 -> 0` (models 85 / 95, colour 214 + lightmap). With `FM2_FAMILY_COLOUR=1` the importer scaled 159 colour draws to the shader's brightness with neutral light (the slab: shader [0.83, 0.81, 0.78] against texture mean 0.97, scale x0.86; the 396097bc colour 214: x2.3): the Sebring frame hardly changes (left band 116 / 119 / 115 against the same, right band 182 to 187; `screenshots/seb_fc_vs_x5.png`). Reading: with neutral light (lerp = 1) the shader's slab colour is 0.83, times the lightmap lerp 0.63 gives 0.52 (190 in bytes), and the engine draws about 0.6 (200): they agree. The "about 3x too bright (0.19)" figure of update (18) came from the `seb_lit.py` lit-max run (0.356 at lerp 1.17); that and the importer's neutral evaluation disagree by 2.3x for the same shader (different constants: `seb_lit.py` does not use the draw's own palette constants the way `shader_eval` does), so that conclusion is withdrawn; which of the two is right is not settled (Xenia's slab area is about 150 to 180 in bytes = 0.3 to 0.46, between the two). Sebring re-imported to the default (no colour scaling).

## Update 2026-10-25 (20): the two interpreter setups compared: they agree; `seb_lit.py`'s "lit max" was not a lit max

Same draw (Sebring model 49 group 1 draw 3, `f9fbef3ac3660c4c`, draw constants c1 2.0, c3 0.2, c4 1.6, c6 0.8), `fm2_ps_run` (the tool under `seb_lit.py` / `lit_model.py`): neutral lerp (c184 = c185 = 1) gives G 0.794 (RGB 0.807 / 0.794 / 0.753); the importer's `shader_eval` ("neutral light") gave [0.833, 0.813, 0.777] for the same family (model 49), the same within the vertex sample. With the track's real lightmap colours (dark 0.0175, light 1.17) the output is G 0.372, whatever uv I put in register r4: the tool overwrites each sampler's interpolator register with the draw's own mesh uvs (`resolved`), so the lightmap uv sweep in `seb_lit.py` / `lit_per.py` / `lit_model.py` never moved anything for draws whose mesh supplies the lightmap uv: the "lit max" is simply the value at the draw's own lightmap uvs (the first-run numbers of updates (16) to (18), e.g. 0.653 for `f1fe1fd4`, 0.563 for `ded4e49d`, 0.356 for this slab, are real-lightmap values, not maxima, so they must not be compared with Xenia as "lit"). Nothing was wrong with either setup; the gap I reported as "2.3x" is the lightmap factor of this draw (lerp 0.47 at its real lightmap texel, against 1 in the neutral evaluation).
Slab, engine against shader at the same spot: shader 0.794 (neutral) x lerp 0.47 = 0.37 (164 in bytes); the engine draws the white texture 161 (0.97, no x2, and without the dirt layers) times its own lerp (about 0.62): 0.6 (200). So the engine is about 1.6x brighter on this slab: about 2.4x from the missing dirt layers and the missing x2 combined (effective shader albedo 0.4 against texture 0.97, but the shader doubles), and the engine's lightmap lerp is about 1.3x higher than the interpreter's (0.62 against 0.47; the engine's tile sample at the draw's uv reads about 0.53 where the interpreter's implies about 0.39: not explained: the interpreter samples the tile through the draw's `c170` atlas transform, the importer's cropped tile has none). Xenia's left band (0.465) is between. Not resolved; `FM2_FAMILY_COLOUR=1` scales the albedo to the neutral value 0.83 (engine 0.5 afterwards) and Sebring's left band does not move because the picture there is dominated by the strip draws.

## Update 2026-10-26: lightmap tile crop: negative scale read as a clamp flag, not a mirror (`FM2_LM_MIRROR=1` = old) — result mixed, not committed

Finding (game code + data): the road pixel shaders read the lightmap tile transform as `cndgt r0.xy, -c170.x, sat(uv), uv; mad r0.xy, r0.xy, |c170|.xy, |c170|.zw`, i.e. scale and offset are taken as absolute values and the sign of `c170.x` only chooses clamped or raw uvs. The importer cropped tiles with a negative `su` as mirrored (page = offset - |su| t). Page dump of Sebring tile 152 (page 796, 512x512, su -0.0625, ou 0.484): the page is laid out in 48-texel cells (32-texel tile plus an 8-texel gutter each side) with a boundary at x 240 and 288; the absolute reading [248, 280) sits centred in the cell, the mirrored reading [216, 248) straddles the boundary at 240 (half of it is the neighbouring cell). Change in `convert_lightmaps`: `s.abs()` (default), `FM2_LM_MIRROR=1` restores the old crop. All FM2 tracks re-imported, remaster tables rebuilt.
Result, same camera (`cam_*` against `lmfix_*`, ours / Xenia left | right): unchanged on Tsukuba, Silverstone, Road Atlanta, Nurburgring, New York, Mugello, Maple Valley, Laguna Seca (their tiles near the spawn are uniform or unaffected); Test Track 0.96 -> 0.77 (left), Sunset 0.76 | 1.57 -> 0.69 | 1.43, Sebring 0.40 -> 1.90 (the dark strip is now near-white, 241 in bytes against Xenia's 180), Suzuka 3.12 | 9.00 -> 3.70 | 10.75. Sum 16.02 -> 16.20: slightly worse. Picture: `screenshots/lmfix_cmp.png` (base | lmfix | Xenia for Sebring, Test Track, Suzuka). So the page-layout argument says the new crop is the right region for tile 152, but the brightness bands do not confirm it on the tracks it changes. Not decided; the deciding test would be whether baked shadows line up with the objects that cast them (a place with a clear cast shadow in the lightmap), which the band metric cannot show.

Decision (2026-10-26, same day): keep the absolute crop as the default. Objective check with the game's own data, no Xenia needed (`FM2_LM_SEAMS=1`, importer diagnostic): where draws with different lightmap tiles share a vertex position, the bilinear lightmap value each reads there is compared (a right crop should give the same light on both sides of a seam). Mean jump, mirrored -> absolute crop: Mugello 0.242 -> 0.218, Sebring 0.251 -> 0.217, Suzuka 0.308 -> 0.263, Test Track 0.407 -> 0.379, Tsukuba 0.230 -> 0.199; Sunset 0.158 -> 0.177 (the one track that gets worse). Variants inside the tile on Tsukuba (`FM2_LM_FLIPU` / `FM2_LM_FLIPV`, diagnostics): 0.199 (none), 0.205 (u), 0.208 (v), 0.194 (both), mirrored + v 0.233: none of them is clearly better than the plain absolute crop. Caveat: in every variant about 40 to 55 % of shared vertices still jump by more than 0.2 (corners where a wall meets the ground legitimately differ; the metric does not separate those), so the margin is modest. Together with the shader code (`|c170|`) and the page layout (tile centred in its 48-texel cell), the absolute crop is the better-supported reading; it makes the band comparison slightly worse (16.02 -> 16.20, mostly Sebring's strip now too bright and Suzuka), which points at other errors on those surfaces. Installed data: all FM2 tracks with the absolute crop, remaster tables rebuilt. `FM2_LM_MIRROR=1` gives the old crop back. Not tested: a seam metric restricted to coplanar road surfaces (would sharpen the test).

## Update 2026-10-26 (2): seam test on flat ground only: the absolute crop wins on all six tracks

`FM2_LM_SEAMS=1` now also prints a "flat ground only" figure: only vertices whose area-weighted normal is within about 8 degrees of vertical, and still at least two tiles among them (ground seams, where both sides must carry the same light; walls meeting the ground are excluded). Mean lightmap jump, mirrored -> absolute crop: Mugello 0.333 -> 0.187, Suzuka 0.387 -> 0.279, Sebring 0.323 -> 0.251, Sunset 0.231 -> 0.201 (Sunset now also favours the absolute crop; its all-vertices figure had gone the other way), Test Track 0.507 -> 0.462, Tsukuba 0.303 -> 0.290. Share of jumps over 0.2: Mugello 65 -> 35 %, Sebring 57 -> 36 %, Suzuka 70 -> 46 %. **The absolute crop is confirmed as the better reading on every track tested.** Flip variants inside the tile (absolute crop, flat ground): u flipped Mugello 0.217 / Test Track 0.525 / Tsukuba 0.292; v flipped 0.215 / 0.461 / 0.262; both 0.213 / 0.545 / 0.281; plain 0.187 / 0.462 / 0.290: no flip is better overall, so the plain absolute crop stays.
Still open: even with the right crop the flat seams jump by 0.19 (Mugello) to 0.46 (Test Track) on average, so something else in the tile lookup is off as well (candidates: the lightmap uv decode (`uv_unorm`) or a half-texel offset, the gutter texels the uvs may reach into, or genuinely different bakes across tiles). Installed data: all FM2 tracks with the absolute crop.

## Update 2026-10-26 (3): the lightmap uv comes from the draw's vertex shader: lightmap seams nearly gone, baked shadows now where Xenia has them

Cause of the remaining seam jumps: the importer guessed each draw's lightmap uv (`lightmap_offset` + `uv_unorm`, unsigned / 65535). The game's pixel shader clamps an interpolator (`max_sat rA, rI.xxxy, rI.xxxy`) and runs it through the tile transform (`cndgt` / `cndge` on `c168` / `c170`); the vertex shader fills that interpolator with its own fetch format and scale (e.g. 16_16 signed integer x 1/32768). Change: `fm2_uv.rs` finds that interpolator (the `max_sat` source in shaders that have the `cnd*` on a `c16x` / `c17x` constant: every road family seen) and resolves the tile item's uv through the vertex shader like the other samplers; `fm2_scenery.rs` uses it for `lm_uv` (fallback: the old guess; `FM2_LM_UV_OLD=1` keeps the old guess). Resolved on 5631 of 5631 lightmap draws on Mugello and 7460 of 7461 on Test Track.
Flat-ground seam jumps (`FM2_LM_SEAMS=1`, mean, share over 0.2): Test Track 0.507 (old crop, old uv) -> 0.462 (absolute crop) -> **0.101, 17 %** (absolute crop + vertex-shader uv); Mugello 0.333 -> 0.187 -> **0.091, 13 %**. With the vertex-shader uv the mirrored crop is clearly wrong (0.420 / 0.349).
Pictures (`screenshots/lmuv_cmp.png`: previous | now | Xenia for Sunset, Suzuka, Maple Valley, Test Track): Suzuka now has the large dark baked shadow left of and beside the car that Xenia shows; Maple Valley the bridge's dark shadow over the left of the road as in Xenia; Sunset's road is brighter than Xenia's now; Test Track about the same. Band comparison (same camera): sum 16.02 (`cam`) -> 17.86 (`lmuv`): Suzuka left 3.12 -> 5.69, Sunset 0.76 -> 1.52, Maple Valley left 1.00 -> 0.51, Test Track 0.96 -> 1.22, Road Atlanta 1.58 -> 1.44. The bands move because the light under each surface is now the right light: the brightness rules tuned so far (family gamma table, baked-road albedo, shadow rule) were all checked against the old, wrong lightmap values and need re-checking. Installed data: all FM2 tracks re-imported with both fixes, remaster tables rebuilt.

## Update 2026-10-26 (4): brightness rules re-checked against the corrected lightmaps: Suzuka joins the BC4 rule; twelve-track sum 14.22 (best so far)

Suzuka's spawn road (model 867, hash `52ff81cbba901906`: tile 840, BC4 grains 841 and 116, no colour texture), interpreter averaged over the draw with the track's lightmap colours and the corrected lightmap uv: no gamma 0.227 (the engine drew 0.22), BC4 grains sRGB 0.045, Xenia 0.040. So with the right lightmap Suzuka fits the BC4 rule that earlier (wrong lightmap) over-darkened it. Added to `FAMILY_GAMMA` (BC4). Suzuka left | right: 5.69 | 10.75 -> 0.66 | 0.94.
BC4 rule on every baked road (`FM2_BC4_SRGB_ALL=1`, full re-import, twelve shots `bc4all_*`): sum 15.42 against 14.22 for the table: Test Track left 1.22 -> 0.45, Mugello 0.98 -> 1.95, Maple Valley right 2.30 -> 4.59 (worse), Test Track right 9.09 -> 3.08, Maple Valley left 0.51 -> 0.66 (better). The per-family table stays; the default import is back.
Current twelve-track state (table + both lightmap fixes, FM2 camera, game shadow rule), ours / Xenia left | right: Tsukuba 1.03 | 2.89, Test Track 1.22 | 9.09, Sunset 1.52 | 3.15, Silverstone 0.89 | 1.09, Sebring 1.90 | 1.12, Road Atlanta 1.44 | 1.28, Nurburgring 0.65 | 0.11, New York 0.76 | 0.84, Mugello 0.98 | 1.63, Maple Valley 0.51 | 2.30, Laguna Seca 3.18 | 2.48, Suzuka 0.66 | 0.94. Sum of |ln ratio| **14.22** (start of this work 17.68; before the lightmap fixes 16.02).
Not re-checked yet: Sunset (its road shader samples four textures but the draw binds two; the interpreter returns black, so the bound pairs need fixing first), Sebring (now too bright, 1.90), Laguna Seca (3.2x), the shadow rule (needs a lit/shadow pair in Xenia).

## Update 2026-10-26 (5): Sunset's road: found by painting; no gamma choice fits on the draw average, not added

Binding: `fm2_ps_run` binds a draw's items to samplers by index, the importer (`fm2_uv`) by role (shadow / tile / uv-fed). For Sunset model 77 group 2 draw 0 (hash `22b80bd792e72d6e`) the shader samples tf2..tf5 but the draw binds only two items (tile 256, texture 58); tf4 / tf5 are not bound at all and read black, hence the 0 output. New probe option `PS_WHITE=4,5` (those samplers read white) gives 0.192, but texture 58 is a concrete WALL texture (`screenshots/sunset_tex58.png`): model 77 is a wall, not the road; the earlier note that called it Sunset's road was wrong.
Painting test on Sunset's left road patch (`bitpaint.py sunsetinfield_00 120 600 300 670`): texture `0x105` (261, BC4 1024x512, a grain shared by several draws). Draws with it near the spawn: model 79 (hashes `534d74061a777a9a` with colours 249 / 252 and `6e2458fee8acdb77` with 253 / 254, 204-dword shader, tile 260) and model 2084 (`babfbc606e6174ea`, 32x256 kerb / paint textures: yellow-white, not the road). Interpreter on model 79 group 2 draw 3 (`534d74...`, the asphalt), averaged over the draw with the track's lightmap colours and the corrected lightmap uv: no gamma 0.329, `tf5` (texture 252, BC1) sRGB 0.056, `tf4` (261, BC4) sRGB 0.340, all colours sRGB 0.061. Xenia's left / right bands 0.157 / 0.076, the engine draws 0.24. Neither choice fits (2.1x above, 2.8x below); the draw average covers a 28-triangle strip whose lightmap varies, so the patch value is not pinned down. Not added to the table. Next if continued: evaluate the shader at the band's own vertices (needs the patch's world position) instead of the whole-draw average.

## Update 2026-10-26 (6): Laguna Seca with the corrected lightmaps: still 3x too bright; a static shadow map is the lead

Interpreter on the spawn road (model 81, draw average, track lightmap colours, corrected lightmap uv): plain draw (texture 6) no gamma 0.695, colour sRGB 0.381; blend draws (6 / 132 by mask 176) no gamma 0.649, both colours sRGB 0.322 (141 instead of 132: 0.336). Xenia 0.055 / 0.122, the engine draws 0.18. Every choice is far above Xenia, so no gamma rule explains Laguna; the game's own lightmap at these vertices is lit (lerp about 0.8).
Pictures (`screenshots/lag_vs_x11.png`): in Xenia the whole foreground up to the overhead banner is in shadow (the car's own shadow is not even visible), the road beyond the banner is lit; ours is lit everywhere apart from the car's shadow. With the track's own light direction (`FH1_RM_FM2_SHADOW_DIR=full`, Laguna `ShadowLightDirection` (1, -0.85, 0.9), about 32 degrees high, sun ahead) the banner's real-time shadow lands well ahead of the car, not on the foreground; bands 3.18 / 2.48 -> 4.82 / 3.72 (worse). So the dark foreground is neither the lightmap nor the banner's real-time shadow.
Lead: the road pixel shaders read two shadow samplers before the lightmap (`(p0) tfetch2D ... tf0` gated by the `c108` / `c109` shadow fade, and `tfetch2D r0.w, r1, tf1` combined with it by `cndgt`): tf0 is the real-time shadow map; tf1 is fed by its own interpolator and looks like a STATIC shadow texture of the track (TrackSettings `StaticShadowColor` 0x600 goes to `c103`). Neither is bound by the draw (system textures), and the engine has no equivalent: large static shadows (Laguna's foreground, possibly Suzuka's and Maple Valley's dark right-hand areas) would come from it. Next: find where the game loads that texture (a per-track file or a `.stx` entry), what `tf1`'s uv is (the interpolator's vertex-shader source), and how the shader combines it with `c103`. Not done.

## Update 2026-10-26 (7): the road shaders' tf0 / tf1 are the two real-time shadow cascades, not a static shadow map (lead of update 6 withdrawn)

Vertex shader of the Mugello road (model 3290 group 0; the same layout on the other road families): `o0 = c15..c18 x world position` and `o1 = c19..c22 x world position` (two 4x4 projections of the world position), `o2` the world position, `o5` / `o6` / `o7` the view, lightmap / tiling uvs and fog. The pixel shader fetches `tf0` at `r0.xy` only when the near-cascade test passes (`(p0)` after `mad_sat ... c109 / c108`, the shadow fade by depth) and `tf1` at `r1.xy` otherwise, and picks one with `cndgt`: so `tf0` = near shadow cascade, `tf1` = far shadow cascade, both the game's real-time sun shadow map (the matrices are the `c_shadowTexMtxNear` / `c_shadowTexMtxFar` pair the car effects name), bound by the renderer. `c103` (`StaticShadowColor`) is not read by these shaders at all (their constant list: c0..c7, c101, c102, c104, c105, c108, c109, c114, c168 / c170, c184, c185). So there is no separate static shadow texture in the road shading; the shadow term is `min(1 - (1 - c105) * S, lerp(dark, light, lightmap))` with S from the cascades, which the engine already does (`FH1_FM2_SHADOW_GAME`).
For Laguna that means the dark foreground in Xenia is a REAL-TIME shadow cast by geometry with the game's own sun (track `ShadowLightDirection` (1, -0.85, 0.9), override on). With that direction in the engine (`FH1_RM_FM2_SHADOW_DIR=full`) the banner's shadow lands ahead of the car, not on it; with the default (Horizon time-of-day azimuth) there is no shadow there either. Open: which caster shades Xenia's foreground (the bridge deck, the stand behind the camera) and whether the engine's z flip of `ShadowLightDirection` is right for this track (the `fz` variant was tested before the lightmap fixes only). Not done.

## Update 2026-10-26 (8): shadow-direction variants re-tested with the fixed lightmaps: the default stays

`FH1_RM_FM2_SHADOW_DIR` variants (full / fz / fx / fxz) against the current default on Laguna, Test Track, Suzuka and Maple Valley (left | right ours / Xenia; sum over the four tracks): default 6.45 (Laguna 3.18 | 2.48, Test Track 1.22 | 9.09, Suzuka 0.66 | 0.94, Maple Valley 0.51 | 2.30), full 10.41, fz 8.46, fx 8.70, fxz 8.31. Placement by eye (`screenshots/sd2_<track>.png`: default, full, fz / fx, fxz, Xenia): the default puts the car's shadow where Xenia has it on Test Track (right, behind) and Suzuka (left, behind); full and fz darken the car's own rear (sun behind the camera), fx / fxz move the shadow to the other side on Suzuka. None of them reproduces Laguna's dark foreground. Result: the default sun direction is the right one for these tracks; Laguna's foreground shadow stays unexplained (its caster and the game's sun there are not identified).

## Update 2026-10-26 (9): shadow rule and drop shadow re-tested with the fixed lightmaps: both stay on

Twelve tracks, fresh baseline (`cur_*`, current defaults) against `FH1_FM2_SHADOW_GAME=0` (`sgoff_*`) and `FH1_FM2_DROPSHADOW=0` (`dsoff_*`): sums 14.33 / 14.35 / 14.49, the same within the shot-to-shot noise. Per track the shadow rule helps Laguna (left 4.02 -> 3.12) and Suzuka (0.36 -> 0.47), and costs New York (0.89 / 1.00 -> 0.76 / 0.84) and Tsukuba's right band (1.97 -> 2.51); the drop shadow only touches Tsukuba (2.81 -> 2.51) and Suzuka marginally. Both are game-derived and neutral on the total, so the defaults stay. The large right-band errors (Test Track 9.1, Sunset 3.2, Tsukuba 2.5, Maple Valley 2.3) do not respond to either: they are not the real-time car shadow. Note: Suzuka's left band reads 0.47 here against 0.66 in the `lmtab` run with the same data (sun / exposure variation between runs); differences below about 0.2 in a ratio are noise.

## Update 2026-10-26 (10): patch-level shader check; FM2 colour textures as sRGB tried globally (mixed)

New tools: engine `FH1_SHOT_PROBE=1` (with FH1_SHOT) logs the ground points under a 3x3 grid in each comparison band at the screenshot time ("shot probe left / right: centre x,y,z"); importer `FM2_PROBE_AT=x,y,z[;...]` (with `FM2_PROBE_LERP=dark_g,light_g`, the map's lightmap colours, since the staging folder has no fx_globals.json yet) finds the triangle of every draw under the point, builds a virtual vertex there (all uv sets and the lightmap uv interpolated), runs the draw's pixel shader at that spot with neutral light for several gamma choices (family / none / BC4 / colours / all) and multiplies by the lightmap light at the spot. `shader_eval` now runs through `shader_eval_verts` (per vertex, optional gamma mask).
Results at the band centres (lit G; Xenia in brackets): Sebring left (slab `f9fbef3a`, lightmap 0.78): none 0.84, colours sRGB 0.37 (0.465; the engine draws 0.89 = "none"); Sebring right 0.77 / 0.30 (0.488); Sunset left (asphalt `534d74061a777a9a`, lightmap 0.66, lerp -0.19..1.19): none 0.42, colours sRGB 0.085 (0.157); Sunset right 0.26 / ~0 (0.076). The engine draws FM2 colour textures linear (their format word's sign byte is 1), i.e. the "none" column.
Global test `FM2_RM_COLOUR_SRGB=1` (remaster tables: every FM2 colour layer A / B / C uploaded as sRGB; no re-import): sum 14.33 -> 15.99. Laguna Seca 3.12 | 2.48 -> **1.39 | 1.09**, Sunset 1.52 | 3.15 -> 0.38 | 0.79, Road Atlanta left 1.42 -> 1.17 (right 1.23 -> 0.40), Sebring right 1.11 -> 0.62 (left unchanged: its slab colour is not on a colour role), New York 0.76 | 0.84 -> **0.17 | 0.21** (much worse), others unchanged (their roads are baked or BC4-only). So colour sRGB is right for some families (Laguna's road clearly) and wrong for others (New York): it has to be per family, as the gamma table already is for the baked roads. The switch stays off; tables restored to the default. Next: carry the family table's colour bits to the engine (a per-material sRGB flag from the importer) and fill it family by family with the patch check.

## Update 2026-10-26 (11): per-family colour sRGB carried to the engine: twelve-track sum 12.37 (best so far)

Patch probe (Xenia in brackets; lit G at the band centres): Laguna spawn road `8b9b73e9195be7b9` (palette road: colour 190, BC4 191 / 193, tile 331): left none 0.396, BC4 0.238, every sampler sRGB 0.103 (0.055; Xenia's foreground is in the real-time shadow, ShadowColor 0.38 against a lerp of about 0.6 -> x0.6 -> 0.062), right 0.351 / 0.163 / 0.070 (0.122). New York (`a5e94f008e60ab00`) and Road Atlanta (`20bef97d1cbb7c0d`) did not give a clear choice (their Xenia bands are probably shadowed; not added).
Change: `FAMILY_GAMMA` value `COLOURS` (every sampler decoded as sRGB once) for Laguna `8b9b73e9195be7b9`, Sunset `534d74061a777a9a` and Sebring `f9fbef3ac3660c4c`; for those draws the importer writes `"colour_srgb": true` into the material (and into its dedupe key), and `remaster.rs` uploads that material's colour layers (A / B / C) as sRGB. Laguna, Sunset and Sebring re-imported, remaster tables rebuilt.
Result (left | right, ours / Xenia): Laguna Seca 3.12 | 2.48 -> **1.43 | 1.09**, Sunset 1.52 | 3.15 -> 0.38 | 0.79 (right much closer, left now too dark), Sebring unchanged (1.90 | 1.11: the slab's colour is not on a colour role of its material, not checked further), other tracks unchanged. Sum of |ln ratio| 14.33 -> **12.37**.

## Update 2026-10-26 (12): Sebring slab scaled to its shader; two importer texture-id clashes and an engine texture-cache bug fixed: sum 11.16

- Sebring slab (`f9fbef3ac3660c4c`): its material (`h_diff_1_lm`) keeps only the white concrete 161 of a shader that also blends dark dirt layers (160 / 162 / 163), so the colour flag alone changes nothing (white is white in both colour spaces). New `FAMILY_SCALE` list: those families' single colour texture is always brought to the shader's own brightness (the family-albedo pass, otherwise opt-in with `FM2_FAMILY_COLOUR`): 161 x0.32.
- Importer bug: generated textures were numbered from shared bases, so passes overwrote each other's files when they ran on the same track: atlas crops, family colour scaling and chequer strips all from `0xFFFF_5000`; Nurburgring's BC4 road blend and the sprite pass both from `0xFFFF_4000` (seen as black-and-white chequer streaks over Sebring's pit lane). Now: 2000 baked roads, 3000 dark, 4000 BC4 road blend, 5000 atlas crops, 6000 family colour, 7000 chequers, 8000 sprites. All FM2 tracks re-imported.
- Engine bug (`fh1-remaster` `scenery.rs`): scenery images were cached by (track, texture id) only, so a texture bound as sRGB by one material family and linear by another took whichever loaded first (Sunset's flagged colour 252 sometimes came out linear). The cache key now includes the colour space.
Twelve tracks (`fam6_*`, ours / Xenia left | right): Tsukuba 1.03 | 2.51, Test Track 1.22 | 9.09, Sunset 0.38 | 0.79, Silverstone 0.89 | 1.09, Sebring 0.64 | 1.11 (was 1.90: now overshooting to dark), Road Atlanta 1.44 | 1.31, Nurburgring 1.26 | 0.29 (was 0.65 | 0.11: its baked road blend had been overwritten by sprites), New York 0.76 | 0.84, Mugello 0.98 | 1.63, Maple Valley 0.51 | 2.41, Laguna Seca 1.43 | 1.09, Suzuka 0.47 | 0.94. Sum **11.16** (17.68 at the start of this work).

## Update 2026-10-26 (13): shadow-aware patch check on New York, Road Atlanta, Sunset: no clear gamma choice, nothing added

Method: the engine's own shadow visibility at the 3x3 band points from a debug shot (`FH1_RM_FM2_SHADOW_DBG=1` paints the road with sh^3 in red; sh = ((R / 255)^2.2)^(1/3)), then the game's rule `min(1 - (1 - ShadowColor) * (1 - sh), lerp) / lerp` applied to the probe's lit values (lerp from the probe's lightmap value and the track's lightmap colours).
- New York (`a5e94f008e60ab00`, ShadowColor g 0.24): both bands fully shadowed (sh 0.03 to 0.05); factor 0.86 / 0.83. Left: no gamma 0.183, BC4 / colours 0.104, all 0.021 (Xenia 0.043); right 0.090 / 0.038 / 0.005 (0.032). "Colours" fits the right band (1.2x) but leaves the left 2.4x too bright; "all" the reverse. Meanwhile the engine draws New York at 0.76 / 0.84 of Xenia, darker than every shader variant on the left, so the engine and the interpreter disagree about this draw by about 3x (not resolved: the engine's material for this family may use another colour layer). Not changed.
- Road Atlanta (`20bef97d1cbb7c0d`, ShadowColor g 0.35): left fully lit (sh 1.0): no gamma 0.363 (0.85x Xenia), colours 0.147 (0.35x) -> linear fits; right partly shadowed in ours (3 of 9 points, sh 0.22 to 0.56; mean factor about 0.94): no gamma about 0.51 (4x), colours 0.21 (1.6x) -> sRGB closer. The bands disagree; Xenia's right band probably holds the car's shadow, which ours places elsewhere. Kept linear (current).
- Sunset (`534d74061a777a9a`, already `COLOURS`): ours is lit at both bands (sh 0.99 to 1.0), while Xenia's left band holds the car's shadow (its car shadow falls to the left-back, ours does not reach the band), so the left band cannot be judged by this method; the right band supports neither choice (colours about 0, none 3.4x). Kept.
Limit of the method: it uses OUR shadow placement; where the engine's real-time shadow falls differently from Xenia's (Sunset, Road Atlanta right), the comparison is still contaminated. Nothing changed in code or data.

## Update 2026-10-26 (14): why the engine draws New York's spawn road darker than its shader: the importer's wear heuristic

New York's spawn road draw (model 220, hash `a5e94f008e60ab00`: tile 1043, texture 7 = BC4 1024x512 wear mask, 208 (`0xd0`) = BC4 512x512 grain, 209 (`0xd1`) = BC1 colour) becomes material 1092 (`h_blnd2_ao_lm`): A = 209 (linear), B = `0xFFFF3000` (a generated copy of 209 darkened to 0.4 in linear light by the importer's wear pass, `FM2_WEAR_DARK`, default 0.4), weight = 7, lightmap 1043. So the engine blends the colour with a 0.4x copy of itself by the wear mask and drops the grain 208 entirely; that is a heuristic from before the shader interpreter, not the game's blend. The shader itself (interpreter at the patch, with the engine's full shadow there) gives 0.18 (no gamma) / 0.10 (sRGB) on the left band against Xenia's 0.043, while the engine's heuristic lands at 0.033 (0.76x of Xenia). So the engine is close to Xenia for the wrong reason, and the interpreter is 2.4x to 4x too bright for this family: replacing the heuristic with the shader's own blend would move New York away from Xenia until the interpreter's own gap is understood. Not changed. Open questions for this family: what the wear mask does in the game's shader (it may darken far more than its average suggests), and whether the real road under Xenia's band is this draw (painting not done here).

## Update 2026-10-26 (15): New York's road: confirmed by painting; the game's blend is not a wear darkening

Painting test (`bitpaint.py newyork_01 130 600 300 670`, relaxed threshold): texture `0xd1` (209), so the left band is the draw of model 220 (`a5e94f008e60ab00`). Its pixel shader (183 dwords, draw constants c1 1.25, c2 0, c3 0.5, sampler binding by index matches the roles: tf2 tile 1043, tf3 texture 7, tf4 texture 208, tf5 texture 209) reads texture 7 (BC4 1024x512) with the LIGHTMAP uv (the interpolator the tile also uses), so it is a large-scale overlay over the whole draw, not a tiled wear mask; the albedo is `0.5 x colour 209 + 0.5 x texture 7` (`mul r9, tf5, c3` / `mul r4, tf3, c3` / `mad r4, r9, c3.w, r4`), the grain 208 enters the specular / tint path (`c2 - tf4`), and the light is `2 x min(1 + (c105 - 1) x S, lerp(c184, c185, lightmap))`. So the importer's wear pass (lerp between 209 and a 0.4x copy of 209 by texture 7, see update 14) is not what the game does, but happens to land near Xenia. With the real blend the interpreter gives 0.18 (no gamma) / 0.10 (sRGB) at the band in full shadow against Xenia's 0.043; the remaining 2.4x is not explained by the shader inputs read so far (lightmap 0.19, ShadowColor 0.24, both doubled by the shader). Candidates not checked: the post-process (New York is a dark scene; FM2's final combine adapts to the frame's average luminance within `TrackLumLimits`), or Xenia's band not being lit by the same sun / shadow. Nothing changed.

## Update 2026-10-26 (16): FM2's final combine re-tested with the fixed lightmaps: stays off

`FH1_FM2_POST=1` on all twelve tracks against the current default (`fam6`, 11.16): computed blend 15.93 (it lands on the dark curve every time, same as forcing `FH1_FM2_POST_T=0`: 15.95), light curve forced (`T=1`) 24.09. Computed: brightens almost everywhere (Mugello 0.98 -> 1.53, Laguna 1.43 -> 2.55, Suzuka right 0.94 -> 2.81); New York unchanged (0.76 -> 0.73). Light curve: darkens hard (New York 0.07, Suzuka 0.04). So the post does not explain New York (the engine is already at 0.76 of Xenia there; the 2.4x is between the interpreter and Xenia), and the frame-average adaptation still cannot be reproduced (the engine's frame average stays below every track's `TrackLumLimits` minimum, as found on 2026-10-22). Off by default as before.

## Update 2026-10-26 (17): Test Track's right band (9.1x): the car's contact shadow in the game reaches it, ours does not

Shadow debug shot (`FH1_RM_FM2_SHADOW_DBG=1`) at the band points: left band lit (sh 0.99 to 1.0, lerp 0.83); right band mostly in the car's real-time shadow (sh 0.18 to 0.24, one point 0.55; lerp 0.82). With Test Track's ShadowColor g 0.31 the game rule gives 0.45 to 0.55 of the lit light there (0.37 in full shadow); ours renders right / left = 0.45. Xenia's right / left is 0.06. The missing factor (about 0.15 to 0.2) is what the car's contact shadow does in the game (FM2 GlobalCarAttribs `DropShadowSettings`: opacity 0.8 on the ground, i.e. x0.2): sun shadow 0.37 x contact shadow 0.2 = 0.074, about Xenia's 0.06. So in Xenia the patch beside the car is under both shadows, while our contact shadow (the FH1 port `car_shadow/drop_shadow.rs`, now also on FM2 cars) only rims the body. The game's code names `CCarPresentation::CalcDropShadowDisplacement` and a shader constant `c_DisplaceDropShadow` (strings in the FM2 image; no direct cross-reference found yet), which suggests the game moves the contact shadow away from the car, probably along the light direction; the engine port has no displacement. Next: find `CalcDropShadowDisplacement` in the game code (through the RTTI / vtable that names it) and port the displacement. The ground probe (`FH1_SHOT_PROBE`) returned no hits on Test Track (the collision ray misses there; not investigated).

## Update 2026-10-26 (18): Test Track's right band: the car's own shadow is drawn with DropShadowColor (near black) in the game

- `CalcDropShadowDisplacement` (RTTI `CParams5<ICarPresentation, CalcDropShadowDisplacement>`, a deferred call: wrapper `0x82575bb8`, handler `0x82575b68` calling vtable slot `0x9c` with four vectors and a float) is the same function the engine's FH1 port already implements: it fits the contact-shadow quad to the four wheel ground points (FH1 `0x831486B8`). No sideways displacement; that lead is withdrawn.
- FM2 has a second car shadow: the `RenderProjShadow` technique (effect `@82103d90`; TrackSettings `Performance FocusCarProjectedShadows / NonFocusCarProjectedShadows`). Its pixel shader reads a variance shadow map (`s_varianceShadowMap`, Chebyshev test against `c_shadowParams`), weights by n.L (`psLightDirWS`) and outputs a multiplier `lerp(c_shadowColour, 1, lit)` scaled by `alphaDirtAmount`. With `c_shadowColour` = the track's `DropShadowColor` (black on ten of the twelve tracks, 0.07 Sunset / Suzuka) the car's shadow on the road is near black, while scenery shadows in the road shaders use `ShadowColor` (0.24 to 0.5). What fills `c_shadowColour` for this pass is INFERRED (not traced in code).
- Test: forcing the engine's shadow colour near black (`FH1_RM_FM2_SHADOW_COL=0.03`; main.rs now lets an explicit value win over the track's): Test Track right band 9.09 -> **0.87**; Tsukuba, Maple Valley right bands unchanged (2.66 / 2.35: not the car's shadow there); Suzuka left 0.47 -> 0.38 (its scenery shadows darken too). So the car's shadow needs its own darkness, separate from the scenery shadows; the engine's single shadow map cannot tell them apart. Proposed design (not implemented): either a car-only projected shadow (the drop shadow's top-down silhouette projected along the sun onto the ground, darkened to DropShadowColor) with the car removed from the scenery shadow on FM2 tracks, or a car-distance term in the FM2 lightmap shader (needs the car position as a per-frame uniform). Needs a plan in docs/DESIGN.md first.

## Update 2026-10-26 (19): what fills the car shadow pass's `c_shadowColour`: traced part-way, NOT confirmed as DropShadowColor

- The car-shadow effect sets `c_shadowColour` in `0x82516700` (`GetParameterByName("c_shadowColour")` at `0x825169f4`, then SetVector) from a vector it reads at the start of that function through `0x82761208`: field `+0x30` of record 0 (0x290 bytes each) of a renderer table held by the global at `0x829f4610`.
- That field is written only by `0x82761058` (called once, from `0x8250d920`), with a vector the caller builds at `0x8250d750..0x8250d7c8`: `(obj+0x110, obj+0x114, obj+0x118, 1.0)`, where `obj` is the result of virtual slot `0x2c` on `this+0x14`; it is forced to (1, 1, 1, 1) when the byte `this+0xf99` is set, and when `this+0x14` is null the colour is the constant 0.33 grey (`0x820212e4`).
- TrackSettings' `DropShadowColor` is parsed into the settings object at `+0x610` (loader `0x82525918..0x82525970`) and copied to renderer `+0xee0` by ApplyTrackSettings. If `obj` were the settings object offset by 0x500, `obj+0x110` would be exactly `+0x610`; the other fields this caller reads from `obj` (`+0x60` as a byte, `+0x64 / +0x68`, `+0x70..0x78`) do not line up cleanly with the settings layout (`+0x560` is parsed as a float), so the link is NOT confirmed. What `obj` is stays open (candidates: the track's settings object through some accessor, or a separate car-shadow settings block).
- Empirical support stays as before: forcing a near-black car shadow fixes Test Track's right band (9.1x -> 0.87x).
Not built (the go-ahead was conditional on confirming `DropShadowColor`).

## Update 2026-10-26 (20): the car shadow pass uses ShadowColor (confirmed), and the game counts the car's shadow twice

Object found: `this+0x14` in the shadow-setup function `0x8250d568` (on the renderer) is the track presentation; its virtual slot `0x2c` is `CTrackPresentation@Presentation_R1::0x825069f8 = return this + 0x4e0` (RTTI scan of every vtable with a one-line getter in slot `0x2c`). The track presentation object is the TrackSettings owner (the loader's object with ShadowColor at `+0x5f0`, ShadowLightDirection at `+0x550`), so the fields the car-shadow setup reads are: `obj+0x110` = `+0x5f0` **ShadowColor** (-> `c_shadowColour` of `RenderProjShadow`), `obj+0x70..0x78` = `+0x550` ShadowLightDirection, `obj+0x60` = `+0x540` (byte; selects that direction: the override flag). The layout lines up on all three, so this is VERIFIED. `DropShadowColor` (`+0x610`) is NOT what the car shadow pass uses; the inference in updates 18 / 19 was wrong.
Consequence: the car's shadow on the road is darkened twice in the game: by the scenery shadow map in the road shader (the car is a caster there; `min(1 - (1 - ShadowColor) S, lerp)`) and again by the projected car shadow pass, a multiply towards ShadowColor. On Test Track that is about 0.37 x 0.31 = 0.11 of the lit road (plus the contact shadow), against Xenia's 0.06. Test with an equivalent forced colour (`FH1_RM_FM2_SHADOW_COL=0.1`, which also darkens scenery shadows): Test Track right band 9.09 -> 2.84, Tsukuba right 2.51 -> 0.65 (both car shadow), left bands unchanged.
Design changes accordingly (docs/DESIGN.md): Option A becomes "a projected car-shadow decal that multiplies the ground towards ShadowColor, ON TOP of the scenery shadow (the car stays a caster)", with the light direction from ShadowLightDirection when the override flag is set (else the default light direction).

## Update 2026-10-26 (21): projected car shadow (`FH1_FM2_CAR_SHADOW=1`): twelve-track sum 10.94 -> 9.38

Built (`crates/fh1-render/src/car_shadow/proj_shadow.wgsl`, `ProjShadowMaterial` / `update_proj_shadows` in `drop_shadow.rs`): per car a ground quad (fitted to the wheel ground points, reaching 8 m beyond the body) whose fragment walks from the ground point towards the light and samples the car's soft top-down silhouette (the drop shadow's 256x256 coverage, three 5x5 box blurs) at six heights between the body's bottom and top; the ground is multiplied by `1 - (1 - ShadowColor) x occlusion x n.L`, the game's `RenderProjShadow` formula, with the track's ShadowColor (from `FH1_RM_FM2_SHADOW_COL`, which the engine sets on FM2 tracks) and the light that casts the scenery shadow (so the car's shadow is darkened twice, as in the game). Off unless `FH1_FM2_CAR_SHADOW=1`.
Twelve tracks, fresh baseline `base12` against `cs12` (left | right): Tsukuba 2.37 -> 1.33 (right), Test Track 9.09 -> 5.23 (right), Road Atlanta 1.26 -> 1.00 (right), Maple Valley 2.30 -> 1.72 (right), Laguna Seca 1.43 -> **0.94** (left: the car's shadow falls on the foreground there), New York 0.76 -> 0.56 (left, worse), all other bands unchanged. Sum 10.94 -> **9.38**. Not tested: AI / traffic cars (each car with a drop shadow gets one), a race, uneven ground (the quad is one plane).

Update (same day): `FH1_FM2_CAR_SHADOW` is now on by default on FM2 tracks (`FH1_FM2_CAR_SHADOW=0` turns it off). It only acts where the engine sets `FH1_RM_FM2_SHADOW_COL`, i.e. on FM2 tracks with the game shadow rule on, so FH1 / other maps are not affected. Checked: Test Track's right band with no variable set matches the `cs12` run. Current twelve-track sum: **9.38** (17.68 at the start of this work).

## Update 2026-10-26 (24): patch probe on Nurburgring's right, Maple Valley's and Suzuka's left bands: no gamma changes fit

(Shader value at the band centre with the track's lightmap colours, lit G; engine shadow visibility from `FH1_RM_FM2_SHADOW_DBG`; Xenia in brackets.)
- Nurburgring (`4aa0eec057a0743d`, already BC4): left: family 0.062 (0.064, the shader matches Xenia; the engine draws 0.081 = 1.26x, its baked blend is a bit brighter than the shader); right: 0.080 (0.349). The engine draws about 0.10 there, close to the shader, so Xenia's right band must be a brighter surface than this road (a kerb / line or a different position); not a gamma question.
- Maple Valley (`b57319ed5f943822`, three textures, no BC4 sampler): 0.236 to 0.255 for every gamma choice (0.517 left); the engine draws about 0.26 = the shader. Xenia's left road is twice as bright as the game's own shader at this lightmap value (0.55); so either Xenia's band covers a brighter patch (sun-lit lightmap) or the lightmap / uv still differs there. Not resolved.
- Suzuka (`52ff81cbba901906`, already BC4): left lit 0.048 (0.040, matches); the engine draws 0.019 because our car shadow (real-time + the new projected one) falls on that band (visibility 0.61 mean, down to 0.18), while in Xenia it does not; right 0.027 (0.013).
No family added; nothing changed in code or data. The remaining differences at these bands are placement (which surface / which shadow is under the band), not shader gamma.

## Update 2026-10-26 (25): the track's own sun height (direction kept): not better

New variant `FH1_RM_FM2_SHADOW_DIR=height` (`light.rs`): our sun's compass direction with the track's TrackSettings sun height (32 to 38 degrees on the overridden tracks; Tsukuba / Sunset have no override and stay as they are). Twelve tracks against the current default (`cs12`, 9.38): sum 9.82. Maple Valley right 1.72 -> 1.48 and Nurburgring left 1.26 -> 1.09 improve; Laguna left 0.94 -> 1.24, Suzuka left 0.47 -> 0.36, New York left 0.56 -> 0.46 get worse; the rest unchanged within noise. Not adopted; the default stays. With this, every variant of the sun / shadow direction tried (full, fz, fx, fxz, height) is worse than the default on the twelve-track sum.

## Update 2026-10-26 (26): race test on FM2: AI cannot race on imported tracks yet

`FH1_AI_RACE=0:5` on `fm2/testtrackinfield_00`: no AI cars spawn: "AI line fm2/testtrackinfield_00 route 0: The system cannot find the path specified". The engine loads AI racing lines only from FH1's format (`<assets>/ailines/<track>/route_NNN.owt`, `ai.rs line_path`); the FM2 (and FM4) importer writes `imported/<map>/ai/line.json` (points of the racing line) and `track.json` timing gates, which only the hot-lap timer reads. So a race with AI cars on an FM2 track is not possible yet; it needs the engine to build a `RacingLine` from `ai/line.json` (feature work, not done). The player car alone was checked earlier (40 s drive, no errors). One warning seen in every FM2 run, "No previous asset to copy from for image" (a 256x256 RGBA image), appears with the projected car shadow on and off alike, so it is not from today's changes.

## Update 2026-10-26 (27): AI races on imported circuits (plan in docs/DESIGN.md, steps 1-5)

What changed (engine only, `crates/fh1-engine/src/ai/`):
- `line.rs`: `RacingLine::parse_imported` / `load_imported` build the AI's racing line from the importer's `imported/<map>/ai/line.json` (centre line, racing line, left / right wall distances; z flipped into engine space). Test `imported_line_matches_file`: rebuilt points within 0.005 to 0.09 m mean of the file on Test Track, Laguna Seca, Suzuka, Nurburgring and New York.
- `plugin.rs`: route 0 on an imported map is that line. `FH1_AI_RACE=<route>:<cars>:<car>:<laps>` on an imported map puts the AI on the track's own grid slots (`spawns.json` `grid_NN`) in that game's imported cars, the player on the next slot, and counts laps through the `track.json` finish gate (a car on the grid behind the line starts lap 1 on its first crossing; laps default 2). Finished cars cruise. `FH1_AI_LOG=1` logs each AI car every 5 s (lap, progress, offset from road centre and racing line, speed, resets, position).

Tested (release build, 60 fps cap, `FH1_PERF_LOG=1`, logs in `data/work/race5_*.log`):
- Laguna Seca, `FH1_AI_RACE=0:5::2` (earlier full run): all 5 AI finished 2 laps, laps 122 to 135 s (the AI's own prediction 113 to 122 s), finish order slots 2, 4, 1, 3, 5, 0 resets. Run with perf log to 131 s: frame time mean 16.68 ms, p99 17.9 ms, max 19.0 ms, no frames over 1.5x.
- Test Track, `FH1_AI_RACE=0:5::1`: all 5 finished 1 lap in 294 to 324 s, 0 resets, cars 0.2 to 1 m from the racing line at speed; frame time mean 16.68 ms, p99 17.5 to 17.7 ms, max 19.0 ms over 330 s.
- Errors: none. The one warning in every FM2 run ("No previous asset to copy from for image") is the known one from update 26.
- Screenshots: `screenshots/race_lag_131.png` (an AI Mercedes 300SL passing the parked player at the end of lap 1, its projected shadow under it), `screenshots/race_tt_14.png` (the field up the straight at 14 s).

Not done / not tested:
- The player's car is placed on the grid but nothing drives it, and its laps are only counted if you drive it yourself (not tested).
- The `off-road` counter in the AI log stayed 0 in every run; I did not check that it can count at all, so it is not proof the AI never left the road. Positions and the road half-width in the log show them inside the road at every 5 s sample I read.
- The line's own lap counter (`lap` in `FH1_AI_LOG`) reads one higher than the gate count, because the grid sits behind the line's start; the gate count is what decides the finish.
- FM4 circuits: not tested (same file format, same code path).

## Update 2026-10-26 (28): FM4 circuits, a reset bug on deep tracks, and an off-road (grass) count

FM4 test, `FH1_AI_RACE=0:5::1`, 5 AI in FM4's own cars:
- **Catalunya** (4.56 km): all finished, laps 133 to 172 s (the AI's prediction 126 to 164 s), 0 resets, frame time mean 16.68 ms (60 fps cap), p99 17.9 ms.
- **Bernese Alps festival circuit** (5.76 km): first run **broken**. No car finished, and each logged about 25,000 resets, one per physics tick, from about 50 s in. Cause: the AI's "fell out of the world" rule used a fixed floor of y = -50 m (made for Colorado); this circuit goes down to -99 m, so below -50 every car was put back on the line at 15 m/s every tick (the 54 km/h crawl in the log). **Fix** (`ai/driver.rs`, `ai/line.rs`): the floor is 15 m below the line's lowest point, or -50 m, whichever is lower. After the fix: all 5 finished in 135 to 167 s, 1 reset in total, Colorado: not re-tested; its floor only changes on a route whose lowest point is below -35 m, and then only gets lower (more lenient). I did not check whether any Colorado route goes that deep.

Off-road counter: the existing `off-road` count (past the line's half width + 1 m) cannot see leaving the tarmac on imported tracks, because their half width is the wall distance (Laguna wall to wall: median 32 m, up to 120 m; the tarmac is about 12 to 15 m). New count in the `FH1_AI_LOG` line: `grass N (s)`, the times two or more wheels went onto a surface with the game's OffRoadness > 0 (Dirt, Grass, Gravel traps...), using the vehicle's own `offroad_wheels`. Each excursion is logged with the wheels' surface ids, position and line position (`AI log grass:`).
- Control: the player on `FH1_AUTODRIVE` (circling) went onto grass and stayed there: `grass 1 (28.9 s)`. The count works.
- Finding, **not explained**: the AI do put two wheels off the tarmac often. In 60 s: Laguna 1 to 7 excursions per car (6 to 18 s on dirt), Catalunya 0 to 9 (up to 13 s), Bernese Alps 0. Checked from the files: the imported racing line itself is 100 % on asphalt on Laguna, Test Track, Catalunya and the Alps (`data/work/line_surf.py`), and the surface lists match the game's (FM4's 61 names extend FM2's 38 in order), so the dirt is real track-side data. Two spot checks: Catalunya's start straight runs the line 2 m from the grass, and a 1 m wobble puts the left wheels on it; on Laguna several excursions are right after the start, 7.5 m off the line, while the field merges from the grid slots. Possibly related: Laguna laps are 8 to 10 % slower than the AI's own prediction. Not fixed; it is step 4 of the race plan in `docs/DESIGN.md`.

## Update 2026-10-26 (29): a real race on imported circuits (race module), tarmac road width, and a correction

**Correction to update 27:** the `FH1_AI_RACE` dev race did **not** use the track's grid slots, as I wrote. `Track.spawn_names` holds display labels ("Grid 00"), and my filter looked for "grid_", so it never matched and every dev race used the old line-based grid. Fixed in `ai/plugin.rs` (accepts "Grid " too). The update 27 / 28 lap times are from line-based grids.

What changed (plan in `docs/DESIGN.md`, "a real race on imported circuits"):
- `race.rs` `Events::load_imported` (loaded in `ui/world_load.rs` for imported maps): one race per imported circuit from its own files. Grid = `spawns.json` `kind: grid` (player last). Gates = `track.json` sectors in waypoint order, then the finish (`start_gate` 0: the grid is behind the line). Path = the racing line. Route 0 = the imported AI line. Field = 7 of the game's own cars. Laps 3 (`FH1_RACE_LAPS` overrides). The game's car names come from its `cars/index.json` (FM4's repeated make dropped: "Nissan GT-R R34", not "Nissan Nissan GT-R R34"). The race module's Colorado-only check is gone; event-start markers stay Colorado-only.
- `FH1_RACE=0` starts it; `FH1_RACE_LOG=1` logs every racer's gate crossings and the results table.
- `FH1_AI_ROAD=1` (off by default): on imported lines the AI's road is the tarmac, measured at load from the collision surfaces (walk out from each racing-line point until the game's OffRoadness > 0.25, a 0.3 m step or no ground), instead of the wall-to-wall width. The AI use that width for passing, defending and merging room (`driver.rs` 663).
- Unit test `race::tests::imported_race_definition` (Laguna Seca, Catalunya): gates in race order along the line, the finish at the line's start, every grid slot behind the finish, field not empty. Passes.

Tested (release, 7 AI, 2 laps, player finished by `FH1_RACE_AUTOFINISH`, logs `data/work/race9_*.log`, `race10_*.log`):
- Laguna Seca and Catalunya: grid, countdown, all 7 AI pass the gates in order, laps counted, results with places and names. The return to free roam after the results pages: not checked (nothing logs it). Frame time 6.94 to 6.97 ms mean (144 Hz cap), p99 8.1 ms.
- Colorado regression check (`FH1_RACE=0`, Recaro Rush): starts with 7 named FH1 drivers, results as before.
- Grass / dirt time per AI car over the race, without / with `FH1_AI_ROAD=1`: Laguna 30 to 42 s -> 0 to 2.2 s; Catalunya 58 to 134 s -> 0.1 to 1.0 s. Resets 1 to 6 -> 0 to 2.

Not solved / not tested:
- **The AI are slow in a full race.** Lap 1 from the grid: Laguna 134 to 180 s, Catalunya 170 to 197 s, with or without `FH1_AI_ROAD`; lap 2: Laguna 101 to 125 s, Catalunya 143 to 150 s. In 260 / 330 s only 1 or 2 of 7 finished 2 laps. Keeping them on tarmac did not make them faster. Not investigated; one suspect: the start merge row for route 0 (`tables.start_merge(route)`) is FH1's own Colorado route 0 row, applied to the imported line too.
- The player in these tests was not driven (auto-finish). A race you drive yourself: not tested.
- The F6 event list / career screen with the imported race: not tested.
- FM2 / FM4 per-event lap counts and AI difficulty: not read (3 laps and FH1's medium AI ids used).

## Update 2026-10-26 (30): why the AI were slow in full races (mostly a test artifact), and what FH1_AI_ROAD costs

`FH1_AI_ROAD` is now on by default on imported circuits (`=0` = the wall width). New in `FH1_AI_LOG`: a pace line per AI car, time-weighted while racing (moving, not finished): speed / the line's profile speed, the driver's target / profile, speed / target. `FH1_AI_RACE_LINE_GRID=1` puts the dev race back on the line-based grid (for A/B).

Findings (Laguna Seca, logs `data/work/race11_*`, `race12_*`, `race13_*`):
1. **The main cause is the parked player car.** Nothing drives the player in these tests; it stands on its grid slot, on the racing line 17 to 100 m before the finish. The AI arrive at the end of every lap and crawl or stop behind it: one solo AI car stood at 0 km/h for 10 s or more 17 m before the line; in the full race, slots 1 and 2 were at 4 km/h beside it. Same solo car, same everything, the only difference being where the parked player stands: 121.8 s (line grid, player beside the line) against 135.6 s (real grid slot). The two runs were within 4 m of each other at every 5 s sample up to 125 s. So the slow full-race laps in updates 28 / 29 are mostly this, not the AI's own pace. In a race you drive, the player is not parked there.
2. **A real AI weakness behind it:** a stopped car on the racing line makes them crawl or stop instead of steering round it. That would also happen behind a crashed car. Not fixed.
3. **Start merge:** FH1's route 0 merge row does apply to the imported line; off (`FH1_AI_GAME_MERGE=0`) the 7 lap-1 times were 145 to 171 s against 146 to 180 s on (one run each, and both include finding 1). Small at most.
4. **`FH1_AI_ROAD` costs pace:** clean solo lap (line grid, nothing in the way), same car: 121.8 s without, 129.4 s with (+6 %); target / profile 1.00 in both. Likely cause, **not verified**: the driver keeps `track_edge_clearance` from the road edge (`driver.rs` 663), and the game's line uses the tarmac right to the edge at apexes, so the car is pushed inward there.
5. The AI's own solo pace: 121.8 s against its own prediction of 113.6 s (7 % slower), reaching 75 % of its target speed on average (that average includes the standing start and corners, so it is not a clean measure of grip).

Not done: fixing 2 or 4; a test harness that moves the parked player out of the way, so full-field AI pace can be measured.

## Update 2026-10-26 (31): why FH1_AI_ROAD cost 6 %, fixed

**First guess wrong:** the driver's passing clamp (`driver.rs` 663, road half width - TrackEdgeClearance - own half width, about 1.4 m) does not bite: on the tarmac-fitted Laguna line it would move 0 of 1807 points (the fit keeps each side at least 1.5 m).

**Real cause:** every AI driver races `RacingLine::limited`, FH1's AIRacing.xml ChiLimit: the car's centre keeps 2 m from the road edge on the inside of a bend (1 m past it allowed on the outside). On wall widths this never binds; on tarmac widths it pulls the game's own apex line inward at 412 of 1807 points on Laguna (0.37 m mean, 0.5 m worst) and 912 of 2330 on Catalunya (0.42 m mean). Those small moves add kinks, and the speed profile reads curvature over short chords, so the AI's own predicted lap went 113.6 -> 122.9 s on Laguna.

**Fix** (`ai/line.rs`): an imported line is marked as the game's own (`RacingLine::game_line`); `limited` keeps its shape and only adds the driver's ChiVariance shift. `FH1_AI_GAME_LINE_LIMIT=1` = the old behaviour. FH1's own routes are unchanged (AI unit tests pass).

Re-measured (one solo AI car, line-based grid, nothing in the way, logs `data/work/race14_*`):

| | lap | AI's predicted lap | time on grass / dirt |
|---|---|---|---|
| Laguna, `FH1_AI_ROAD=0` | 121.8 s | 113.6 s | 2.5 s |
| Laguna, tarmac fit, before the fix (update 30) | 129.4 s | 122.9 s | - |
| Laguna, tarmac fit, fixed | **120.2 s** | 112.2 s | 0.5 s |
| Catalunya, tarmac fit, old limit | 172.6 s | 169.3 s | 0 s |
| Catalunya, tarmac fit, fixed | **160.1 s** | 143.9 s | 3.9 s |

So `FH1_AI_ROAD` (on by default) now costs nothing on Laguna and keeps the car on the tarmac. Still open: the solo car is 7 % (Laguna) to 11 % (Catalunya) slower than its own prediction; the AI still stop behind a parked car (update 30). Both not investigated.

## Update 2026-10-26 (32): AI no longer crawl into a stopped car

**Repro:** one AI car on Laguna Seca, the player parked on the next grid slot (the dev race); the AI arrived at 127 km/h, slowed to 3 km/h, touched the parked car, sat still for about 10 s until the stuck rule reset it. Lap 134.4 s against 120.2 s with nothing in the way.

**What the log showed** (`FH1_AI_TRAFFIC_LOG=1`, new: every 0.25 s each pass / follow decision and each speed cap from a car ahead): the driver chose to pass on the right from 80 m out, correctly, but
1. the pass slot sat exactly at the minimum distance (`need`, bodies + clearance) from the parked car, and
2. the car's actual position stayed about 1 m inside its planned path: while passing, the driver aims at the racing line shifted by the pass offset ("keep the path line-shaped"), and here the game's line swings across the straight towards the parked car, so the aim point drifted back towards it.
So the "car under my nose" stopping-distance cap (`driver.rs`, uses the car's actual position) stayed on the whole way and braked it to a crawl.

**Fix** (`ai/driver.rs`, both on by default):
- `FH1_AI_PASS_STATIC` (`=0` off): passing a stopped / very slow car (under 5 m/s), the driver aims at the slot's own position across the road instead of the line-shaped path. Passing moving cars is unchanged.
- `FH1_AI_PASS_MARGIN` (metres, default 0.5; `=0` the old slot): the pass slot sits that far beyond the minimum.
The margin alone did not help (134.4 s); the aim change did.

Tested (logs `data/work/race15_*`, `race16_*`):
- Solo car, parked player: 134.4 s, 1 reset -> 121.3 s, 0 resets (120.2 s with nothing in the way); passes at about 58 km/h.
- Laguna, 7 AI, 1 lap, parked player: lap 1 151 to 176 s -> 116 to 169 s (5 of 7 under 132 s; slots 3 and 7 had other incidents, 1 and 2 resets).
- Colorado regression check (Recaro Rush, 160 s, one run each): resets 14 (old) -> 9; lap 1 75 to 109 s (old) -> 76 to 87 s. No sign of a regression; single runs, so not proof of an improvement.

Not tested: passing a stopped car in a corner; a race with a driven player.

# 2026-11-01 (late): W1 blocked -- the texture fetch constants are not in the parsed draw stream
**Where the road-parity work stands.** The plan's acceptance metric (`data/work/parity_g1t.txt`, road-mask sum |ln|) is **5.21** against a target of <= 2.0, with no track within +-10 %. Five decode rules have been measured on all twelve tracks:

| rule | road-mask sum | note |
|---|---|---|
| per-shader family table (default) | **5.21** | family albedo ~2x too dark |
| no decode at all | (band 2.9x too bright) | reference for the others |
| per-texture game word (`FM2_WORD_GAMMA`) | 5.61 | decodes **nothing** on road draws (word == none) |
| per-texel bake | 5.33 | mean == flat average |
| flat bake | 4.96 | (old W2-default shots) |

**The block.** The console's decode is **per texture fetch**, not per texture file, so the right mask needs the fetch constant's format/degamma field. Checked where it could be:
- the draw state records (`Draw::state`, new `FM2_PROBE_STATE=1` dump) hold only three 2-word render-state records for Tsukuba's road draw (`0x0C 00000148/00000000`, `0x08 0000018C/00040001`, `0x05 000000F0/00480004`) -- **no 6-dword fetch constants**;
- the `0x2d` stream record carries sampler **bindings** (`count`, then `(item, ...)` pairs), and its parsed `items` are already verified working;
- the texture table word (`0x1A200186` etc.) is the CAFF header dword, and its bits 8-13 do not behave as a decode flag (the sky textures also read `0x01` yet need sRGB; the word rule decodes nothing on roads).

So the fetch-constant data is either in a command-stream record the parser skips, or in the PVS/texture records not yet decoded. Finding it needs reverse-engineering (dump the raw `.stream` around a road draw and identify a 6-word structure matching the texture's format word), which is a research task, not a safe incremental step.

**What is measured and reusable.** `FM2_PROBE_AT` (band world point -> draw, tile, lightmap, lit value per sampler mask, now including the `word` rule), `FM2_PROBE_STATE`, `FH1_FM2_CONTRAST=2` (frame average and `t` in one shot), the grade-off stage split (`FH1_FM2_POST=0` + `FH1_RM_LUT=0`), and the content-aware road mask in `data/work/parity.py`.

**What is known about the error's shape.** Pre-post (grade off), Tsukuba's road product matches Xenia at **1.09** and Sebring at **0.93**; Silverstone is 0.38, Suzuka 0.24, Maple Valley 0.01 of Xenia. The needed albedo sits strictly between the family value and the no-decode value on every track checked (Tsukuba 0.152 between 0.069 and 0.273), and no subset of tf3..tf6 reaches it. The post `t` cannot fix any of it: on Tsukuba the scene is right pre-post and the computed blend overshoots to 2.23.

**Recommendation.** Next attempt: dump the raw `.stream` bytes around a road draw and look for the fetch-constant record (6 dwords per sampler, format word matching the texture's `0x1A200186`). If found, thread the degamma bit into `shader_eval_at`'s `sample()` and the road bake as a per-sampler mask. If not found after two real attempts, accept the family table and stop calling the road within scope of the current data.

**2026-11-01 (later, MODLOG 171): the fetch-constant search is complete and negative.** Five checks: (1) draw state has no fetch constants; (2) the record type/length histogram shows `0x03` is the only 6-word type and all 4146 are identical; (3) the `0x2d` payloads are `(item, slot)` pairs; (4) a raw scan of every Tsukuba model for the texture word finds **0 occurrences**; (5) the five textures of the road draw (lightmap 150, colours 130/144/145) all carry the **same word `0x1A20017B`**, so the word encodes no per-texture information. The per-fetch decode is **not present in the data the project parses**; the family table is the only lever and is a heuristic. The acceptance target needs a new data source (a capture of the console's executed fetch constants), not another rule.
