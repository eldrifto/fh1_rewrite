# MODLOG

Running log of what changed and what was tested. Newest first. "Tested" means I ran it and looked at a screenshot or the importer's output. I cannot see the game live, so every visual check is a screenshot of the start line (or the spot named). Where I did not check something, it says "not tested".

Written 2026-10-17, after the fact, from the session's commits and my notes. Entries before 2026-10-16 are summarised from memory of the earlier part of this session, so their detail is thinner.

Importer changes (`crates/fh1setup/...`) were made in the sibling worktree `D:\Projects\slop\fh1_rewrite_fm3` on branch `fm2-lightmaps`. Engine changes (`crates/fh1-engine`, `crates/fh1-remaster`) are in this folder on branch `ui-elements`.

Not committed right now (uncommitted in the fm3 worktree): the Suzuka road gain, the wear-invert rule and the matching survey/debug knobs. See the first three entries.

---

## 2026-10-17 Gamma-space lightmap product tried on all 12 maps (`FH1_RM_FM2_GAMMA_LM=1`, not adopted, UNCOMMITTED knob in `material.rs`)

**Changed:** nothing new; the engine knob added earlier was run on every map.
**Why:** Sunset Infield's road is 0.43x Xenia; its texture (mean 92) times the game's lightmap blend in display space (about 1.1) gives about 101, close to Xenia's 104, while the engine's linear product gives 74.
**Tested how:** screenshots of all 12 maps with the knob on, road-band ratio against Xenia (ours / Xenia, linear).
**Result:** closer on 3 of 12, farther on the rest: Sunset Infield 0.43 to 0.66, New York 0.41 to 0.93 (road colour 92,61,49 against Xenia 95,63,51), Tsukuba 1.09 to 0.99; but Silverstone 1.56 to 2.37, Road Atlanta 1.38 to 0.52, Laguna Seca 1.36 to 2.27, Mugello 0.56 to 0.37, Test Track 0.66 to 0.54, Suzuka 2.06 to 2.60, Nurburgring 1.58 to 1.84.
**Still broken / not tested:** no single global colour-space rule fits all maps. The maps it helps (New York, Sunset Infield, Tsukuba) are the ones with low light colours (about 1.2 or less); the ones it hurts mostly have larger light colours (Laguna Seca 1.47, Silverstone 1.37), so the lightmap light/dark colours may be applied differently from the engine's lerp. Not tested.
**Next:** check how the game applies `DarkColor` / `LightColor` (the shader's `c184` / `c185` lerp) against the engine's `lm * (light - dark) + dark`.

---

## 2026-10-17 Sunset Infield start-line chequer (cut-out decal) (UNCOMMITTED)

**Changed:** `fm2_scenery.rs` (wear-blend pass): a wear-blend draw whose base colour is a cut-out texture (in `keyed_ids`) and whose shader alpha is not constant is drawn as a plain cut-out card (`h_diff_opac_1_lm`), not as a wear blend. `FM2_NO_DECAL_CUTOUT=1` turns it off.
**Why:** Sunset Infield's start line (model 36, a 2-triangle decal with texture 96: grey squares opaque, the others transparent) came out as a solid pale band; Xenia shows a black-and-white chequer.
**Tested how:** `FM2_UNDER` at the start; screenshot of the spawn view; all 12 maps re-imported and shot; road-band ratios against Xenia and whole-frame differences compared with a noise baseline (two runs with no intended change differ by 0.3 to 4.8 per pixel from road grain and animation).
**Result:** chequer drawn across the road. Road-band ratios unchanged on all maps (Sunset Infield 0.44 to 0.43, Laguna Seca 1.29 to 1.36, Suzuka 2.00 to 2.06, others within 0.05). New York's frame difference was above its baseline (7.4 against 3.7), but side by side the two shots look the same.
**Still broken / not tested:** Sunset Infield's road is still dark brown (0.43x Xenia): its base texture (252) is a mid-dark gravel and I have not looked at its lightmap or colours. The tyre-streak darkening on that card is dropped (the decal has none in Xenia).

---

## 2026-10-17 Test Track road fixed (placeholder textures) and Laguna Seca road lightened (UNCOMMITTED)

**Changed:** `fm2_scenery.rs`: (1) runtime textures (PVS flags 1) of at most 32x32 that are not BC4 masks are no longer used as colour layers (`placeholder_ids`, 34 across the maps; `FM2_KEEP_PLACEHOLDER=1` restores); (2) the wear-invert rule now applies to any draw with a real colour texture and a bright (> 140) wear mask, not only tiled ones; (3) debug `FM2_UNDER=x,z,y` lists the draws under a point with their material.
**Why:** Test Track's road draw (model 122) listed texture 24, a 1x1 black placeholder, as its base colour; the wear step could not make a darkened copy of it, so the second layer fell back to white and the road came out pale. Laguna Seca's road (model 168) is grey asphalt with a bright wear mask, so the old order darkened almost the whole road.
**Tested how:** `FM2_UNDER` at start-line spots, texture views, then all 12 maps re-imported and a median of two bare-road bands compared with Xenia (ours / Xenia, linear).
**Result:** Test Track 2.58 to 0.67; Laguna Seca 0.27 to 1.29; the other ten maps unchanged. Both numbers are from road bands only.
**Still broken / not tested:** Silverstone has a large flat black patch left of the white line (Xenia: grey tarmac). Turning wear off, and turning the baked-road texture off, left it unchanged, so it is neither; the draws probed there (models 37 and 97) are a baked-road blend and a grass draw, so the patch is probably a lightmap or an unprobed draw. Two attempts, so recorded here, not chased further. Laguna Seca at 1.29 is now a little bright.

---

## 2026-10-17 Road colour from the real pixel shader (opt-in `FM2_SHADER_ROAD=1`, UNCOMMITTED)

**Changed:** new `crates/fh1-shaders/src/ps_cpu.rs` (CPU interpreter for FM2 pixel shaders, ALU semantics from `wgsl.rs`, plus `fog_register`); `fm2_uv.rs` hands each draw's pixel shader (`PsRun`: microcode, literals, item to sampler and sampler to interpolator maps) to the importer; `fm2_scenery.rs` `shader_albedo()` runs it on up to 24 vertices with neutral lighting (lightmap blend = 1, sun = 0, fog off) and, only when `FM2_SHADER_ROAD=1`, bakes that mean colour for baked roads instead of the `c1` heuristics. Probe: `crates/fh1setup/examples/fm2_ps_run.rs`. Plan added to `docs/DESIGN.md`.
**Why:** road brightness against the Xenia shots differed by track in both directions (Test Track and Nurburgring about 2.5x too bright; Laguna Seca, Sunset Infield, Mugello about 0.4x).
**Tested how:** sweeps of the interpreter's inputs (output is exactly proportional to the lightmap blend, independent of ambient; the sun term adds on top), then re-import of all 12 maps and a median of two bare-road bands (left and right of the car) compared with Xenia (ours / Xenia, linear):
- before to after: Tsukuba 1.09 to 2.17 (worse), Silverstone 1.55 to 2.45 (worse), Mugello 0.56 to 2.16 (worse), Suzuka 2.00 to 1.06 (better), Maple Valley 4.25 to 7.11 (worse); Test Track 2.58, Nurburgring 1.58, Sunset Infield 0.44, Sebring 0.94, Road Atlanta 1.38, New York 0.41, Laguna Seca 0.27 unchanged.
**Result:** not adopted as default. Test Track's and Nurburgring's road colour does not come from the baked-road branch at all (unchanged after the switch); Test Track's blends use a black 1x1 placeholder (texture 24) as their first colour, so the pale road comes from elsewhere. Laguna Seca's 921 `c0` = 0 road draws evaluate to black in the interpreter (probably overlay passes), so they fall back to the old path.
**Still broken / not tested:** which draws paint the Test Track and Nurburgring roads under the car; Xenia's roads show much more detail (tyre marks, patches) than our flat roads, so some of the "brightness" gap may be contrast and detail.
**Next:** find the draws under the car on Test Track by hiding draws one at a time; look at contrast and detail.

---

## 2026-10-17 Test Track pale road: cause found, not fixed

**Changed:** nothing in code. Test textures were painted solid colours in the installed data and restored from a backup (`data/work/tt_tex_backup`).
**Why:** Xenia's Test Track road is mid-dark grey asphalt; ours is pale beige.
**Tested how:** painted every texture a solid colour and shot the start line; painted a handful of candidates (43, 45, 46, 51, 53) separately; read the pixel shader of model 16 (`fm2_show`).
**Result:** the road is not drawn from a colour texture. Painting texture 53 green turned the whole road green: 53 is the draw's baked lightmap tile, so the road is "baked road colour x a bright lightmap". The road draws (models 16, 43, 516, 546...) list only BC4 one-channel textures and carry pixel-shader constants `c1` = 1.2, `c2` = 0.1, `c3` = 0.1, `c4` = 0.3, `c5` = 2.0 (other road draws: `c1` = -0.05, `c3` = 0.6, `c5` = 2.5). The shader fetches three of those textures with `.xyz` and blends them with the constants. The importer treats `c1` as the road's colour, so 1.2 gives near-white.
**Still broken / not tested:** nothing fixed. A correct fix means reading how the three layers and `c0..c5` combine in that shader (the Suzuka `c3` gain was a partial version of the same idea). Texture 46 turned out to be the yellow line, not the road.
**Next:** decode the 3-layer road shader (or add a per-map road colour as a stopgap).

### Follow-up: first decode attempt (nothing changed in code)

Read the full pixel shader of model 16, group 2 (the road family with `c1` = 1.2). It fetches `tf4`, `tf5`, `tf6` (colour, `.xyz`), `tf3` (a mask) and `tf2`, then builds three palette terms from one sample `v = c0 - tf6.rgb`: `v*c5`, `v*c4 + c3`, `v*c2`, and `v*c1 - v*c2`; the `tf5` luma (weights 0.114 / 0.299 / 0.587) picks between them, and the result is multiplied by the sun and ambient terms. So `c1..c5` are palette gains and offsets, NOT a road colour: the importer's `c1`-as-colour reading is wrong for this family. Not resolved: `c0` is also multiplied into the sun term (line 64) and is 0 on some draws and 1 on others, so what each constant means per draw depends on the group; the swizzles in the dump (`r11.zxyy` and similar) make hand-reading error-prone. Two ways forward: write a small interpreter that runs the shader's microcode on the sampled textures (data-driven, large), or pick a per-map road colour from the Xenia shot (quick, guesswork).

---

## 2026-10-17 Default car is the FM2 Nissan Skyline GT-R V-Spec II (UNCOMMITTED)

**Changed:** `crates/fh1-engine/src/main.rs`: new `DEFAULT_FM2_CAR` (`../imported/fm2/cars/NIS_SkylineGTRVSpecII_02`, not the Nurburgring edition `NIS_SkylineGTRNur_02`) is picked before the Corrado when no `--car` is given. The Corrado stays the fallback if FM2 cars are not installed.
**Why:** the user's new Xenia reference shots (all FM2 tracks, closer to the engine's spawn point) use that car.
**Tested how:** built the release exe, ran `--track fm2/tsukuba_00` with a screenshot, read the log line.
**Result:** log shows `NIS_SkylineGTRVSpecII_02: 1540 kg, 253 N·m, redline 8000 rpm, 6 gears`; the screenshot shows a blue R34 on the Tsukuba start line.
**Still broken / not tested:** it is the default on every map (including Colorado), not only FM2 maps; the paint is the stock blue, not matched to Xenia's; other maps not launched with it.

---

## 2026-10-17 Wear-invert narrowed to tiled colour textures (UNCOMMITTED)

**Changed:** `fm2_scenery.rs`: the swap now applies only when the wear mask is bright (mean > 140), the draw has a real colour texture (not a baked road) and that colour is tiled over the draw (uv extent >= 4). Debug print of bright-wear draws removed.
**Why:** the first version (entry below) also hit Laguna Seca and Test Track roads (one-pass colours, extent about 1.0) and made them pale; the new Xenia shots show Laguna's road is mid-grey asphalt and Test Track's is mid-dark grey.
**Tested how:** re-imported all maps; screenshots of Suzuka, Laguna Seca, Test Track (opened). Tsukuba and Sebring taken, not opened.
**Result:** Suzuka's tan verge strip kept; Laguna Seca's road dark again like before the rule (still browner and darker than Xenia's); Test Track's road is still pale beige, so that is a separate bug that predates the rule.
**Still broken / not tested:** Test Track road colour (Xenia: mid-dark grey asphalt); the dark band beside Suzuka's edge line is still there.

## 2026-10-17 Wear-blend polarity inverted for bright masks (first version; superseded by the entry above)

**Changed:** `crates/fh1setup/src/fm2_scenery.rs`: a "wear" mask whose mean brightness is above 140 swaps the blend's two layers (`dark_invert`). `FM2_NO_WEAR_INVERT=1` turns it off.
**Why:** Suzuka's sandy verge looked wrong. Its wear mask is a 512x1024 grass-blade detail image (mean 193), not a tyre-streak mask (Road Atlanta's is 38), so darkening by it darkened almost the whole strip.
**Tested how:** Counted wear masks per map (before the field-number fix my first count showed everything as low). Bright masks: Suzuka 231, Test Track 198, Laguna Seca 175, New York 23, Maple Valley 13, Nurburgring 10, Tsukuba 4. Re-imported all maps, screenshots of Suzuka, Laguna Seca, Test Track, New York.
**Result:** Suzuka's verge now tan with a grass edge, like the Xenia shot, but a dark band now runs along the road edge next to the white line. Laguna Seca's road went from dark brown to pale flat beige-grey (Xenia's is mid-grey): not clearly better. Test Track's road also went pale. New York looked unchanged.
**Still broken / not tested:** Maple Valley, Nurburgring and Tsukuba shots from this run not opened. The rule also hits road draws on Laguna Seca and Test Track, probably wrongly (swap exposes the light base texture instead of the baked road). Proposed fix: apply it only where the base is a real colour texture, not a baked road. The user is testing in game before I decide.
**Next:** restrict the rule (or turn it off), then look at the dark band on Suzuka.

## 2026-10-17 Suzuka road brightness from the shader's own constant (UNCOMMITTED)

**Changed:** `fm2_scenery.rs`: the baked road's grain is scaled by `c3 / 0.7` (clamped 1..3); `baked` cache key gained that factor. `FM2_NO_C3_GAIN=1` turns it off.
**Why:** Suzuka's road was near-black. Its shaders carry `c3` = 1.55 (Tsukuba and Silverstone: 0.7) and `c5` = 2.58 (vs 2.0), so the original road is lighter.
**Tested how:** Screenshots of Suzuka, Tsukuba, Silverstone after re-import.
**Result:** Suzuka's road is now mid-dark grey, closer to Xenia; white lines still visible. Tsukuba and Silverstone unchanged.
**Still broken / not tested:** other maps' roads (they only change if `c3` > 0.7; not surveyed). Suzuka grass and shadows not compared against Xenia.
**Next:** survey `c3` across maps.

## 2026-10-17 Mugello start-line chequer (commit 8d4ae48)

**Changed:** `fm2_scenery.rs`: the chequer-strip rule also accepts one long thin mask (was exactly two), if its spread of values is wide (>45). Mugello's start line is a single chequer mask plus a grain.
**Why:** Mugello showed a plain white bar; Xenia shows a black-and-white chequer.
**Tested how:** Screenshot of Mugello, then re-imported everything and shot Tsukuba, Silverstone, Laguna Seca, Sunset Infield (opened); seven others taken but not opened.
**Result:** Mugello shows the chequer. The four opened maps unchanged.
**Still broken / not tested:** the other seven shots from that run.

## 2026-10-17 Test Track start-line chequer (commit 8258e9b)

**Changed:** `fm2_scenery.rs`: draws with no colour texture and two long thin (8:1) masks use the chequer mask (stretched to white, ignoring the near-white opacity mask) as the colour of a plain card; the card skips its own dark lightmap tile. `FM2_NO_CHECKER=1` turns it off.
**Why:** Test Track had a flat dark band across the road about 14 m ahead of the start. Hiding draws one at a time found it (models 16/102/122 area; model 102 is the 2-triangle strip).
**Tested how:** Hid draws; viewed the textures; screenshots after each step; regression shots of Nurburgring, Silverstone, Mugello (opened), Sebring and Suzuka (taken, not opened).
**Result:** chequer drawn. Also found the earlier "Test Track road is blown out by sheen" call was wrong (same with sheen off; it is light concrete). No Xenia shot of Test Track to compare.
**Failed on the way:** first version matched nothing because the item list repeats texture 227 (needed de-duplication); second version drew the chequer too dark until the lightmap tile and white stretch were handled.

## 2026-10-17 Keep the files' triangle winding (commit 442209b)

**Changed:** `fm2_scenery.rs`: the importer no longer flips a draw's winding by an up-facing majority; `FM2_FLIP=1` restores it. Added probe `crates/fh1-formats/examples/fm2_state.rs`.
**Why:** Laguna Seca's gantry sign text was mirrored and a sign board was missing. The flip turned gantries (undersides face down) inside out.
**Tested how:** Census of draws with a clear up/down majority on Tsukuba, Road Atlanta, Sebring, Suzuka (mostly up-facing: e.g. Sebring 1946 up vs 71 down; Nurburgring census printed blank, not tested). Re-imported all maps, start-line shots of 12.
**Result:** Laguna Seca sign reads correctly, missing board appears. Tsukuba grandstand roof, Suzuka gantry and signs, Silverstone fence, Sunset Infield grandstands appear. No roads or terrain went missing in the opened shots.
**Still broken / not tested:** winding only checked at start lines; a one-off flipped draw elsewhere is possible.

## 2026-10-17 Real-time sun shadows on FM2 + sky-dome fix (commit 5d0e3a2 shader, local only; `fm2_sky.rs` fix UNCOMMITTED)

**Changed:** `crates/fh1-remaster/src/material.rs`: FM2's lightmap path samples the sun's shadow map and darkens only where the lightmap is bright (`FH1_RM_FM2_DYN_SHADOW`, default 0.6). `crates/fh1-engine/src/fm2_sky.rs` (untracked file): sky dome marked `NotShadowCaster` and `NotShadowReceiver`. Sheen skips shadowed ground.
**Why:** FM2 scenery is drawn unlit from baked lightmaps, so cars and props cast no shadow.
**Tested how:** Many debug builds painting the shadow value, cascade index, shadow-volume test and the raw stored depth.
**Result:** the shadow value was 0 everywhere because the sky dome I added earlier encloses the scene and sat at the sun's near plane. With it excluded, the car's shadow shows on Tsukuba, Mugello and Test Track; a gantry shadow band shows on Road Atlanta.
**Failed on the way (about six attempts):** reading light 0 (the sun is not light 0, only the second has shadows on); filter method and caching (not the cause); my own cascade maths (fine).
**Still broken / not tested:** only a few maps looked at. `fm2_sky.rs` is untracked, so a clean checkout will not have the fix.

## 2026-10-17 Sun sheen on flat grey FM2 surfaces (commit 7b94644, local only)

**Changed:** `material.rs`: Blinn-Phong highlight off the sun on flat, low-saturation surfaces, scaled by lightmap (`FH1_RM_FM2_SHEEN` default 0.1, `FH1_RM_FM2_SHEEN_POW` 24).
**Why:** Xenia's roads glare toward the low sun; the unlit path had no specular.
**Tested how:** Road Atlanta at strengths 0.06, 0.18, 0.6; Tsukuba.
**Result:** 0.15-0.18 matched Xenia's glare on Road Atlanta; default later lowered to 0.1. Tsukuba looked the same.
**Still broken / not tested:** sunlit road on Road Atlanta's left is still quite bright; white kerbs also catch the sheen.

## 2026-10-17 Tyre streaks on two-colour road shoulders (commit 075ba6c)

**Changed:** `fm2_scenery.rs`: road draws listing tiled asphalt plus a one-pass overlay colour and two masks now darken the tiled asphalt by the streak mask; default wear darkness 0.55 to 0.4. `FM2_NO_ROAD_WEAR=1` turns it off.
**Why:** Road Atlanta's streaks were barely visible (the overlay was nearly the road's grey).
**Tested how:** Road Atlanta at wear 0.25 (no effect, wrong path), then after the fix at 0.3; Tsukuba and Sebring shots after re-import.
**Result:** darker rubber lanes on Road Atlanta; Tsukuba and Sebring unchanged. Xenia's racing line is still sharper.

## 2026-10-17 Road grain contrast softened (commit fbf7e41)

**Changed:** `fm2_scenery.rs`: baked road grain keeps its mean, contrast halved (`FM2_ROAD_TEX_CONTRAST`, default 0.5).
**Why:** Road Atlanta's asphalt looked speckled; the original shader sums four grain textures.
**Tested how:** Road Atlanta and Tsukuba screenshots. **Result:** smoother; Tsukuba fine. Road is still lighter than Xenia's (left alone, tied to lighting).

## 2026-10-17 Road Atlanta kerb stripes (commit f5f7496) and verge (commit 01a90ab)

**Changed:** `fm2_scenery.rs`: (1) grey-mask blends use the layers in the shader's order (swap after taking the mask out); (2) the grey-mask test compares the wider of the two uv spans with lower thresholds (<1.5 / >=2).
**Why:** pale blurry banks beside the road, then missing green-and-white kerb stripes.
**Tested how:** painted every texture a solid colour to identify draws; Road Atlanta shots; Nurburgring, Mugello, Suzuka shots after the swap.
**Result:** orange-brown verge and green-white kerb stripes like Xenia. Terrain-blend counts rose (Tsukuba 350 to 388, Road Atlanta 595 to 629).
**Not tested:** Sebring shot taken, not opened; eight other maps not re-checked after (2).

## 2026-10-17 Nurburgring blurry grass (commit 305006c)

**Changed:** `fm2_scenery.rs`: with three or more colours and one mask, greyscale colours are not blend layers (`FM2_KEEP_GREY=1` to disable).
**Why:** one terrain blend took a greyscale AO image as its first colour, giving blurry grey-olive verges.
**Tested how:** bisected to draw 1601 (model 89); screenshots of Nurburgring, Sebring, Maple Valley.
**Result:** crisp green grass. Only two of three layers are blended, so brown earth patches are still missing.

## 2026-10-16/17 FM2 sky, sun and fog colours, fixed lighting

**Changed:** commits 9def1a1, 8f40498 (painted sky from `<track>out.sky.rmb.bin`; importer `fm2_sky.rs`, engine `fm2_sky.rs`, all sectors kept), a5954b5 (sun colour from `LightColor`, fog colour from TrackSettings fog layers, `FH1_FM2_FOG_GAIN` 0.2), c4770ff (FM2 maps: clock frozen at 14:00, own TrackSettings, sun from `ShadowLightDirection` with z mirrored), 401bd0a (default grading LUT 0.3 for FM2 maps).
**Why:** warm sepia cast and Colorado's sky on FM2 maps, versus Xenia's neutral light and painted skies.
**Tested how:** screenshots of Tsukuba, Road Atlanta, Mugello and others at each step.
**Result:** painted skies show; colour cast reduced. DDS sky textures needed a custom loader and an R/B swap for formats ending 0x86.
**Still broken / not tested:** Mugello sky has a hard-edged patch; no sun disc or glow; overall grade still flatter than Xenia. Engine side (`track.rs`, `main.rs`) is uncommitted in this folder.

## 2026-10-16 Road and terrain blends (commits 33cb6a3, f7b1ed4, a81e1aa, 7e9284a)

**Changed:** `fm2_scenery.rs`: black road constant uses the grain texture as asphalt (gamma-encoded, threshold 0.1); road shoulders blend tiled asphalt and edge strip by the strip's mask; a greyscale DXT spanning a draw once with two tiled colours is the blend mask.
**Why:** Tsukuba, Suzuka, Silverstone roads pitch black; Sebring shoulders wrong.
**Result:** Tsukuba roads grey asphalt with checker and kerbs. Suzuka and Silverstone stayed dark until the Suzuka gain above; Mugello darkened at threshold 0.1 so I put the cutoff at 0.1 only after the grain texture path was gamma-fixed.

## 2026-10-16 Earlier today (from memory, thinner detail)

- **Lightmaps and per-draw uv:** FM2 draw uv now comes from each draw's own shaders (`fm2_uv.rs`, `fh1-shaders/src/fm2.rs`); lightmap tiles cropped from `.stx` pages; remaster tables rebuilt with `fh1setup data/work/disc --only remaster --force`.
- **Alpha atlases:** a texture only used by shaders with a constant output alpha stays opaque (alpha is not opacity there).
- **Normal-map filter** (mean ~127,127,255) keeps normal maps out of colour lists; **atlas crop** pass; **double pixel-shader set** (`Draw.ps_alt`).
- **x32 uv fallbacks** cut from 119 to 80, then to 71 draws: position-based roads, Sebring other-samplers, Nurburgring fallbacks, single-colour draws; planar road grain laid out in metres.
- **Purple tint, New York bands, Mugello lines** (the lines are dark rubber decals, correct), **Road Atlanta road and far bank**.
- **Engine port:** FM2 engine changes ported into `ui-elements`; deadzone and camera changes merged from main (local merge commits 1faf336, e27cba3, not pushed).
- **Launcher:** FM2 and FM1 tickboxes (worktree `fh1_rewrite_main`, branch `launcher-fm2-fm1`, commit 460f9a4); non-RTX release zip built as `0.1.3-fm2`. Not merged; the zip has not been rebuilt with any of today's fixes.
- **Tested:** screenshots of each fix at the time. **Not tested:** a real race on any FM2 track (AI line, lap timing, collision).

---

## Open items (not done)

- Road Atlanta: catch-fence detail missing; road lighter than Xenia's.
- Props: Nurburgring cones; lamp glows.
- About 71 draws still use the old x32 uv guess (Nurburgring buildings, Sebring terrain blends).
- Terrain blends use two of three layers (brown earth patches missing).
- Tsukuba kerbs render; not compared closely against Xenia.
- No PR opened for `fm2-lightmaps`; `launcher-fm2-fm1` unmerged; release zip not rebuilt.
- Per AGENTS.md: no commits or builds until asked; `docs/DESIGN.md` does not exist yet.

## 2026-10-19: road brightness pipeline diagnosis (no code change)
Read cal_5..cal_9 and dbg_0..4 (Sunset Infield): engine stages agree with their own formulas (albedo 0.34, term 0.37, product 0.126); Xenia needs ~0.2. Sun-height x LightColor does not correlate with per-track ratios. Written up in STATUS.md. Not tested: shader-derived sun term.

## 2026-10-19: FM2 skips FH1's colour grade
**Changed:** `crates/fh1-engine/src/main.rs`: FM2 tracks set `FH1_RM_TONEMAP=none`, `FH1_RM_LUT=0`, `FH1_RM_VIGNETTE=0` unless set; `FH1_RM_FM2_GRADE=1` restores the old look (LUT 0.3, filmic curve).
**Why:** the CPU pixel-shader interpreter on Sunset's road gives the Xenia pixel (113,110,102 vs 111,110,98); the engine's own stages matched their formulas; FH1's curve darkened the road to 73.
**Tested how:** one shot per track, median of road bands vs the Xenia shots, linear-light ours/Xenia. Before -> after: Test Track 0.62->1.0, Sunset 0.36->0.74, New York 0.41->1.11, Mugello 0.56->0.98, Laguna 1.36->0.82 (better); Tsukuba 1.05->1.8, Nurburgring 2.26->3.5, Maple Valley (shot differs) 6.8, Suzuka 1.89, Road Atlanta 1.6 (worse: too bright); Silverstone 0.32, Sebring 0.55 (too dark). Sum of |ln ratio| over 12 tracks 9.9 -> 7.2. Not tested: a real race, other FH1 tracks (change is FM2-only).
**Open:** bright roads need their own fix (Nurburgring, Maple Valley, Tsukuba, Suzuka, Road Atlanta; Xenia also adds a warm haze/bloom on Atlanta), dark ones (Silverstone black patch, Sebring).

## 2026-10-19: Nurburgring road diagnosis (no code change)
Interpreter on model 118 draw 0 gives 125 (sRGB), engine 123, Xenia 70. Cause not found; see STATUS.md. Not tested: the model 151 draws.

## 2026-10-19: Nurburgring painting test (no code change)
Road = texture 123 of model 118 draw 0 (green when painted); grass matches Xenia, road does not. Installed data restored from backup and verified. See STATUS.md.

## 2026-10-19: copied CPU shader interpreter into this checkout; gamma probe switch
**Changed:** added `crates/fh1-shaders/src/ps_cpu.rs` and `crates/fh1setup/examples/fm2_ps_run.rs` (copied from the `fm2-lightmaps` worktree, where they were uncommitted), `pub mod ps_cpu;` in `crates/fh1-shaders/src/lib.rs`, and the probe-only `PS_GAMMA` env switch in `fm2_ps_run.rs`. No importer or engine change.
**Tested how:** `cargo build -p fh1setup --example fm2_ps_run` ok; Nurburgring model 118 group 0 draw 0 baseline 0.211 reproduced; with PS_GAMMA=3,4,5 gives 0.066 (Xenia 0.058). Sunset model 77 group 2 draw 3 goes black with PS_GAMMA. Details in STATUS.md. Not tested: engine, other tracks.

## 2026-10-19: gamma-per-draw investigation (probes only, no importer/engine change)
**Changed:** new probe `crates/fh1setup/examples/fm2_bind_dump.rs`; `fm2_ps_run.rs` prints draw state records.
**Found:** no gamma bit in draw state, `0x2d` binding flags, PVS flags or `.bix` words. Gamma-decoding only the BC4 grey samplers brings Nurburgring (64 vs Xenia ~70) and Tsukuba (76, was too bright) close and leaves Sunset fine; gamma on Sunset's BC1 colour breaks it. Hypothesis: BC4 = sRGB. Details in STATUS.md.
**Tested how:** `/tmp/lmgrid.sh` lightmap-grid sweeps with `PS_GAMMA` per sampler. Not tested: engine render, other tracks, Test Track.

## 2026-10-19: FM2_BC4_SRGB switch (off by default), Nurburgring road
**Changed:** `crates/fh1setup/src/fm2_scenery.rs`: with `FM2_BC4_SRGB=1`, draws with one BC1-3 colour texture plus two small square grey BC4 textures get a baked colour texture `gain * lerp(sRGB-decoded grey means, by colour luma)` (`FM2_ROAD_GAIN` = gain, `FM2_BC4_SRGB_W=1` also decodes the luma). Default behaviour unchanged.
**Why:** the shader reading (STATUS.md) says the road albedo is lerp of the two BC4 greys by the BC1 texture's luma; sRGB-decoding the greys matched Xenia in the CPU interpreter.
**Tested how:** re-imported Nurburgring with the switch on, rebuilt remaster tables, release-engine screenshot of the grid view, road-band medians: off 0.192 linear, on 0.042 (Xenia about 0.0575). Then re-imported with the switch off (baseline reproduced 0.192). Not tested: other tracks, a race, grey detail tiling.
**Revert:** delete the `FM2_BC4_SRGB` block, or just leave the env var unset.

## 2026-10-19: BC4-sRGB extension checked, not implemented (no code change)
Interpreter on Test Track model 321 draw 0: gamma on the grey BC4s gives 74 against Xenia ~115 (no gamma 126), so the rule fits Nurburgring but not Test Track. Details in STATUS.md. Not tested in the engine.

## 2026-10-19: grey BC4 texture comparison (no code change)
Compared Nurburgring 124/125 with Test Track 507/11 (and Tsukuba, Sunset BC4s): no pixel property separates them. Per-sampler gamma in the interpreter fits Nurburgring (tf4, tf5), Test Track (tf3 only) and Tsukuba (tf4) with different samplers, so gamma is a per-shader-sampler setting we have not found in the data. Details in STATUS.md. Not tested: the engine.

## 2026-10-19: searched model files for the texture gamma bit (probe only)
**Changed:** new probe `crates/fh1setup/examples/fm2_data_dump.rs`; no importer/engine change.
**Found:** no sign/gamma dword in `.bix` headers, PVS records, stream state or binding records, or the pixel-shader container's constant table (all samplers share one type descriptor). Only material parameter names (`..._colour` / `..._bump`) hint at a type. Details in STATUS.md. Not tested: decoding the `pool` table, the XEX.

## 2026-10-19: decoded the .data pool table (probe only, no code change)
The table after 0x1690 is a relocation list, no gamma info. Material names carry `_colour_K` suffixes that are constant per track and match the gamma sampler item on Test Track (1) and Tsukuba (2) but not Nurburgring; unproven. See STATUS.md. Not tested: engine.

## 2026-10-19: FM2_BC4_SRGB gated to the Nurburgring road shader family
**Changed:** `crates/fh1setup/src/fm2_uv.rs` (`DrawUv::ps_sig`, hash of the draw's pixel-shader microcode), `crates/fh1setup/src/fm2_scenery.rs` (`Pending::ps_sig`, `NB_ROAD_SIGS`, `FM2_BC4_SRGB_SIG` debug print; the baked-blend rule now requires the hash). Default behaviour unchanged (switch off).
**Why:** the pattern also matched one Sebring draw, which is already too dark.
**Tested how:** import of all tracks with `FM2_BC4_SRGB=1`: only Nurburgring's 4 draws change; Nurburgring road band 0.0415 linear against 0.192 off (Xenia ~0.0575); Sebring shot on vs off differs by 0.43 mean pixel value (noise). Then re-imported with the switch off. Not tested: a race, the other Nurburgring layouts.
**Revert:** remove the `ps_sig` plumbing and the switch block, or leave `FM2_BC4_SRGB` unset.

## 2026-10-19: INCIDENT: FM2 installed data overwritten by the older importer, restored (no source change)
**What:** my FM2 re-imports in this session used this branch's importer, which is missing the `fm2-lightmaps` importer work, so all FM2 tracks were regenerated with older road rules (Tsukuba's road went black).
**Repair:** built the `fm2-lightmaps` worktree importer into `target-fm3/` (inside this project; the other folder was only read) and re-imported all FM2 tracks and the remaster tables with it. Checked: Tsukuba and Nurburgring roads back to their earlier look (band values in STATUS.md). Not tested: the other ten tracks against earlier shots.
**Lesson:** re-import FM2 only with `target-fm3/debug/fh1setup.exe` until the branches are reconciled.

## 2026-10-19: fm2-lightmaps merged into the working tree, BC4 switch ported (no commit)
**Changed:** added `fh1setup/src/fm2_sky.rs` (+ `mod`), three `fh1-formats` examples, the `fm2_show` sky argument; `fm2_scenery.rs` and `fm2_uv.rs` now the `fm2-lightmaps` worktree's working copies plus my `FM2_BC4_SRGB` block (hash gate). Previous versions in `data/work/premerge/`.
**Tested how:** build ok; all FM2 tracks re-imported with the switch: only Nurburgring changes (0.187 -> 0.0407 linear road band); re-imported with the switch off, remaster tables rebuilt; Tsukuba shot (road band 115/87 left/right, far road 0.043 vs Xenia 0.070). Not tested: the other tracks after the merge, a race.
**Tsukuba:** gamma rule not applied (ours is already 0.62x Xenia on the crude comparison).

## 2026-10-19: found FM2's own post-process shader and per-track grade data (probes only)
New probes `fm2_fxlist.rs` / `fm2_fxdump.rs` (fh1-shaders examples). The game's `FinalCombineBloomTonemap` does desaturate + tint + a Light/Dark tone curve driven by the TrackSettings values (formula in STATUS.md). Nothing changed in the engine or importer. Not tested: implementing it in the engine.

## 2026-10-19: FM2 final combine in the engine post, `FH1_FM2_POST=1` (off by default)
**Changed:** `crates/fh1-remaster/src/post.rs` (`Fm2Post`, grade shader branch, extended uniform), `crates/fh1-engine/src/main.rs` (loads the track's TrackSettings for FM2 when the env var is set). Default behaviour unchanged. Optional `FH1_FM2_POST_T` (0..1, default 1) blends the dark / light tone-curve sets.
**Test it:**
```
FH1_FM2_POST=1 FH1_OWNERSHIP=0 FH1_SHOT=screenshots/post_tsu_on.png FH1_SHOT_AT=28 ./target/release/fh1-engine.exe --track fm2/tsukuba_00
```
and the same without `FH1_FM2_POST=1` for the comparison. You should see the whole frame slightly warmer and darker/contrastier on tracks with a strong desaturation (Mugello, Suzuka, Sebring); Tsukuba's road changes little (left band 115,114,111 -> 118,115,110). Band script: `python data/work/cmp_bands.py <xenia png> <ours png...>`.
**Result:** see STATUS.md (mixed; not conclusive). Not tested: the 255 unit, a computed `t`, bloom.

## 2026-10-19: FM2 post verified against the game's code, blend computed from the frame (`FH1_FM2_POST=1`, off by default)
**Changed:** `crates/fh1-remaster/src/post.rs`: `Fm2Post` gets `lum_min` / `lum_inv_range` from `TrackLumLimits`; the grade shader computes the dark / light blend `t` from an 8 x 6 grid of the frame (override `FH1_FM2_POST_T`). Units verified in the game's code (all points, boosts and limits / 255, gamma unscaled).
**Tested how:** release build (`--no-default-features`), Tsukuba, Sebring, Suzuka, Laguna Seca and Mugello shots at the default spawn, band ratios against the Xenia shots (STATUS.md). Result: with the computed blend every frame goes to the dark curve and gets brighter, away from Xenia on most tracks; switch stays off. Not tested: the game's adaptation texture, bloom.

## 2026-10-19: traced the luminance texture of the game's final combine (no code change)
`DownSample16X` / `DownSample16XAndAccumulate` average the scene colour (same units as the final combine's input) with temporal smoothing, so the blend input is the frame's average luminance; our frames average well below every track's `TrackLumLimits`. Post curve cannot explain the roads' brightness gap (STATUS.md). Not tested: HDR sky.

## 2026-10-19: FM2_BC4_SRGB generalised to shader-derived road albedo (off by default)
**Changed:** `crates/fh1setup/src/fm2_scenery.rs`: `decode_rgba` marks BC4 textures when `FM2_BC4_SRGB` is set, `sample_rgba` sRGB-decodes them, and `FM2_BC4_SRGB` also enables the shader-derived road albedo (`shader_albedo`).
**Tested how:** interpreter lit-texel comparison on four families (table in STATUS.md): BC4 gamma fits all. Re-import with the switch and screenshots: Tsukuba left band ratio to Xenia 1.60 -> 0.98; Suzuka 3.1 -> 0.32 and Mugello 0.92 -> 1.83 (worse). Re-imported with the switch off afterwards. Not tested: Suzuka / Mugello shaders, other tracks.

## 2026-10-19: Suzuka / Mugello road draws in the interpreter (probe + debug print only)
`FM2_ROAD_DBG=1` prints more per road draw (shader hash, triangles, constants, textures). Interpreter table (STATUS.md): BC4 gamma fits Nurburgring, Tsukuba, Test Track but not Suzuka (0.33x) or Mugello (0.45x); likely a different draw under the camera or a missing light term. Not tested: both.

## 2026-10-19: painting test on Suzuka and Mugello (no code change)
Baked road textures painted solid colours at the spawn view: the road under the camera is `ffff2000` (Suzuka, shader family 52ff81cb...) and `ffff2002` (Mugello, 4319799a...), the draws already analysed; textures restored and verified. Mugello's shader gives 0.06 to 0.08 against Xenia 0.133 (c0 = 0 family, no sun term). Not tested: Mugello lightmap texels.

## 2026-10-19: Mugello lightmap tile checked (no code change)
Tile 0x714: max 239 (0.94), lit strips 0.78 to 0.87; at lm 1.0 the light term only rises 1.17x, so it cannot explain the 1.4x to 2.2x gap to Xenia (0.133 vs 0.06 to 0.095). The family's shader has no ambient or sun term (c0 = 0). Not tested: constants persisting from earlier draws, the display exponent.

## 2026-10-19: inherited constants checked (probe only)
The one constant each road shader reads but its draw does not set is the specular exponent register (c2 / c8 / c4 / c6); it is used only in the specular term (killed by c0 = 0 on Mugello). Inherited values cannot be recovered statically and do not change the lit-texel predictions. New probe `fm2_ps_consts.rs`. Not tested: frame-order replay.

## 2026-10-19: material block constants read (probe/script only)
The missing specular exponents are in each model's material block (Nurburgring 9.68, Mugello 9.68, Suzuka 8.47, Tsukuba 12.31, Test Track 7.78). Interpreter with the real exponents and sun geometry: the sun term adds at most +0.004 to the lit road, so it is not the cause of the remaining gaps. No code change; scratch scripts `data/work/mat_consts.py`, `data/work/sun_real.py`.

## 2026-10-19: texture gamma in the game's code (no code change)
The `.bix` format word has no sign bits; the `0x2d` record applies a per-binding sampler-state block from a model-local table, and the sampler-state block carries the sign/gamma bits. So gamma is a per-binding setting in the model's state table, not a format property (STATUS.md). Not done: decoding the state tables (hashed state names).

## 2026-10-19: per-model state tables decoded (no code change)
The `0x2d` index selects identical, empty state entries (Nurburgring, Tsukuba), so it does not carry gamma; hashes unidentified. Earlier claim corrected in STATUS.md. Not done: the texture header builder and set-texture code.

## 2026-10-19: texture header builder read (no code change)
`0x823c5e00` / `0x8236b6d0` take the sign bits only from the format word (zero in the files); no format-dependent gamma. The BC4 proxy is not explained by the texture code; a Xenia GPU trace would decide it. See STATUS.md.

## 2026-10-19: VMX128-aware listing (scratch tool only)
`data/work/vmx128.py` decodes the vector instructions of the game listing; checked on the light-direction code. No sign of a display gamma stage in the game's software. See STATUS.md.

## 2026-10-19: twelve-track comparison with correct Xenia mapping (scripts only)
`shoot_all.sh` / `cmp12.py`. BC4 switch: Tsukuba, Silverstone, Nurburgring improve; Suzuka over-corrects; Mugello and Maple Valley worsen; six tracks unchanged. Whitelist by shader hash would keep the gains. No code change.

## 2026-10-19: shadow term checked (no code change)
Light floor 0 changes nothing visible; the game's shadow/lit ratio per track is 0.17 to 0.56 against the engine's constant 0.4. Small refinement only. See STATUS.md.

## 2026-10-19: six untouched road families, first pass (diagnostics only)
`FM2_UNDER` got `FM2_UNDER_TOL` and the shader hash; New York's `h_diff_1` road read in the interpreter (matches Xenia with ordinary sRGB texture decode); the engine sun sheen is compensating for a real specular glare on Road Atlanta / Maple Valley. Only the importer's diagnostic print changed.

## 2026-10-19: FH1_FM2_SPEC switch in the engine (off by default)
**Changed:** `crates/fh1-engine/src/main.rs`: with `FH1_FM2_SPEC=<k>` the FM2 sun sheen uses power 9.7 and gain `k * GlobalSpecularLevel` from the track's TrackSettings. Default unchanged.
**Tested how:** `shoot_all.sh` / `shoot_some.sh` / `cmp12.py` over twelve tracks with BC4 data: sum of |ln ratio| 16.42 (current sheen), 16.15 (k = 0.05), 22.72 (no sheen); Road Atlanta and Maple Valley improve, Nurburgring and Silverstone get worse. Not adopted. Not tested: a per-draw version.

## 2026-10-19: per-draw specular term (switches only)
**Changed:** `fm2_uv.rs` (`PsRun::spec_exp`, `spec_exponent`), `fm2_scenery.rs` (`shader_eval` refactor, `shader_spec_k`, `draw_spec`, material key and `ps[8]`, `FM2_SPEC_MAT`, `FM2_UNDER_TOL`), `remaster.rs` (p0.y = K, p1.w = exponent), `fh1-remaster/src/material.rs` (new term, `FH1_RM_FM2_SPEC_*` constants), `fh1-engine/src/main.rs` (`FH1_FM2_SPEC_MAT`, `FH1_FM2_SPEC`). Defaults unchanged.
**Test it:** `FM2_BC4_SRGB=1 FM2_SPEC_MAT=1 ./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps`, `./target/debug/fh1setup.exe data/work/disc --only remaster --force`, then `FH1_FM2_SPEC_MAT=1 FH1_OWNERSHIP=0 FH1_SHOT=screenshots/x.png FH1_SHOT_AT=28 ./target/release/fh1-engine.exe --track fm2/nurburgring_00` (add `FH1_RM_FM2_SPEC_DIR=-1,1,0` for the game's default light direction); compare with `python data/work/cmp12.py`. Reset: re-import without the variables.
**Result:** sum of |ln ratio| 16.42 -> 15.92 over twelve tracks (small); mixed per track. Not adopted. Not tested: a race, the direction literal against a Xenia trace.

## 2026-10-19: BC4 sRGB shader albedo whitelisted by shader hash
`BC4_ROAD_SIGS` (Nurburgring, Tsukuba, Silverstone road shaders) in `fm2_scenery.rs`; `FM2_BC4_SRGB_ALL=1` lifts it. Twelve-track sum of |ln ratio| 17.68 (off) -> 16.24; Suzuka / Mugello / Maple Valley unchanged from default. Test: `FM2_BC4_SRGB=1 ./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps`, remaster rebuild, `bash data/work/shoot_all.sh x`, `python data/work/cmp12.py off x`.

## 2026-10-19: BC4 sRGB road albedo on by default (importer)
**Changed:** `fm2_scenery.rs`: `bc4_srgb_on()` (default on, `FM2_BC4_SRGB=0` = off) replaces the opt-in checks. **Result:** all FM2 tracks re-imported with it; twelve-track sum of |ln ratio| 17.68 -> 16.19. **Revert:** `FM2_BC4_SRGB=0` on the import plus the remaster rebuild, or delete the `bc4_srgb_on` default. **Test:** `./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps` (needs the `fm2` feature build), `./target/debug/fh1setup.exe data/work/disc --only remaster --force`, `FH1_OWNERSHIP=0 FH1_SHOT=screenshots/t.png FH1_SHOT_AT=28 ./target/release/fh1-engine.exe --track fm2/tsukuba_00`; Tsukuba's left road band should read about 90 (was 113), Nurburgring about 61 (was 128).

## 2026-10-19: Laguna Seca road investigation (diagnostics only)
Added `FM2_FIND_TEX` / `FM2_NEAR` prints to the importer, `data/work/bitpaint.py`. Laguna's spawn road = model 81 (plain diffuse draw with a pale 1024x1024 colour texture and tile 175, plus two `e1ba186e` blend draws); the plain draw alone predicts 0.7 against Xenia's 0.055; the blend draws are the suspect. No behaviour change.

## 2026-10-19: Laguna Seca road, blend shader and painting contradictions (diagnostics only)
`FM2_FIND_SIG` added; blend formula read; painting says texture 0xbe paints the road but nearby draws do not use it; the Xenia shot is under the banner's shadow. No behaviour change.

## 2026-10-22: Laguna banner shadow checked (no code change)
Sun is low and in front; engine shadows work (strength 1.0 darkens the shadowed band); culling and cascade changes do nothing; the 3.4x gap is the road's base colour (BC4-textured family `1eb344c3...`), not a shadow. See STATUS.md.

## 2026-10-22: Laguna 1eb344c3 family in the interpreter (no code change)
Lit values: none 0.127, colour-only sRGB 0.055, BC4-only sRGB 0.052, both 0.022 against Xenia 0.055 / 0.122: not whitelisted. Suggests a per-family gamma-sampler table. See STATUS.md.

## 2026-10-22: per-family gamma table (importer)
`FAMILY_GAMMA` / `family_gamma()` replace `BC4_ROAD_SIGS`; `shader_eval` takes the per-sampler mask and reads placeholder textures as white; `FM2_FAMILY_COLOUR=1` (off) scales colour textures to the shader's brightness. Twelve-track sum 16.30 (default) vs 16.19 before, 16.27 with the colour scaling on. Test: `./target/debug/fh1setup.exe import-fm2 data/work_fm2/disc --only maps`, remaster rebuild, `bash data/work/shoot_all.sh x`, `python data/work/cmp12.py off x`. Revert: `FM2_BC4_SRGB=0`.

## 2026-10-22 (4): Laguna spawn-road shaders measured (no code change)
Read the two model-81 shaders and tile 175's pixels; lit G predicted 0.3 to 0.8 under any gamma choice against Xenia's 0.055 to 0.12, so nothing was added to `FAMILY_GAMMA`. Documented in STATUS.md. Laguna's installed map was re-imported with the current importer (an old release exe was run by mistake first and then overwritten). Nothing committed.

## 2026-10-22 (5): c100 / LightAmbient check (no code change)
Read the Mugello and Suzuka road shaders in full: neither reads c100, so no ambient term to port. Documented in STATUS.md. Nothing committed.

## 2026-10-22 (6): display exponent 2.0 vs 2.2 (no code change)
Arithmetic on the lit-texel numbers: 2.2 fits better (sum |ln| 1.32 vs 1.75). Documented in STATUS.md. Nothing committed.

## 2026-10-22 (7): car-shadow strength experiment (no code change)
`FH1_RM_FM2_DYN_SHADOW=0.9` on four tracks: Test Track right 7.41 -> 1.70, Tsukuba 2.16 -> 1.28, Maple Valley / Suzuka unchanged. Default stays 0.6. Documented in STATUS.md. Nothing committed.

## 2026-10-22 (8): Suzuka / Maple Valley right-band shadow check (no code change)
`FH1_RM_SUN_DIR=shadow` made things worse (Maple Valley left 0.09, Test Track right 15.9, Suzuka unchanged). Documented in STATUS.md. Nothing committed.

## 2026-10-25: FH1_RM_FM2_SHADOW_DIR experiment
Added the env switch in `light.rs` (default off). No variant helps; Suzuka's road gets no real-time shadow in any variant. Documented in STATUS.md. Nothing committed.

## 2026-10-25 (2): FH1_RM_FM2_DYN_LIT experiment
New env switch in `material.rs` (default off) bypassing the lightmap gate of the dynamic car shadow. Suzuka's car shadow appears; right band unchanged. Documented in STATUS.md. Nothing committed.

## 2026-10-25 (3): FH1_FM2_SHADOW_GAME (game shadow formula)
New switch (engine main.rs sets env from TrackSettings ShadowColor; material.rs uses the game's min() rule). Twelve-track sum 16.30 -> 15.94; New York and Laguna better, Test Track right band worse. Default off. Nothing committed.

## 2026-10-25 (4): FH1_FM2_SHADOW_GAME on by default
Engine main.rs: on unless `FH1_FM2_SHADOW_GAME=0`. Documented in STATUS.md. Nothing committed.

## 2026-10-25 (5): car drop shadow for FM2 glTF cars
`drop_shadow.rs`: silhouette collection also takes glTF body parts (`FH1_FM2_DROPSHADOW=0` = off). The blob appears; right bands barely move; the dark ratio in Xenia stays unexplained. Documented in STATUS.md. Nothing committed.

## 2026-10-25 (6): FH1_RM_FM2_SHADOW_POW experiment
Env exponent for the game-rule shadow factor (default 1). 2.2 fixes Test Track / Tsukuba / Laguna partly, ruins New York. Default unchanged. Nothing committed.

## 2026-10-25 (7): shadow factor numbers, New York vs Test Track (no default change)
Added `FH1_RM_FM2_SHADOW_DBG` (debug paint of shadow-map visibility and lightmap lerp; default off). New York supports a linear factor, Test Track's right band cannot be read as a shadow ratio. Documented in STATUS.md. Nothing committed.
