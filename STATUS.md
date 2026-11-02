# STATUS, 2026-11-02: FM2 visual parity — where it stands after the measurement rebuild

> **This replaces the previous STATUS.md** (the 2026-11-02 "detailed handoff" and the older material below it). That text is preserved in git history: `git show 4f30844:STATUS.md`. Everything still true from it is carried forward here; everything it recorded about the old **5.08** acceptance figure is now void, because that figure was not a measurement (section 3).
>
> The plan is `docs/DESIGN.md` (git-ignored): plan v6 (per-bucket fixes) and plan v7 (per-game visual profiles). Every number below is in `MODLOG.md` entries 223-245.
>
> **The goal, restated by the user 2026-11-02:** first reach visual parity with FM2; have the findings make the other Xbox 360 Forzas cheaper; ideally switch between each game's visual style. FM2 parity is the active workstream.

## 1. The short version

**The goal is not met.** On the corrected metric the twelve circuits read **6.04** (sum of |ln| of the median ours/Xenia road ratio) against a target of **2.0**, with **every track off** (best: New York 0.48; worst: TT King Cobra 3.49). Ten weeks of work was spent judging progress with a number that could not be reproduced when a threshold moved 3.3x (section 3); the measurement was replaced this session, and with a trustworthy instrument the picture is different from what the old notes said.

**What the new measurements establish:**
- The road draws are named on all twelve tracks and the **game's own shader** can be evaluated at the exact pixels we measure, so changes can be judged without fitting to a screenshot.
- Our engine renders **0.04-0.62 of the game's own shader value** at the same road points -- never above it.
- Road **albedo alone** is **0.18-1.11** of the game's (median ~0.5), but making it *more* faithful makes the picture *worse* every time it was tried (twice, both buckets).
- Our **pre-post scene** is short of light (0.40-0.87 of the reference on nine tracks), and FM2's own tone-curve blend, **pinned at its dark end (t ~ 0.02) because of that shortfall**, then over-brightens the darks and compresses the highlights. That is the visible signature: shadows too bright, highlights too dark.
- The **shadow machinery is not the lever** either: coverage on clean far road is 3 % against the game's 14 %, but distance, caster cull, decal colour, the shadow gate and the sun direction were all tested and none improves the acceptance.

**The honest position:** every cause is now named with numbers; no remaining change inside the plans has an evidence-backed mechanism. The decision left to the user is at section 12.

## 2. How it is measured now

One command:

```powershell
python -I data/work/acceptance.py <prefix>        # ~8 min, all twelve tracks
```

It shoots each track at the reference pose, then compares against `screenshots/xenia_ref/g1_*.png` on pixels that are:
1. **proven asphalt** by the engine's own collision ray under that pixel (`surfaceTypes.xml` OffRoadness = 0),
2. **neutral in both pictures** (absolute chroma < 0.12 -- a hue test, never brightness),
3. **frozen** once per track in `data/work/anchor_pixels.json`, so an A/B difference is the render's and not the filter's pick,
4. **binned by the reference's own brightness** into dark / mid / bright thirds, because a single median hides a track whose darks and brights fail in opposite directions.

`median` is the plan's acceptance statistic; `err` is the mean |ln| over the three bins. **Target: sum <= 2.0, every track <= 0.10.**

**Reference set is frozen:** `data/work/reference_hashes.txt` holds the sha256 of all twelve pictures plus the commit they were scored on; the harness refuses to compare if one changes.

**Repeatability, measured:** per-track bins reproduce exactly across runs (Silverstone 3.64/0.62/0.46 twice; 3.81 twice after a different build). The **aggregate sum drifts by about +-0.5 with unchanged code** (5.38 -> 5.97 -> 6.04 across the session), so only per-track, same-build A/B comparisons support a claim; a "total improved by 0.3" would not.

## 3. Why 5.08 was retired

`data/work/p0_audit_rev.txt` recomputes the old mask over nine threshold settings. It reproduces 5.08 exactly, and then:

| symptom | evidence |
|---|---|
| the number moves up to **3.3x** when one threshold is nudged | Road Atlanta 0.35 .. 1.43, Maple 0.38 .. 1.40, Tsukuba 1.25 .. 2.82, Suzuka 0.34 .. 1.46 |
| a large share of the "asphalt" is **the car** | 25 % of selected pixels on Tsukuba, 51 % on Laguna Seca, 54 % on Maple lie inside the car box |
| the intersection is the **biased subset** | Tsukuba: 26,079 pixels qualify in ours but only 2,028 in Xenia; the 3,446 that survive are where the two happen to agree |
| it **cannot report all twelve** | New York yields 24 common pixels (200 needed) |

This is the same class of error as MODLOG 161 ("the road band was the HUD"). **No number measured before this rebuild should be quoted.**

## 4. The numbers today

### 4.1 Acceptance (current build, restored defaults)

| track | median | bins dark/mid/bright | err | albedo (ours/game) | bucket |
|---|---|---|---|---|---|
| Tsukuba | 1.28 | 2.36 / 1.33 / 1.00 | 0.38 | 0.70 | A |
| Laguna Seca | 1.57 | 4.62 / 1.25 / 1.07 | 0.61 | 0.42 | mixed |
| Road Atlanta | 1.39 | 1.48 / 1.69 / 1.19 | 0.36 | 0.27 | A |
| Silverstone GP | 0.60 | 3.89 / 0.62 / 0.46 | 0.87 | 0.90 | B |
| Nordschleife | 0.77 | 1.38 / 0.75 / 0.38 | 0.52 | 0.49 | A |
| Sebring | 1.21 | 1.73 / 1.33 / 0.69 | 0.41 | 1.11 | B |
| Mugello | 0.86 | 1.07 / 0.87 / 0.76 | 0.16 | 0.48 | A |
| Suzuka | 0.60 | 1.25 / 0.52 / 0.53 | 0.50 | 0.46 | A |
| Sunset Infield | 0.53 | 0.74 / 0.54 / 0.34 | 0.67 | 0.70 | A |
| Maple Valley | 2.15 | 3.25 / 2.15 / 0.48 | 0.89 | 0.29 | mixed |
| New York | 0.48 | 1.00 / 0.74 / 0.12 | 0.80 | 0.20 | mixed |
| TT King Cobra | 3.49 | 5.91 / 1.06 / 0.79 | 0.69 | 0.18 | mixed |

**sum |ln(median)| = 6.04, sum of bin errors = 6.86.** Buckets: **A** = albedo low and the scene bins point the same way; **B** = albedo right, error elsewhere; **mixed** = albedo low *and* shadows too bright (something adds light the game does not).

### 4.2 The tone-curve blend (W3, all twelve)

The game blends two tone curves by the frame's average luminance against each track's `TrackLumLimits`: `t = saturate((avg - min)/(max - min))`, t=1 = light curve.

| track | avg | min | max | t |
|---|---|---|---|---|
| Tsukuba | 0.2310 | 0.3333 | 1.4902 | 0.002 *(clamped low)* |
| Laguna Seca | 0.2792 | 0.5882 | 0.8431 | 0.000 *(clamped low)* |
| Road Atlanta | 0.3086 | 0.1569 | 1.6510 | 0.023 |
| Silverstone GP | 0.4154 | 0.3725 | 0.9412 | 0.021 |
| Nordschleife | 0.4956 | 0.0000 | 1.3725 | 0.109 |
| Sebring | 0.5000 | 0.3686 | 1.0588 | 0.048 |
| Mugello | 0.4234 | 0.0784 | 1.1608 | 0.105 |
| Suzuka | 0.4523 | 0.4510 | 1.2157 | 0.003 |
| Sunset Infield | 0.1831 | 0.2078 | 0.8275 | 0.027 *(clamped low)* |
| Maple Valley | 0.2254 | 0.2314 | 1.0980 | 0.017 *(clamped low)* |
| New York | 0.0560 | 0.0000 | 1.4941 | 0.014 |
| TT King Cobra | 0.3188 | 0.3137 | 0.4902 | 0.258 |

**t = 0.00-0.26, median ~0.02.** Our scene is dark enough that the game's own auto-exposure always picks the "brighten the mids" curve -- which is the measured cause of the shadow-lifting signature, and it means the curve is **downstream** of the scene error, not the cause.

### 4.3 The material stages (albedo / light term / product, no post curve)

| track | albedo | light term | product | product ÷ Xenia road |
|---|---|---|---|---|
| Silverstone GP | 0.0513 | 0.9911 | 0.0513 | 0.50 |
| Sunset Infield | 0.1046 | 0.5906 | 0.0630 | 0.40 |
| Maple Valley | 0.1221 | 0.3515 | 0.0545 | 1.43 |
| Tsukuba | 0.1441 | 0.4621 | 0.0666 | 0.57 |
| Laguna Seca | 0.0612 | 0.0762 | 0.0578 | 0.87 |
| Road Atlanta | 0.1651 | 0.2623 | 0.0802 | 0.75 |
| Nordschleife | 0.0802 | 0.6105 | 0.0513 | 0.56 |
| Sebring | 0.6654 | 0.5841 | 0.4969 | 1.30 |
| Mugello | 0.1981 | 0.5395 | 0.1095 | 0.66 |
| Suzuka | 0.0262 | 0.1620 | 0.0144 | 0.47 |
| New York | 0.0091 | 0.0116 | 0.0065 | 0.56 |
| TT King Cobra | 0.0284 | 0.0356 | 0.0262 | 2.87 |

**Reading:** the pre-post road is **too dark on nine tracks** (0.40-0.87) and **already too bright on three** (Sebring, Maple, TT). The tone curve then amplifies the shortfall into the opposite error in the final image -- which is why Tsukuba, Road Atlanta and Laguna end up *too bright* while their scene is *too dark*.

**Caveat:** `product` and `albedo x light term` disagree by up to 65x on the darkest tracks (Road Atlanta 0.080 vs 0.043, Suzuka 0.014 vs 0.004, New York 0.007 vs 0.0001). Where they disagree the split between albedo and light term is **not** quotable; the `product ÷ Xenia` column is.

### 4.4 Shadow coverage (Silverstone)

`S = curve^-1(displayed) / unshadowed material`; coverage = share of road points with S < 0.6, depth = median S among those.

| region | coverage | depth |
|---|---|---|
| game, all road | 35 % | 0.42 |
| ours, all road | 2 % | 0.51 |
| game, **far road only** (y < 540) | **14 %** | 0.56 |
| ours, **far road only** | **3 %** | 0.51 |

**Coverage is the gap; depth already matches.** The near-car band (game 91 %, ours 0 %) is **contaminated** by the car body and its decal and is not used -- the reference there is 6 % of its unshadowed value, which the game's own shadow rule cannot produce.

## 5. What is proven (do not re-derive)

| fact | evidence |
|---|---|
| FM2's textures are exactly the disc's: BC4 byte-identical, BC1 value-identical; no gamma decode anywhere in the session | MODLOG 181, 219, 175, 179 |
| The colour-space order is irrelevant (0.39/0.49/2.24 vs 0.41/0.49/2.24) | MODLOG 217 |
| The material/remaster tables must be rebuilt after every import or materials keep stale flags | MODLOG 196 |
| The road draw under the car, all twelve tracks (model / shader signature / material) | MODLOG 226, `p1_draws_acc.txt` |
| The game's own shader can be evaluated at our exact road points (`FM2_PROBE_AT` + `FM2_PROBE_LERP`), and its outside lightmap multiply is not a double count | MODLOG 230, 234 |
| FH1's grade (tonemap/LUT/vignette) is already off for FM2 tracks; the FM2 combine is the game's own | MODLOG 228 |
| Our pose matches the reference: gradient NCC peaks at (0,0) on eight tracks, (0,-2)/(0,-4) on four | MODLOG 225 |
| The shadow map is active at the grid (`sh` 0.47-0.62 next to the car) and the lightmap gate is already open there | MODLOG 237 |

## 6. What was measured and rejected (do not repeat)

| hypothesis | measurement | verdict |
|---|---|---|
| The bake should use the draw's own shader albedo (W1) | Maple 2.15 -> **3.62**; six A-bucket tracks 2.44 -> **3.36** (albedo more faithful: 0.29 -> 0.65) | rejected, reverted |
| Gate the road's sun/specular term on the lightmap (W2.3) | shadow map already active, gate already open near the car | moot |
| Use the game's `DropShadowColor` (black) for the car decal | Silverstone 3.64 -> 3.81, Maple 3.07 -> 3.21 | rejected, reverted |
| Lightmap light floor 0 (the game has no floor) | 2.74 -> 2.76, dark bins unchanged | rejected (3rd confirmation) |
| Shadow distance 600 / small-caster cull off | coverage unchanged (3 %), depth only (0.51 -> 0.19-0.21) | rejected |
| Global sun-direction flip (`fx`) | coverage 3 % -> 10 %, but Nordschleife 0.54 -> 0.82 | rejected (trades tracks) |
| A per-track sun-direction fix | the game's `0x825220f0` only copies the XML vector; the engine already does the same | nothing to fix |
| Per-track gains / fitted tone tables | "drawing the target" | standing non-goal |
| ~15 earlier hypotheses (mirrored lightmap, shadow reach, squared ShadowColor, post tuning, per-format decode, forced albedo, real varyings) | all measured | rejected, see MODLOG <=222 |

## 7. What remains unknown

1. **Why the pre-post scene is short of light by ~1.5-2x.** The stage paints localise it to albedo and the light term, but on the tracks where the two paints disagree the split is unmeasurable (section 4.3 caveat).
2. **Why our far-road shadow coverage is 3 % where the game's is 14 %** -- the caster set, not the range (section 4.4).
3. **Whether the reference's road is darker than `albedo x lightmap` because of its display path** (the game's frame average is unknown to us; our estimate of it is what pins t at 0).

## 8. Tooling built for this (in `data/work/`, git-ignored)

- `acceptance.py <prefix>` -- the judged number for all twelve tracks; writes `acceptance_<prefix>.json`.
- `p0_probe.py` -- the asphalt-anchored metric itself (frozen pixels, bins, pose check).
- `p0_audit.py` -- why the old mask failed (threshold sweep, car share, asymmetry).
- `p2_avg.py` -- the blend input t against each track's `TrackLumLimits`.
- `p3_dbg.py` -- per-stage paints (albedo / light term / product).
- `p3_alb.py` -- road albedo against the game's own shader, per track.
- `p3_oracle.py` -- game shader vs engine vs Xenia at the same pixels.
- `w2_shadow.py` -- shadow coverage and depth, ours vs the game.
- `p1_draws.py` -- names the road draw from the engine's probe points (`FM2_UNDER`).
- `compare.py` -- Xenia vs engine comparison sheets (`screenshots/compare_*.png`).
- `reference_hashes.txt`, `anchor_pixels.json` -- the frozen reference and frozen pixel sets.

**Engine/importer changes made this session (committed):** the shot probe logs each pixel's surface id and off-roadness; `FM2_PROBE_AT` gained a height tolerance and diagnostics. Both are logging/diagnostic only.

**Environment:** ViGEmBus 1.22.0 installed (service running), `vgamepad` 0.2.0 built into `data/work/venv`, and a virtual Xbox 360 pad verified working (`data/work/vpad_test.py`) -- the 2026-11-01 emulator-input blocker is liftable, though the emulator route is no longer on the critical path. Instrumented Xenia (`data/work/xenia-build`, commit `828014b9`) and its texture dumps (`data/work/texdump4`) are unchanged.

## 9. How to reproduce a measurement

```powershell
# the judged number (twelve tracks, ~8 min)
python -I data/work/acceptance.py <prefix>

# one variant on the frozen pixels
python -I data/work/acceptance.py <prefix> --env FH1_FM2_POST_T=0.5 --tracks=maplevalley_00

# the game's own shader at our road points (no reference image involved)
python -I data/work/p3_alb.py --tracks=maplevalley_00 --per-bin=4

# the blend input, all twelve
python -I data/work/p2_avg.py

# comparison sheets
python -I data/work/compare.py --tracks=tsukuba_00,silverstone_00,maplevalley_00 --out=three
```

After any import: `.\target\debug\fh1setup.exe "data/work/disc" --only remaster --force` (the stale-table trap in section 5).

## 10. The goal after this: per-game visual profiles (plan v7)

The user's goal is FM2 parity *first*, with the findings reusable for the other 360 Forzas and a switch between visual styles. Today the FM2 style exists only as **sixteen `if track.id.starts_with("fm2/")` blocks** in `crates/fh1-engine/src/main.rs` (146-420) that set environment variables -- no second game can join, values cannot be listed, and inferred fits look identical to values read from the game's files.

Plan v7 (`docs/DESIGN.md`) turns that into a named profile per game with a hard rule: **every field must be either read from that game's files or measured against the reference with a MODLOG entry naming it -- there is no third category.** Steps: v7.1 extract the FM2 profile with **no behaviour change** (gate: acceptance reproduces 6.04 / 6.86 exactly), v7.2 the `FH1_VISUAL_STYLE` switch, v7.3 FM2 parity inside the profile, v7.4 FM4 as the first other game.

## 11. Not tested

- No rendered change was adopted this session: the tree's rendering is exactly the committed state.
- The comparison sheets are visual artefacts; no claim in this document rests on them.
- A full lap or race on FM2 scenery by a person; frame cost beyond one HUD reading per circuit.
- The emulator capture route with the new virtual pad (installed and proven, not yet used to drive Xenia).

## 12. The decision left to the user

Acceptance 6.04 against 2.0, every cause named, no evidence-backed change left inside the plans:

1. **Accept** the measured state with this document as the record.
2. **Adopt a clearly labelled calibration** (a per-track gain fitted to the reference). It would very likely reach <= 2.0, but it is drawing the target, not reproducing the game -- and it conflicts with the "game data before guesswork" rule unless labelled as such everywhere.
3. **Park** FM2 road parity and spend the effort on the profile/switch structure (plan v7) or on another area.

## 13. Older material that still stands

- **FH1 itself** (the engine's own game) works per the README: full Colorado map, 175 cars, races with AI, garage, the remaster renderer. Only regression-tested lately (a career race starts, runs and finishes).
- **Xbox One family cars** (FM6 IDx, FM5 E21, FH2 F-Type, plus Fast7 GT-R, Rocket 69, FH2 R34): imported, load and drive; the user has driven four of them ("very rough but satisfy me for now"). Known gaps: normal/gloss/AO maps ignored, headlight models skipped, the launcher does not list those games, car names still come from media names. Full detail in the previous STATUS (git history) and MODLOG 97-136.
- **Reference capture rig:** isolated Xenia install in `tools/xenia-test/` (git-ignored), `kernel_display_gamma_type = 1`, twelve longest layouts captured at the grid, capture via `PrintWindow` while unfocused (colour-faithful to 0.02 levels). The 47-entry lobby track list is mapped to our import ids.
- **Imported races** work only through the dev hook `FH1_RACE=0`; they are not in the menus.
- **Mistakes worth not repeating** (this session's additions): trusted a mask that changed answer by 3.3x; reported `FM2_PROBE_AT` as broken when a log filter had simply missed its output line (corrected in MODLOG 230); used a near-car band whose reference pixels were the car's own decal (MODLOG 237); and quoted an aggregate score without knowing it drifts +-0.5 with unchanged code (MODLOG 238).
