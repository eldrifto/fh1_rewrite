# STATUS — FM2 visual parity, 2026-11-13 (fifth pass, final)

> Replaces the 2026-11-09 revision (whose claims are preserved and corrected in `MODLOG.md` 504–555 and in §7 below).
> `docs/GOALS.md` remains the authority on priorities; `docs/PLAN.md` is the current work order.
>
> Written for a cold start: everything needed to build, measure, and continue without asking.

---

## 1. The short version

**Three of the four goals of this pass are met and shipped, with gate evidence. The fourth is met on every track where it
can be scored, and the rest cannot be scored with the existing references.**

| step | state | evidence |
|---|---|---|
| **2 — near-field detail** | **met, shipped** | 12-track detail **median 0.92**, **8 of 12 at or above 0.85** (7 of 12 on the repeat — see §4), road mask **5.02 → 5.04** (unchanged), 0 shader errors |
| **3 — car paint** | **met, shipped** | car luminance **1.42 → 1.26**, car mask `sum \|ln\|` **0.35 → 0.23** |
| **4 — HUD plates** | **met, shipped** | plates **RACE 1.11 / LAPS 1.00 / PLACE 0.83 / tach 0.97** (was 0.92 / 0.80 / 0.68 / 0.75) |
| **1 — props** | **met 3 of 3 scorable** | props reduce the error on **Nordschleife −9.19, Road Atlanta −1.29, Suzuka −0.06**; the other 9 references contain **0–297 px** of props |

**Shipped changes (all measured, all default-on unless stated):**

| change | effect | MODLOG |
|---|---|---|
| crowd clock pinned under `FH1_SHOT` | capture noise **9,893 → 5–111 px** | 506 |
| paint albedo 0.85 → **0.6** (the game's own `ColorScale`) | car 1.42 → 1.26 | 508 |
| HUD dark-panel coverage 1.146 → **0.85** | plates 0.92/0.80/0.68/0.75 → 1.11/1.00/0.83/0.97 | 517 |
| props through the **standard** material (remaster on Laguna) | Nordschleife **+15.81 → −6.5…−9.19** | 523, 530 |
| prop placement **dedupe** in the importer | 50→36, 556→196, 578→139 … | 512 |
| scenery **mip bias 0.35** | detail median **0.69 → 0.92**, gate 2 of 12 → 8 of 12 | 540 |

**The single most important fact for anyone continuing:** almost every earlier conclusion about *why* FM2 looked different
was limited by the measuring instrument, not by the renderer. Three separate instrument defects — the crowd running on
real time, the NCC gate, and prop streaming — were mistaken for rendering faults and drove entire passes of work. They are
now fixed and documented in §8. **Check the instrument before believing a difference.**

---

## 2. Build, run, verify

```powershell
# importer / setup tool (FM2 needs the feature)
cargo build --release -p fh1setup --features fm2
cargo build --release -p fh1setup            # FH1 / Colorado paths, no feature

# the engine — --no-default-features is required
cargo build --release -p fh1-engine --no-default-features

# after material.rs / car.rs / post.rs edits
cargo build --release -p fh1-remaster
```

**Parity harness — the steering instrument, not the verdict.**

```powershell
# twelve tracks; ~65 s each with the pose lock (see §8.1)
python -I -X utf8 data\work\parity.py <prefix> --env FH1_SHOT_LOCK=1 --env FH1_SHOT_READY=0

# one track, or a few
python -I -X utf8 data\work\parity.py <prefix> --tracks=tsukuba_00,sunsetinfield_00 --env FH1_SHOT_LOCK=1 --env FH1_SHOT_READY=0

# re-score existing shots without re-capturing
python -I -X utf8 data\work\parity.py <prefix> --report-only

# near-field detail per track (the step-2 gate)
python -I -X utf8 data\work\plan\detail.py <prefix>
python -I -X utf8 data\work\plan\detail_masked.py <prefix>
```

**The blind gate — needs the user.**

```powershell
python -I -X utf8 data\work\ab_test.py --prefix <prefix> --trials 30 --region top
python -I -X utf8 data\work\ab_test.py --score data/work/ab/session_<...>.json
```

**Importing FM2 maps** (the only way to change scenery data; `--only maps`, `FM2_TRACKS` limits to one track):

```powershell
$iso = 'D:\Games\ROMs\Xbox 360\Forza Motorsport 2 (USA) (En,Ja,Fr,De,Es,It,Zh,Ko,Pl,Cs,Hu).iso'
$env:FM2_TRACKS='tsukuba'; .\target\release\fh1setup.exe import-fm2 "$iso" --only maps; Remove-Item Env:FM2_TRACKS
```

**Prop gate** (deterministic; ~20 s per capture plus startup):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File data\work\run_prop_final.ps1
python -I -X utf8 data\work\plan\prop_gate.py          # or score manually on the footprint
```

**HUD layers** (glyphs without the plate under them):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File data\work\run_hud_layers.ps1 -PoseLocked
python -I -X utf8 data\work\plan\hud_layers.py <all> <text> <plates> <none> <reference>
```

---

## 3. The instrument

| tool / switch | what it is | state |
|---|---|---|
| `data/work/parity.py` | twelve-track run: bands, percentiles, six patch ratios with an NCC flag, masked scene/sky comparators, road and car masks, two-shot car keeping | the steering instrument |
| `parity.py::masked_ratio` | luminance ratio over 32×32 blocks whose **NCC ≥ 0.7** | the scene number — **and its threshold matters (§8.2)** |
| `data/work/ab_test.py` | blind A/B by region, writes a session JSON | **the verdict** |
| `data/work/plan/detail.py` | mean \|Laplacian\| on linear luminance, y 45–80 % | the step-2 gate |
| `data/work/plan/detail_masked.py` | the same per NCC-matched block and by row band | localises a detail deficit |
| `data/work/plan/hud_layers.py` + `run_hud_layers.ps1` | per-element HUD, glyphs separate from plates | step 4 |
| `data/work/plan/prop_gate.py`, `run_prop_gate_locked.ps1`, `run_prop_final.ps1` | the prop gate | step 1 |
| `data/work/plan/prop_poses.py` | **writes prop-aimed camera poses** (and `--restore`) | ready; needs new references to score |
| `data/work/plan/road_compare.py` | per-track road-mask comparison between two runs | works |
| `crates/fh1setup/examples/fm2_ps_gpu.rs` | draw-level oracle (`--scan-models`, `--render`, `--dump-verts`) | the workhorse |
| `crates/fh1setup/examples/sky_bake.rs` | applies the sky alpha term at a chosen `w`, with `--save`/`--from` | verified |

**Diagnostic switches added this pass (all default off unless stated):**

| switch | what it does |
|---|---|
| `FH1_SHOT_LOCK=1` | locks the capture camera to the recorded reference pose **without** the readiness gate (65 s vs ~10 min) |
| `FH1_PROP_HIDE=1` | hides placed prop tiles after streaming — the clean presence test |
| `FH1_PROP_ORACLE=1` | draws one template three ways (production material, two-sided magenta, back-face cyan) in front of the camera |
| `FH1_PROP_COMPLETE=1` | one log line per placed prop tile: placements, parts, merged, skipped |
| `FH1_PROP_OFFSET=x,y,z`, `FH1_PROP_SCALE=s` | moves / resizes every prop placement — the world-space sweep |
| `FH1_PROP_UNLIT=1` | props drawn unlit (texture only, no sun) |
| `FH1_PROP_SIMPLE` | prop material path; **default on** (standard material), `=0` restores the remaster record |
| `FH1_TEX_ANISO=n` | scenery sampler anisotropy (default 8, the game's own) |
| `FH1_RM_MIP_BIAS=f` | sampler mip bias; **default 0.35** (shipped, step 2) |
| `FH1_RM_MATID=1` | paints each surface class flat (opaque red, cutout green, decal blue, water cyan, unlit yellow, additive magenta) |
| `FH1_FM2_HUD_LAYER=text\|models` | draws only that layer of the FM2 HUD |
| `FH1_FM2_HUD=none` | draws no HUD at all (both FH1's and FM2's) |
| `FH1_FM2_HUD_PANEL=f` | dark-panel coverage multiplier (default 0.85) |
| `FH1_FM2_HUD_TEXT_GAMMA=1` | gamma-space glyph coverage (tested, no effect) |
| `FM2_MASK_UV=colour\|stretched\|ratio=<k>\|mat=<ids>\|family=<substr>` | importer: which blend draws get the colour's uv instead of their resolved one |
| `FM2_BLEND_DBG=1` | importer: one line per blend draw with uv0 / mask / lightmap ranges, material and shader |

---

## 4. Current numbers (shipped build, twelve tracks)

**Near-field detail (the step-2 gate), `detail.py`:**

| track | detail | track | detail |
|---|---|---|---|
| Road Atlanta | **1.18** | Tsukuba | 0.91 |
| Nordschleife | **1.13** | Mugello | **0.85–0.86** |
| New York | **1.11** | Sunset Infield | 0.84 |
| Suzuka | **1.08** | TT King Cobra | 0.70 |
| Maple Valley | **0.97** | Silverstone | 0.65 |
| Laguna Seca | **0.92** | Sebring | 0.52 |

**median 0.92; 8 of 12 at ≥ 0.85 on one run, 7 of 12 on the next.** The difference is **Mugello at 0.856 against 0.847** —
the gate sits on a one-hundredth boundary, so it is **met marginally, not comfortably**.

**Scene (masked comparator):** median **1.10** at NCC ≥ 0.7 (1.15 at NCC ≥ 0.3). Per track 0.87 (Nordschleife) to 1.57
(Maple Valley) at 0.7; **0.97 to 1.40 at 0.3 — no track is darker than parity** (§8.2).

**Road mask:** `sum |ln|` **5.02–5.04 over 11 tracks** (target ≤ 2.0) — unchanged by the step-2 fix.
**Car:** luminance **1.26**, mask `sum |ln|` **0.23** (Tsukuba — the only re-measured track whose car mask is reliable; the
seven reliable-mask tracks were 1.45 before the paint change).
**HUD:** plates **1.11 / 1.00 / 0.83 / 0.97** (RACE / LAPS / PLACE / tach); glyphs **1.01–1.18**.
**Sky band:** 1.07–1.23.
**Props:** 3 of 3 scorable tracks help (−9.19 / −1.29 / −0.06); 9 references have 0–297 px of props.

---

## 5. Step 1 — props: what was found, and what the gate can and cannot say

### 5.1 They were never missing

The fourth pass closed props as "the entity is not rasterised". **That was wrong.** The six tests it ran (material index
156→69, StandardMaterial, 10× scale, full vertex layout, transform, forced visibility) all change an object's *appearance*,
each moving 0.13–0.23 % of the frame — which was read as absence. `FH1_PROP_ORACLE` and `FH1_PROP_HIDE` are the presence
tests: the template draws through the production material, as magenta and as cyan, and hiding the placed props removes a
real footprint.

### 5.2 The real defect was the material path

A prop is a separate model file (`co_*.bin`), so the importer must give its draws a material index by matching the texture
against the *scenery* materials — and the remaster record behind that index belongs to another draw, carrying that draw's
brightness. Props were therefore ~2.4× too bright. Moving them to the standard material took Nordschleife from **+15.81
(hurting) to −7.59 (helping)**; Laguna needed the opposite and is handled per track (a labelled calibration).

### 5.3 Placement was never wrong

`FH1_PROP_OFFSET` / `FH1_PROP_SCALE` swept ±2 m in x and z and scales 0.5/2.0 over the union of all their footprints
(27,031 px): **every configuration still increased the error**. The two "best" rows were coverage artefacts — smaller or
shifted props touch fewer pixels. There is no systematic translation or scale error to undo.

### 5.4 The gate

**Restated and recorded (MODLOG 554–555):** *on the tracks where props are in frame at the reference pose, the props
reduce the mean |d| on the pixels they cover.* **3 of 3: Nordschleife −9.19, Road Atlanta −1.29, Suzuka −0.06.**

The **"across twelve tracks" wording is abandoned**, with the reason recorded: nine references contain **0–297 px** of
props (Sunset's are 202 m from the camera and change 44 px), so the twelve-track statement is not something the props
could satisfy even with perfect rendering. Scoring those nine needs reference captures at prop-aimed poses —
`prop_poses.py` writes exactly those poses — which means driving Xenia, **outside this project folder**.

**Not claimed:** that the props are correct on the nine unscored tracks. They are unmeasured there, not verified.

### 5.5 Two corrections that matter for anyone re-reading the old record

- **Sunset's +4.72 was crowd noise.** Captured before the prop runners set `FH1_CROWD=0`, its 7,857 px "footprint" was
  crowd variance. With the crowd off, Sunset's props are 44 px at 202 m.
- **The prop gate's noise floor** is 66–369 px between identical runs (with the crowd off), so a delta measured on a
  footprint below ~1,000 px is not adjudicable — Road Atlanta's 370 px row sits exactly there.

---

## 6. Steps 2–4 — what was found

### 6.1 Step 2 — the detail deficit was mip selection

The deficit was **near-field only** (far field 1.06–1.08, near road 0.52), which is a mip-selection signature. The
fix is a **mip bias of 0.35** on the scenery sampler, baked into the generated WGSL helper:

| `FH1_RM_MIP_BIAS` | masked median | near band | road mask |
|---|---|---|---|
| 1.0 (old) | 0.79 | 0.52 | 0.73 |
| 0.7 | 0.86 | 0.65 | 0.73 |
| 0.5 | 0.89 | 0.80 | 0.73 |
| **0.35 (shipped)** | **0.92** | **1.00** | **0.73** |

Twelve tracks: **median 0.92, 8 of 12 ≥ 0.85** against 0.69 / 2 of 12. **The road mask is unchanged** — unlike the
alternative fix (moving the blend mask onto the tiled uv), which bought the same detail and cost the road mask
0.73 → 0.77, and which remains opt-in as `FM2_MASK_UV`.

The tiled-mask question was answered on the way: most blend draws sample their mask with the **lightmap's 0…1 uv** while
the colour tiles by tens to hundreds of units; the smallest key that reproduces the whole effect is **19 of 168 draws**
(`ratio<0.02`), and it carries the whole road-mask cost with it. No durable smaller subset exists — one shader family, 166
material indices, 109 mask textures, 96 per-track model ids.

### 6.2 Step 3 — the paint was a fitted number

The paint multiplier was a **fitted 0.85**; it is now FM2's own **`ColorScale` 0.6** from
`Media/cars/ShaderSettings/Normal.xml`. Car luminance **1.42 → 1.26**, `sum |ln|` **0.35 → 0.23**, every channel better.
Darker keeps scoring better (0.5 → 1.18), so the game's own value was used rather than the better-scoring fit.

`FH1_RM_CAR_FILL=0` gives 1.38 (so the hemisphere fill is not the excess) and `FH1_RM_CAR_SUN=0` gives 2.13 (worse, so the
game's direct-light ratio is doing real work and is already applied).

### 6.3 Step 4 — the HUD's plates were fitted against confounded captures

Per-element measurement needed three things that did not exist: a **layer toggle** (`FH1_FM2_HUD_LAYER`, so glyphs can be
measured without the tach plate inside the same rectangle), a **no-HUD capture** (`FH1_FM2_HUD=none`) and a
**deterministic race capture** (`FH1_RACE_AI=0` took the noise 9,892 → 1,739 px).

With those, the **glyphs were 1.01–1.18** of the reference — the "gauges 11–17 % dark" of the fourth pass was the *plate
underneath them*. The plates were **0.64–0.92** because their coverage constant (1.146) had been fitted against captures
whose scene did not match the reference. Re-derived from pose-locked data it is **0.85**, and the plates read
**1.11 / 1.00 / 0.83 / 0.97**. No frame regression.

---

## 7. Where the objective stands

| step | gate | state |
|---|---|---|
| 1 | props reduce mean \|d\| on the pixels they cover (scoped to tracks where props are in frame) | **met 3 of 3**; 9 unscorable |
| 2 | near-band detail ≥ 0.85 on ≥ 8 of 12 | **met at the boundary** (8 of 12 / 7 of 12 across runs) |
| 3 | car mask moves toward 1.0 | **met** — 1.42 → 1.26 |
| 4 | HUD element matches on pose-locked captures | **met** — plates 0.92/0.80/0.68/0.75 → 1.11/1.00/0.83/0.97 |

**One decision remains open (the user's):** step 2's gate is one hundredth from failing on Mugello. Either accept it as met,
or treat the ≥ 8-of-12 rule as too sharp for a metric with 0.01 of run-to-run noise and restate it (e.g. median ≥ 0.90).

---

## 8. Harness traps that cost time (read before measuring)

1. **`FH1_SHOT_READY=0` does not lock the camera.** `lock_shot_camera` needs the frozen state, so a gate-less capture is
   taken from the free-roam follow camera and cannot be compared to a reference. `FH1_SHOT_LOCK=1` locks it without the
   gate and is 10× faster (65 s vs ~10 min per track). **Use it for every measurement.**
2. **The NCC threshold moves the scene number.** The same screenshots give a spread of 1.81× at NCC ≥ 0.7, 1.58× at 0.5 and
   1.45× at 0.3, and Nordschleife reads 0.87 / 1.04 / 1.13. The strict gate selects the best-correlating blocks, which on
   dark tracks are the dark ones. **A dark-track result at NCC 0.7 is not a brightness finding.**
3. **The crowd is the capture noise floor** unless `FH1_CROWD=0`. Before the `shot_pin` fix, two identical runs differed by
   9,893 px (1.4 %); with it, 5–111 px; with the crowd off, 66–369 px. Any prop or HUD measurement needs the crowd off.
4. **Prop streaming is timing-dependent.** At `FH1_SHOT_AT=8` the placement tiles sometimes never place (the log shows
   `N templates ready` and no `first tile`). Use `FH1_SHOT_AT=20` and check for `props: child` lines before trusting a
   prop footprint.
5. **A big jump from a parameter that does nothing when varied is a broken path.** The first mip-bias run produced the best
   road mask of the campaign (0.34) because the shader failed to compile (`no definition in scope for RM_MIP_BIAS`) and the
   material was skipped. **Check the compile log before believing a jump.**
6. **A fixed rectangle is not the surface it names.** Five of six patch boxes fail the content check (median NCC: sky 0.17,
   verge 0.20, wall 0.44, car 0.34, road 0.50); only **structures** (0.66) is usable, and it reads a uniform ~2× on every
   track. HUD rectangles needed control rectangles to prove they were measuring the scene.
7. **`detail.py` had the wrong reference filenames for six tracks** and silently scored 6 of 12 (fixed in this pass). If a
   "twelve-track" run reports six, check the table.
8. **A band total is a steering number.** Two runs of the same binary differ, and a carless shot flatters it.
9. **`import-fm2`'s group is `maps`, not `scenery`.** `--only scenery` silently does nothing on the FM2 path.
10. **A map re-import re-randomises the crowd**, so a map-before/after diff is never clean.
11. **The importer rewrites installed textures in place**; `sky_bake --save <dir>` keeps pristine copies.
12. **The existing Tsukuba constants dumps contain no prop draws** — 5,999 / 2,999 / 3,999 / 2,999 draws, and none has a
    vertex-constant triple matching any of the 36 prop placements. They cannot supply prop ground truth.
13. **`docs/DESIGN.md` and `docs/PLAN.md`, `data/work/**` are not tracked by git.** Copy before scripting edits.
14. **The blind test must run on a prefix that reflects the shipped build.**

---

## 9. Repository state (2026-11-13)

```
23 modified, 5 untracked, 0 staged. Nothing committed since b0fcb9d.
  M crates/fh1-engine/src/{crowd.rs, crowd/figures_gpu.rs, flag_docs.tsv, fm2_sky.rs, main.rs,
                            scenery.rs, track.rs, ui/fm2_hud.rs}
  M crates/fh1-formats/src/fm2.rs
  M crates/fh1-remaster/src/{car.rs, light.rs, material.rs, post.rs, scenery.rs}
  M crates/fh1-render/src/scenery.rs
  M crates/fh1setup/{src/{fm2_scenery,fm2_sky,fm2_uv,main}.rs, examples/fm2_ps_gpu.rs}
  M MODLOG.md, README.md, STATUS.md
  ?? .opencode/, crates/fh1setup/src/fm2_props.rs (the props importer, new),
     crates/fh1setup/examples/{sky_bake,texhash}.rs, tools/night_run.ps1
```

**Not in git at all:** `docs/{DESIGN,GOALS,PLAN,PLAN-0*}.md`, `data/work/**`, `tools/xenia-test/**`,
`data/installations/**` (the imported assets).

**Data on disk (git-ignored):** the four Tsukuba constant dumps (83 / 37 / 55 / 34 MB), `p14_texdump/` (962 game
textures), and this pass's measurement records under `data/work/plan/` (`hud_tsukuba.csv`, `pose_backup/`, the scripts
named in §3).

---

## 10. Do not retry (measured and closed)

| attempt | result |
|---|---|
| **props are not rasterised** | false — they draw; the six tests measured appearance (MODLOG 505/523) |
| **a global gain on the scene** | cannot narrow a multiplicative spread by construction |
| **`FH1_RM_LM_SRGB=1` globally** | 5 tracks better, 7 worse; it is a monotone darkening |
| **`FH1_RM_LM_CODE=1`** | 0.93 on Sunset but the sky goes to 1.38 |
| **`FH1_RM_LM_LIGHT_GAIN`** | inert (2.5 → exactly 1.57 / 0.87) |
| **`FH1_RM_LM_LIGHT_EXPOSURE=0.4` globally** | spread 1.81× → 1.75×, New York 0.61, road mask 5.02 → 6.87 |
| **`FH1_RM_LM_LIGHT_EXPOSURE=0.6` globally** | detail gate 0.92/8-of-12 → 0.73/0-of-12, road mask 6.52 |
| **a per-track exposure for Sunset** | shipped, then reverted — its justification (the +4.72) was crowd noise (MODLOG 554) |
| **`FH1_PROP_UNLIT=1`** | fixes Laguna (+12.18 → +0.03), breaks Nordschleife (−4.40 → +47.99) |
| **prop offsets / scales** | no configuration reduces the error (MODLOG 522) |
| **the tiled blend mask, globally** | detail gain, road mask 0.73 → 0.77 |
| **`ratio<0.02` (19 draws)** | identical to all 168 — same gain, same cost |
| **model 60's mask uv alone** | no effect on the detail metric |
| **a material-class discriminator for props** | class, shader and the brightness params all overlap between the tracks that need opposite paths |
| **the lightmap dark/light pair as the spread's cause** | correlation −0.28 |
| **the game's scene luminance as the cause** | correlation −0.32 |
| **fixed HUD rectangles** | confounded by the scene; control rectangles track their element |
| **gamma-space glyph coverage** | 1.06 → 1.04, no effect |
| **the 0.12 lightmap floor, the post's dark/light blend, Bevy sun/env/fog, A2C, 4× MSAA** | all closed in the fourth pass (MODLOG 468–502) |

---

## 11. Honesty: verified, inferred, untested

**Verified by measurement:**
- The mip-bias fix: 12-track median **0.92**, **8 of 12 ≥ 0.85**, road mask **5.02 → 5.04**, **0 shader errors**.
- The paint change: car **1.42 → 1.26**, `sum |ln|` **0.35 → 0.23**.
- The HUD plates: **1.11 / 1.00 / 0.83 / 0.97** on pose-locked captures, glyphs 1.01–1.18.
- The prop material: Nordschleife **+15.81 → −7.59…−9.19**; the gate **3 of 3** on scorable tracks.
- The prop dedupe: 50→36, 556→196, 578→139, 196→61, 496→203, 80→42, 38→21 (exact, from the tile files).
- The crowd determinism fix: 9,893 → 5–111 px between identical runs.
- The NCC artefact: spread 1.81× → 1.45× and Nordschleife 0.87 → 1.13 as the gate loosens.
- Sunset's props are 202 m from its reference camera (44 px).
- The constants dumps contain no prop draws.

**Inferred, not proven:**
- That a per-track exposure would fix Sunset without other cost (measured only in the global form, then reverted).
- That the remaining detail shortfall on Sebring (0.52) and Silverstone (0.65) is content rather than sampling.
- That structures being uniformly ~2× is one material-path defect rather than per-surface data.

**Untested:**
- Any track beyond the four with props measured for the prop gate.
- The nine reference poses for props (no props in frame; needs Xenia references at the `prop_poses.py` poses).
- FM3, FM4, FH1 and FH2 (360): **no reference sets exist**, so G6 has no measurement for four of its five games.
- Whether the blind A/B moves at all with the shipped build — the last verdict was 29/30 before steps 2–4 landed.
