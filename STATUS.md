# STATUS, 2026-11-02 (second revision): goals recorded, the PI display solved, visual parity still the open problem

> **This replaces the previous `STATUS.md`** (the 2026-11-02 "measurement rebuild" handoff). That text is preserved in git history: `git show 4f30844:STATUS.md`. Everything still true from it is carried forward or pointed at here; its detailed FM2 tables and its tooling sections are **not** reproduced in full, so read them there.
>
> Since that document: the goals were written down (`docs/GOALS.md`, `docs/ATTACHMENT.md`), a new priority-1 goal was added (**G6, visual parity with every 360-era game**), the per-car **PI display** question was answered from the game files and **fixed in code**, the **part inventories** of four more games were read, and FM2's cars were **re-imported** with the fix. No rendering change was made. MODLOG 246-258.

---

## 1. The short version

**FM2 visual parity is not met and did not move this session.** The judged number is **6.04** against a target of **2.0**, with every track off. The previous STATUS retired the older 5.08 figure as unreproducible; the replacement instrument stands, and the honest position in it still stands: every cause is named with numbers, and no change inside the plan has an evidence-backed mechanism. The decision left to the user (`STATUS.md` §12 in the old document) is unchanged and now sits inside goal **G6** as the thing blocking it.

**What did move:** the goals now exist as a document with gates and a priority order; the PI display is solved, fixed and verified end to end; the model part inventories are known for FH1, FM2, FM3, FM4 and FM6, which unblocks customisation (G4).

**The project's own game (FH1) is unaffected:** full Colorado map, 175 cars, races, garage, remaster renderer. Only regression-tested lately.

---

## 2. Priority order (the goals, in work order)

From `docs/GOALS.md`, which is the detailed version. Ids are stable, the order is not.

| # | id | goal | state |
|---|---|---|---|
| 1 | **G6** | Visual parity with every 360-era game (FM2, FM3, FM4, FH1, FH2-360) | `in progress` — FM2 measured at 6.04 / 2.0; **blocked on a decision** |
| 2 | **G1** | Completely modular game features (look, HUD, physics, audio, rules …) | `not started`; the look slot is what G6 needs, so the two interleave |
| 3 | **S6** | PI and car data sources | `in progress` — display solved and fixed; backend rating not built |
| 4 | **G2** | Complete car and track list, three stages | `not started` |
| 5 | **G3** | Best model per car | `not started` |
| 6 | **G4** | Enhanced customisation | `not started` — **unblocked** this session |
| 7 | **G5** | Optimisation (720p60 handheld, storage) | `not started` |

---

## 3. G6 — visual parity (priority 1)

**Gate:** per game, aggregate **sum |ln| ≤ 2.0** and **every track ≤ 0.10**, against reference pictures frozen before the work.

**Games in scope:** FM2, FM3, FM4, FH1, FH2 (Xbox 360). FM1 is original-Xbox and out of scope.

### 3.1 The FM2 numbers, unchanged this session

`python -I data/work/acceptance.py <prefix>` (~8 min, twelve tracks), frozen references in `screenshots/xenia_ref/g1_*.png`, pixels proven asphalt by the engine's own collision ray, frozen in `data/work/anchor_pixels.json`, binned by the reference's brightness.

| track | median | bins dark/mid/bright | err |
|---|---|---|---|
| Tsukuba | 1.28 | 2.36 / 1.33 / 1.00 | 0.38 |
| Laguna Seca | 1.57 | 4.62 / 1.25 / 1.07 | 0.61 |
| Road Atlanta | 1.39 | 1.48 / 1.69 / 1.19 | 0.36 |
| Silverstone GP | 0.60 | 3.89 / 0.62 / 0.46 | 0.87 |
| Nordschleife | 0.77 | 1.38 / 0.75 / 0.38 | 0.52 |
| Sebring | 1.21 | 1.73 / 1.33 / 0.69 | 0.41 |
| Mugello | 0.86 | 1.07 / 0.87 / 0.76 | 0.16 |
| Suzuka | 0.60 | 1.25 / 0.52 / 0.53 | 0.50 |
| Sunset Infield | 0.53 | 0.74 / 0.54 / 0.34 | 0.67 |
| Maple Valley | 2.15 | 3.25 / 2.15 / 0.48 | 0.89 |
| New York | 0.48 | 1.00 / 0.74 / 0.12 | 0.80 |
| TT King Cobra | 3.49 | 5.91 / 1.06 / 0.79 | 0.69 |

**sum |ln(median)| = 6.04, sum of bin errors = 6.86.** Repeatability: per-track bins reproduce exactly; **the aggregate drifts ±0.5 with unchanged code**, so only same-build per-track A/B comparisons support a claim.

### 3.2 What that measurement established (do not re-derive)

- The road draws are named on all twelve tracks and **the game's own shader** can be evaluated at our exact pixels.
- Our pre-post scene is **short of light** on nine of twelve tracks (0.40-0.87 of the reference). FM2's own tone-curve blend, pinned at its dark end (t ≈ 0.02) *because* of that shortfall, then lifts the shadows and compresses the highlights. That is the visible signature.
- Making the road **albedo more faithful makes it worse** (tested twice, both buckets, reverted).
- The **shadow machinery is not the lever** (six settings tested: distance, caster cull, decal colour, gate, floor, sun direction).

### 3.3 What is blocking it

`STATUS.md` §12 (old document, git history) offers three options: **accept** the measured state, adopt a **labelled calibration** (a per-track gain fitted to the reference — very likely reaches ≤ 2.0, but it is drawing the target, not reproducing the game), or **park** FM2 road parity.

**G6 records two rules about this:**

1. A fitted gain is a **calibration, not parity**. It must be labelled in the UI and the code, and it does not satisfy the gate.
2. Parity is judged on the **Remaster renderer at shipped default/high quality**, not on a low-end tier.

### 3.4 Not started for four of the five games

FM3, FM4, FH1 and FH2 (360) have **no frozen references and no acceptance run at all**. Each begins by building the harness FM2 already has. That is the largest concrete piece of work inside G6.

---

## 4. S6 — the PI display (solved and fixed this session)

### 4.1 The question, answered from the files

**A car's PI is stored data, not computed at load.** Every game keeps `Data_Car.PerformanceIndex` (normalised 0..1, non-null on every row) and the class ladder in `CarClasses` (`MaxPerformanceIndex`, `MaxDisplayPerformanceIndex`, class letter). Cars with a stored PI: FM2 310, FM3 415, FM4 503, FH2 (360) 216.

The ladders differ per game, which is why the ladder belongs to G1's menu-UI slot:

| game | ladder (letter: max display PI) |
|---|---|
| FM2 | D:400 C:550 B:700 A:850 S:998 U/R4/R3/R2/R1:999 |
| FM3 | F:200 E:275 D:350 C:425 B:500 A:600 S:700 R3:800 R2:900 R1:998 U:999 |
| FM4 | F:200 E:275 D:350 C:425 B:500 A:600 S:700 R3:800 R2:875 R1:998 U:999 |
| FH2 (360) | D:500 C:600 B:700 A:800 S1:900 S2:998 X:999 |

### 4.2 The conversion rule, from eleven in-game readings

```
display = prev + floor(f * span) + 1
prev  = previous class's MaxDisplayPerformanceIndex   (99 for the first class)
span  = class max display - prev
f     = (pi - class min) / (class max - class min)
```

The scale bottoms at **100**. One constant for every game. **Eleven numeric readings across FM2, FM3 and FM4, all exact, plus the letter-only FM2 race badge:**

| car | game | in-game |
|---|---|---|
| 2002 Skyline GT-R V-Spec II | FM2 | B 584 |
| Honda Civic VTi | FM2 | D 211 |
| 2004 Audi TT 3.2 quattro | FM2 | D 364 |
| 2003 #12 Calsonic Skyline | FM2 | **R3, no number** |
| 2002 Skyline GT-R V-Spec II | FM3 | C 401 |
| Scion xD | FM3 | F 175 |
| 2009 Citroen C4 VTS | FM3 | F 174 |
| 2003 #12 Calsonic Skyline | FM3 | R2 825 |
| 2002 Skyline GT-R V-Spec II | FM4 | A 503 |
| 2003 #12 Calsonic Skyline | FM4 | R2 858 |
| 2011 Ford Ka / 2011 Toyota Aygo | FM4 | F **100** (bottom of the scale) |

**Race classes are a per-game display difference:** FM2 shows the letter with no number (buy screen *and* race results), FM3 and FM4 show letter and number. Same car, three answers.

**FM2's race classes cannot be separated by PI** — `R4/R3/R2/R1` all store normalised max 1.0 and display max 999. The letter must come from the stored `ClassID`.

### 4.3 What changed in code (MODLOG 255-256)

- `crates/fh1setup/src/cars.rs` — one shared `display_pi(bands, pi)` replaces two identical copies; clamps to the class maximum so a number can never leave its class.
- `crates/fh1setup/src/fm2_cars.rs` — writes **no `pi`** for FM2's `R4/R3/R2/R1`.
- `crates/fh1_engine`/`race/postrace.rs` — leaves the number field empty when `pi` is 0, so the class letter still shows.
- **Gated** by three unit tests, including the eleven numeric readings: `cargo test -p fh1setup --bin fh1setup` → 16 passed.

**Verified in the shipped artefact** (not just the tests): FM2's cars were re-imported (`310 cars imported, 0 skipped`) and the produced `imported/fm2/cars/index.json` carries **B 584, D 211, D 364, R3 with `pi: null`**, and **74 cars with no number, classes exactly R1-R4**.

**Roster effect** (same rule over every stored PI): FM2 150 cars +1 / 159 unchanged / 1 −1; FM3 198 / 216 / 1; FM4 254 / 246 / 3. The five downward moves are all at the very bottom of the first class, and the in-game reading (Ford Ka = F 100) confirms 100 is right.

### 4.4 Not done in S6

- **FM3/FM4/FM6 were not re-imported**, so their installed numbers are still the old (one-point-low) ones.
- Our own **results screen has not been seen** with an FM2 race car; the postrace change is compile-checked only.
- **The backend rating** (one measured number per car, translated per game) is not built. The open question is whether stored per-game PI wins where the game has it or maps-only is used; the measurement that informs it exists — cross-game rank agreement is only **93.3 %-96.3 %**, so one monotone number cannot be exact, and the wrong-class count will not be zero.
- No re-import was run for FM3/FM4/FM6, and no in-game reading exists for **upgraded** cars (the known gap: a tuned car's PI needs each game's per-part contribution model).

---

## 5. Part inventories (C5) — read, and G4 unblocked

Verified from real imported models and from each disc's carbin sections (`cargo run -p fh1setup --example part_names -- <disc> <car>`):

| game | parts | notable |
|---|---|---|
| FH1 | 50 nodes | four wheel nodes, separate calipers and rotors |
| FM2 | 37-42 nodes | same canonical stems; **no calipers/rotors**; `exhausta/b/d` |
| FM3 / FM4 | 61 sections each, **identical sets** | kit variants `a`-`f` **and** `race`; **one `wheel`** section, not four; no calipers |
| FM6 | 162 nodes | doors, door cards, jambs, sill, roof, fenders, floor, **`enginebay`**, dash, gauges, pedals, trunk liner, roll cage, calipers/rotors, rim + tyre |

**Three consequences recorded in `docs/ATTACHMENT.md`:** FM6 has a real `enginebay` node; wheel layout differs three ways, so wheel-swap code must handle all three shapes; caliper colour is FH1/FM6 only.

**Still unread:** FM5 and FH2 (360); FM6 from only one car (`FAL_Atom_77`), so "not seen" there is not "absent".

---

## 6. Proven facts to not re-derive

| fact | evidence |
|---|---|
| FM2's textures are exactly the disc's (BC4 byte-identical, BC1 value-identical); no gamma decode | MODLOG 181, 219, 175, 179 |
| The colour-space order is irrelevant | MODLOG 217 |
| Material/remaster tables must be rebuilt after every import or materials keep stale flags | MODLOG 196 |
| The road draw under the car, all twelve tracks | MODLOG 226 |
| The game's own shader can be evaluated at our exact road points | MODLOG 230, 234 |
| Our pose matches the reference (gradient NCC peak at 0,0 on eight tracks, (0,-2)/(0,-4) on four) | MODLOG 225 |
| PI is stored, the ladders are stored, and the display conversion is the rule in §4.2 | MODLOG 247-256 |
| Model parts are separate in FH1, FM2, FM3, FM4 and FM6 | MODLOG 256 |

## 7. Rejected — do not repeat

From the previous STATUS, still valid: the draw's own shader albedo (W1), gating the sun term on the lightmap (W2.3), the game's own `DropShadowColor` for the car decal, lightmap light floor 0, shadow distance 600 / caster cull off, a global sun-direction flip, a per-track sun-direction fix, per-track fitted tone tables, and ~15 earlier hypotheses (see MODLOG ≤ 222).

On the PI side: **do not** re-derive the conversion from first principles again — eleven in-game readings pin it, and three successive attempts to reason it out from the data alone were wrong by one point each (MODLOG 247-251).

---

## 8. Open decisions

1. **FM2 visual parity** (blocks G6): accept / labelled calibration / park.
2. **PI mapping** (Q2): stored per-game PI wins where the game has it, or maps only. The rank-agreement measurement says the wrong-class count will not be zero.
3. **Q3** 360 downgrade (user unsure); **Q4** engine-audio as a slot or a parameter; **Q5** track props; **Q6** wheel clipping; **Q7** unrateable cars; **Q8** upgrade ceiling; **Q9** spec-source scope; **Q10** storage gate; **Q11** attachment point list.
4. **`README.md` advertises online free roam** while `AGENTS.md` rule 5 says single-player/offline only. Pre-existing, untouched, needs the user's call.
5. **`docs/` is git-ignored**, so `GOALS.md` and `ATTACHMENT.md` exist on this machine only. Only the README table and MODLOG survive a clone.

---

## 9. Not tested

- **FM2 visual parity:** nothing changed; the number is 6.04.
- **FM3/FM4/FH1/FH2 (360) visual parity:** no references, no measurement at all.
- **Our engine's results screen** with an FM2 race car: compile-checked only.
- **The PI fix in FM3/FM4/FM6 installs:** code fixed, those games not re-imported.
- **Upgraded-car PI:** no model, no measurement.
- **Bevy on Android/Adreno:** not tested; G5 leans on it.
- **Dedupe and compression ratios** on our own content: not measured.
- **Frame cost** beyond one HUD reading per circuit: not measured.
- **A full lap or race on FM2 scenery by a person**; the emulator capture route with the virtual pad (installed and proven, never used to drive Xenia).

---

## 10. Reproducing the checks

```powershell
# FM2 visual parity (twelve tracks, ~8 min)
python -I data/work/acceptance.py <prefix>

# the PI conversion against the eleven in-game readings
cargo test -p fh1setup --bin fh1setup

# the produced FM2 car index (after a re-import)
.\target\debug\fh1setup.exe import-fm2 data\work_fm2\disc --only cars
python -I data/work/pi_verify.py

# a car's part inventory, straight from a disc
cargo run -p fh1setup --example part_names -- data/work_fm3_disc1/disc NIS_SkylineGTRVSpecII_02

# the PI ladder and rank-agreement measurements
python -I data/work/pi_bands.py
python -I data/work/pi_rank.py
```

After any import: `.\target\debug\fh1setup.exe "data/work/disc" --only remaster --force` (the stale-table trap in §6).

---

## 11. Where things are

| path | what | tracked |
|---|---|---|
| `docs/GOALS.md` | the goals: G1-G6, S6/S7, conflicts, questions, priority order | no (git-ignored) |
| `docs/ATTACHMENT.md` | G4.1 attachment schema + per-game part inventories | no |
| `docs/DESIGN.md` | the older plan log (v6, v7); **not yet merged with the G1 slot list** | no |
| `README.md` | goals table with the work order, and "what works / what doesn't" | **yes** |
| `MODLOG.md` | 246-258 cover this session | **yes** |
| `data/work/*.py` | acceptance, probes, PI scripts, comparison sheets | no |
| `crates/fh1setup/examples/part_names.rs` | part inventory dumper (new) | **untracked** |

**Working tree state:** `MODLOG.md`, `README.md`, `crates/fh1setup/src/{cars,fm2_cars,fm4}.rs`, `crates/fh1-engine/src/race/postrace.rs` modified; `part_names.rs` new. **Nothing committed, nothing staged.**
