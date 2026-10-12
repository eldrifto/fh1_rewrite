# FM2 (Forza Motorsport 2) import: handoff — THE OPEN PROBLEM IS THE PER-DRAW UV SCALE

Branch `fm3-import`. **Everything FM2 is uncommitted** — do not commit or push without asking the user.
A Claude memory note, `fm2-scenery-format.md` (project memory dir), carries the same findings.

## The problem (read this first)

FM2 circuits import with textured scenery and lightmaps, and look right except for one systematic defect: **the importer scales every
draw's texture coordinates by a constant 32 (`FM2_UV_SCALE`, default 32), and the real scale differs per draw.**

- Vertex UVs are 16-bit **signed-normalised** pairs (`i16 / 32767`, `Draw::uv_at`). The game turns them into texture coordinates with a
  scale we have never found (almost certainly a vertex-shader constant or the vertex-fetch's exponent adjust).
- **×32 is right for:** terrain / ground tiling, the Silverstone banner bridge (logo repeats ~9 times), kerb stripes, the Nurburgring pit
  apron grid (cells ~0.3–0.5 m).
- **×1 is right for:** signs (billboard boards), tree cards (real conifer / leaf-sheet shapes instead of dotted sparse foliage),
  chain-link fences. **The user confirmed the black rectangle near the Nurburgring start is a sign with a missing texture: it is the ×32
  scale sampling the wrong part of the sign's texture.**
- Everything else already in the pipeline (lightmaps, cut-outs, blends, lighting) is downstream of correct UVs and works.

### Evidence (all reproducible, see "Reproduce")
| Test | Result |
|---|---|
| Nurburgring, `FM2_UV_SCALE=1` | signs textured, trees/fences correct; apron becomes white splotches |
| Silverstone, `FM2_UV_SCALE=1` | banners smear into one stretched logo (wrong); terrain tiling gone |
| `FM2_UV_SCALE=auto` (power-of-two from metres-per-uv-unit, `tiling_scale`) | worse: banner lettering cropped, Laguna Seca sign striped |
| Raw uv span, world-area per uv-area, has-lightmap, stride, model number | none separates the ×1 and ×32 classes (printed by `FM2_AT`, below) |

Data points (Nurburgring, from `FM2_AT=2017,-1920,40`): apron models 22/23 (tex file 22, span 0.97×0.82, ~80 m² per uv unit, **needs 32**);
tree card model 456 (tex 7, span 0.28×1.00, **needs 1**); leaf cards model 431 (tex 14/16/17/18, span 0.5×0.5, **needs 1**); fence quad model 76
(tex 51, raw uv ≈ constant, span 0.00). Silverstone terrain model 10 (blend: mask uv ×1, colour tiling ×32); banner models near the start bridge (**32**).

### What the blend path already knows
For terrain blend draws (`h_blnd2_ao*`) the +12 pair is the **mask uv at ×1** (VERIFIED: its footprint matches the mask atlas exactly) and
the colour tiling is the same uv **×32**. So ×32 is a real multiplier in the game for those, not an artefact of the importer. Non-blend
draws probably have a per-material tiling multiplier that is 1 for object cards and 32 for tiled surfaces.

### Where to look (nothing below is decoded)
1. **The recorded draw stream (`.stream`)** — per draw records between the `0x2e`/`0x2d`/`0x01` ones this importer uses:
   - `0x03 [start, count<<16|1, f32 x4]`: every draw has one, always `0 400001 1.0 0.0 0.0 1.0` (a `(1,0,0,1)` vec4: could be a uv scale/offset
     matrix row; not varying in the models checked).
   - `0x05 [ptr, 480004]`, `0x06 [start, count, f32...]` (pixel-shader constants: model 41's road draw has `0 4 [1,1,1,1][0.039..][0.3..][0.7..]`
     and `0 50001 [1.05 x3, 1]`), `0x08 [ptr, 10001]` (render state, probably cull mode, see D3DRS_CULLMODE string), `0x16 [0x02000000, a, b]`
     (`.data` ranges), `0x19`, `0x2a`, `0x1d`. Dump them per draw for a ×1 model and a ×32 model and diff. Model 456 (tree, ×1) vs model 22 (apron,
     ×32) on Nurburgring; model 10 (terrain) on Silverstone.
   - `0x2e` vertex-fetch records `[fixup, vertex-record offset, 0x10000|0, 0, size-ish, 0]` come in pairs per draw (the second has different last
     words, e.g. `0x784` / `0xc88`): these look like Xenos vertex-fetch constant dwords. If the UV element is declared SHORT2 (integer) vs SHORT2N
     (normalised) the effective scale differs by 32767/1024 = 32 — **the ×32 may simply be `int16 / 1024` for some draws and `int16 / 32767` for
     others, signalled by a fetch-constant/format field.** Worth testing first: compare these words between a ×1 and a ×32 draw.
2. **The vertex shader microcode.** The `.data` VS containers (0x102A1101) are near-empty stubs (all share GUID `f249031746a609a4`). The real
   microcode sits in the `.gpu` tail, each shader ending `4e 4a 00 00` + 8-byte GUID. `crates/fh1-shaders/examples/fm2_gpu_shaders.rs`
   extracts and disassembles it but its GUID lookup finds nothing on Nurburgring models (works on Silverstone model 10, whose pixel shaders it does
   dump). A vfetch in the VS has `exp_adjust`; a `mul`/`mad` on the texcoord input would be the scale. The PS constants named in the
   containers (`c_uvAtlas` reg 168, `c_lightMapDarkColour` 184, `c_lightMapLightColour` 185, `c_constant0/1/2`, `c_fogcolour`, `s_sampler0..4`) tell
   which constants each pixel shader reads; a texture-coordinate scale may be `c_constant*`.
3. **`.data` pools** referenced by the `0x16` ranges (per-draw constant tables / "pool" fixups).
4. Cross-check with FM3/FM4: their `rmb` models store the uv offset/scale explicitly per mesh (`uv_offset_scale` in `fh1_formats::rmb`); FM2 may
   have an older equivalent in the draw's constant block.

### Acceptance
A per-draw (or per-material) scale such that, with no heuristics: Nurburgring start shows the textured signs, conifers and chain-link fences
**and** the apron grid; Silverstone start shows the repeating banner logos, kerbs and terrain. Then delete `tiling_scale` and the `FM2_UV_SCALE=auto`
mode. Other symptoms to re-check after the fix: the Laguna Seca overhead sign (stripes at ×32), far hillside backdrops that look horizontally
stretched (Silverstone/Nurburgring horizon bands).

## Reproduce
```
cargo build --release -p fh1setup                                          # default features (fm2 needs fm4 + fh2)
FM2_TRACKS=nurburgring ./target/release/fh1setup.exe import-fm2 data/work_fm2/disc --only maps     # one track; omit FM2_TRACKS for all 12 (~1 min)
./target/release/fh1setup.exe data/work/disc --only remaster --force      # rebuild the remaster tables — run after EVERY import
cargo build -p fh1-engine --no-default-features                            # engine (the default `rtx` feature needs the DLSS SDK); ~3 min first time
```
Screenshot (PowerShell: set `$env:...`; each run is ~25 s, scenery streams slowly so use `FH1_SHOT_AT` 20+):
```
FH1_OWNERSHIP=0 FH1_SHOT=out.png FH1_SHOT_AT=25 FH1_TELEPORT=x,y,z,yaw ./target/debug/fh1-engine.exe --track fm2/nurburgring_00
```
- `FH1_TELEPORT` pins the car (it also skips the launch menu). Values are the engine-space position (the same numbers as the model space the
  importer prints). Starts: Silverstone `34.9,152.5,-795.6,270`; **Nurburgring `2017.3,548.3,-1910.8,57` (black sign ahead-right, ~33 m, above the
  horizon)**; Laguna Seca `-353.0,1.6,-270.4,33`; Mugello `366.6,-130.4,687.8,-152`; Sebring `-223.9,-0.7,213.4,-88`; Sunset Infield `60.4,0.0,285.1,-90`;
  New York `-276.5,2.5,-337.0,-10`. Any layout: from its `ai/line.json` `points[0]`, `yaw = atan2(-(p1.x-p0.x), -(p1.z-p0.z))` degrees.
- The game launched from Claude Code skips menus (`CLAUDECODE` env); `FH1_LAUNCH_SCREEN=1` forces the title screen. `FH1_RENDERER=faithful`
  currently panics (remaster system `collect_small_casters`); stay on the default remaster renderer.
- To A/B the scale: `FM2_UV_SCALE=1` (or any number) on the import, rebuild the remaster tables, screenshot the same teleport.
- Discs are already extracted: FM2 at `data/work_fm2/disc`, FH1 at `data/work/disc`. Output:
  `data/installations/<id>/assets/private/imported/fm2/<track>_NN/scenery/` (tiles + `materials.json` + `textures/`); remaster tables under
  `.../private/remaster/imported/fm2/<track>_NN/scenery/`.

### Debug env vars (importer, `crates/fh1setup/src/fm2_scenery.rs`)
`FM2_UV_SCALE` (number, or `auto`), `FM2_AT=x,z[,radius]` (draws near a point in model space: model number, bbox, triangle count, textures with
mean brightness, **raw uv span, world area and metres² per uv unit**), `FM2_WEDGE=camx,y,z,lookx,y,z` (small draws in a view cone, for
finding what you see on screen), `FM2_UNRES=1`, `FM2_NAMES=1` (material-name histogram), `FM2_DEBUG`, `FM2_NO_FLIP`, `FM2_NO_BLEND`, `FM2_NO_KEY`,
`FM2_LIGHTMAPS=0`, `FM2_SHADE`, `FM2_LM_FLOOR`. Engine: `FH1_RM_LM_LIGHT_EXPOSURE` (1.0), `FH1_RM_LM_LIGHT_NOLM` (0.75), `FH1_RM_LM_LIGHT_FLOOR`
(0.12), `FH1_RM_LM_LIGHT_GAIN/BIAS` (old fallback), `FH1_STATIC_WORLD=0`.

### Where the code is
- `crates/fh1-formats/src/fm2.rs`: PVS + model parser. `Draw::uv_at` (signed uv), `uv_unorm` (lightmap uv), `uv_offsets`, `lightmap_offset`, the
  stream loop (`parse_model`: only types `0x2e`, `0x2d`, `0x01` are read — **the rest of the stream is the place to look**).
- `crates/fh1setup/src/fm2_scenery.rs`: `fixed_scale` / `tiling_scale` (the thing to replace), material selection, blend + lightmap materials,
  `convert_lightmaps`, `black_keyed`, `cutout_texture`.
- `crates/fh1setup/src/remaster.rs` (`fm2_light`, FM2 branches) and `crates/fh1-remaster/src/material.rs` (`FLAG_LM_LIGHT` shader code).

## Everything else (done, so you don't redo it)

**Uncommitted file list:** new `crates/fh1-formats/src/fm2.rs`, `crates/fh1setup/src/{fm2,fm2_scenery}.rs`,
`crates/fh1-formats/examples/{fm2_uv,png_diff,dds_stats}.rs`, `crates/fh1-shaders/examples/{fm2_gpu_shaders,fxobj_samplers}.rs`, `FM2_HANDOFF.md`;
edited `crates/fh1-formats/src/{caff,lib,pvs}.rs`, `crates/fh1-remaster/src/material.rs`, `crates/fh1-engine/src/{scenery,track}.rs`,
`crates/fh1-world/src/surface.rs`, `crates/fh1setup/src/{fm4,main,remaster,scenery}.rs`, `crates/fh1setup/Cargo.toml`, `README.md`. (Check `git status`.)

**State:** 32 layouts import (collision, AI/spawns, textured scenery, lightmaps); verified in game on Silverstone, Laguna Seca, Mugello, Sebring, Sunset
Infield, Nurburgring, New York. Cars are NOT converted (older carbin layout, very different gamedb tables).

**Format facts**
- Models `<track>out.NNNNN.rmb.bin`: CAFF, 0x190 header, sections `.gpu` (vertex + u16 triangle-list index buffers), `.data` (vertex records
  `[stride, ptr, gpuoff, size]`, index records `[key, gpuoff, bytes, 1]`), `.stream` (draw commands `00 len type 00`, len includes the header, start at u32@0).
- Draw = `0x2e` (word1 = `.data` offset of the vertex record; later draws reuse it), `0x2d` texture bindings (word1 hi+lo = count; `(item, slot<<16|n)` pairs
  from word 2; pairs with low word 0 are probably "global" textures), `0x01 [4 = tri list, index count, key]`.
- PVS (v0xB): 28-byte texture records (may sit at an odd byte offset: scan every byte), then per-model records: N, N items, 12 words, 1 marker word. `item`
  indexes the model's list to a PVS texture index (items past the list: unknown, see below). Flags: 4 `.bix` pair, 0 `.bin`, 1 runtime (`.stx.bin` entry),
  8/9/12 lightmap atlas tiles (word1 = `.stx.bin` page entry, `[su, sv]`, `[ou, ov]`, `su < 0` mirrored), 20 other.
- Vertices: float3 position, then 16-bit uv pairs. Stride 16 has a packed normal at +12; 20/24/28 have none. +12 pair = signed (mask ×1 / colour); the
  lightmap pair is unorm `u16/65535`, 0..1 over its tile, chosen by layout (`Draw::lightmap_offset`: stride 20/24 = last word, 28+ = best-fitting whole word).
- Lightmap pages: `<track>.stx.bin` (`caff::parse_stx`, four bugs fixed: unmapped format, variable descriptor stride, stacked textures, padding).

**Rendering (remaster, `FLAG_LM_LIGHT`)**: FM2 lightmap = the lighting: `albedo * max(lm*(light-dark)+dark, floor) * exposure`, dark/light from each map's
`tracks/fx_globals.json` `V2LightmapColor1/2`; draws without a lightmap tile use lightmap value `FH1_RM_LM_LIGHT_NOLM` (otherwise they render black when the
smoothed normal faces away from the sun). Terrain blends use `h_blnd2_ao_lm` (A, B, mask, AO(5), lightmap(7), lightmap uv on UV2, `uv_set[LM] = 2`).
Cut-outs: `h_diff_opac_1(_lm)` for black-keyed DXT1 foliage (`black_keyed`, >=35 % pure black becomes alpha 0) and BC2/BC3 textures 3..97 % transparent
(`cutout_texture`); fully transparent alpha (unused X8 channel) stays opaque. The P12 static world doesn't draw FM2 tiles (cause unknown), so
`imported/fm2` is skipped there (`fh1-engine/src/scenery.rs`).

**Tried and dropped:** `FM2_UV_SCALE=auto`; deciding cut-outs from the model's `shader_...` material-name strings (artist names, not per draw);
reading texture items past a model's PVS list as direct PVS indices (added wrong textures to blends, 928 blends on Silverstone, black infield).

## Other open work (lower priority)
- **Roads (partly understood):** a road draw's `.stream` has TWO `0x2d` records; the first binds the pixel shader's samplers
  (`[12, 13, 0, 1, 2, 3]` = two system textures (the shadow maps, items past the model's list), then lightmap + three textures), a second re-binds
  slot 0 (`[0]`). The parser used to keep the last, which made every road lightmap-only; it now keeps the FIRST (`parse_model`). Silverstone's start
  road (model 41) then binds three **BC4 masks** and no colour texture: 114 (2048x512 racing-line / tyre-wear streaks laid over the whole road, uv at x1),
  105 (512x512 fine asphalt grain, tiling) and 106. Its colours are the PS constants of the first `0x06` record (`0 4`: c0 = (1,1,1,1), c1 = 0.039 grey,
  c2 = 0.3, c3 = 0.7; shader `c_constant0..4`). So a road is a 3-colour blend driven by those masks. The importer still draws these as a flat
  grey (118,118,124), which happens to be about the mid of c1 and c2; a faithful road needs the constants parsed per draw (`Draw` doesn't keep `0x06`
  yet) and a road material (FH1's `h_road_*` / the remaster Road layering use vertex colours, which FM2 doesn't have, so probably a baked texture in mask space).
- 3-layer blends / multi-mask draws still show one texture (`h_blnd3_*` exists in FH1). Real winding (render-state `0x08` cull records) and normals instead
  of the up-facing-majority heuristic. Brightness defaults are by eye; grass reads yellow-ochre (FM2 TrackSettings v27 is warm).
- Fix the P12 static world for FM2; props / LODs / zones are not handled; **FM2 cars**.
- Layout names are done: FM2's gamedb `_&n` keys carry file hash 6547 (no table's stem hash) and the low 16 bits are the id in `Tracks.str`;
  `StringTables::alias_hash(6547, "Tracks")` in `fm4::strings`, venue names from `fm2::venue_name`, the venue prefix is dropped when the layout name already starts with it.
  Probe: `cargo run -p fh1-ui --example gamedb_str -- data/work_fm2/disc EN <n>...`.
- Pit apron on Nurburgring is a see-through grid (its texture is partly transparent: probably real, unverified).

## Gotchas
- Bash heredocs mangle backslashes and null bytes: write files with the editor tools. Shader paths in `materials.json` use `/`.
- The importers assume case-insensitive paths (Windows).
- Probe examples: `fm2_uv`, `dds_stats`, `png_diff` (`fh1-formats`), `fm2_gpu_shaders`, `fxobj_samplers` (`fh1-shaders`); run with `cargo run -p <crate> --example <name> -- ...`.
