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
   microcode sits in the `.gpu` tail, each shader ending `4e 4a 0000` + 8-byte GUID. `crates/fh1-shaders/examples/fm2_gpu_shaders.rs`
   extracts and disassembles it but its GUID lookup misses Nurburgring models: **that is because Nurburgring's `.gpu` tail has no `4e4a0000`+
   GUID terminator at all** (nor do its models contain the string `.updb`), so there is nothing to pair against — the code is still there and
   decodes. `crates/fh1-shaders/examples/fm2_tail.rs` disassembles an arbitrary `.gpu` byte range as microcode for exactly this case
   (`cargo run -p fh1-shaders --example fm2_tail -- <bin.zip> <model> <start_hex> <end_hex> [vs|ps] [align]`); it already shows a real
   `vfetch_full` and `dp4 r0.y, r3.wzxy, c1.wzxy` in model 22's tail. The open part is the microcode **start offset** (the container's own code
   is a stub, so `code_start = virtual_size + physical_offset` lands on zeros; the tail begins after the vertex/index buffers). Once the start is
   right: the vfetch for the texcoord carries `format` / `normalized` / `signed` / `exp_adjust` ([`fh1_shaders::ucode::decode_fetch`]) — a
   `SHORT2` vs `SHORT2N` / `exp_adjust` difference of 5 (2^5 = 32) between a ×1 and a ×32 model is the answer; else look for a `mul`/`mad` on
   the texcoord with a literal. A vfetch in the VS has `exp_adjust`; a `mul`/`mad` on the texcoord input would be the scale. The PS constants
   named in the containers (`c_uvAtlas` reg 168, `c_lightMapDarkColour` 184, `c_lightMapLightColour` 185, `c_constant0/1/2`, `c_fogcolour`,
   `s_sampler0..4`) tell which constants each pixel shader reads; a texture-coordinate scale may be `c_constant*`.

### Best per-draw discriminator found (2026-10-13)
A `.stream` `0x05 [ptr, 0x480004]` record is present on the **tiled** draws and absent on the **card** draws:
Nurburgring apron 22 and 23 (×32) and Silverstone terrain model 10 (×32, draw 1) have it; tree 456 and leaf cards 431 (×1) do not.
(Nurburgring fence 76 has it but its raw uv is a constant, so it is indeterminate.) It is the cleanest signal separating the two classes so far.
`ptr` is a `.data` offset: model 22 `ptr=0xf0` -> `.data[0xf0..0x104] = {0xf30, 0x0, 0x0, 0xebc}` (two sizes/offsets); Silverstone 10 `ptr=0x310`.
What the `0x05` record *means* (and whether the target data carries the scale) is **not decoded** — decode the `0x05` target next, and check it on
more labelled draws (a sign model, the Silverstone banners) to confirm the correlation holds.

### Ruled out (2026-10-12/13)
None of these differ between a known ×1 draw and a known ×32 draw (Nurburgring model 456 tree vs model 22/23 apron); the scale is **not** stored
as a per-draw or per-texture float:
- The `0x2e` vertex-fetch records: byte-identical except the 5th word (the vertex byte size). No format field there.
- `.data` and `.stream`: no big-endian float `32.0` anywhere.
- The PVS texture record's f32 pair (`[su, sv]`): `1.0, 1.0` for both the apron (file 22) and the tree (file 7). Not the scale.
- The `.data` rendergraph header floats at 0x50/0x60/0x70: three `(1,0,0,0)` for both.
- The apron PS reads `c_constant0` (reg 0); the stream's `0x06` sets it to `(1,1,1,1)`, not 32.
- The `.bix` texture header: it is `BIX0`, `u32 width, height, mip_count, format word, total_bytes, base_bytes` ([`fh1_formats::bix`]).
  The apron texture is 256×**32** (the `0x20` at header +4 bytes is its height, not a scale) and the tree is 512×256 — no uv scale field.
- The raw uv values themselves: the apron's +12 pair spans ~0.99×0.82 and the tree's ~0.14×0.80 (both 0..1 after the signed read), so the
  multiplier really is external to the vertices.
- The pixel-shader constants: terrain (Silverstone model 10, a verified ×32) sets `c0=(1,1,1,1)`, `c1=(0.65,0.65,0.65,1)`, `c2=(0.35,0.35,0.35,1)`;
  the apron (×32) sets only `c0=(1,1,1,1)`; the tree (×1) sets `c0=(0,0,0,0)`. None is a ×32 scale, so it is not a PS constant.
- The `.data` per-draw ranges referenced by the `0x16` records (handoff lead 3): byte-identical between the apron and the tree except each model's
  own offsets (0x01a0 / 0x11b0 vs 0x0234 / 0x1bf4). No scale.
- A real vertex shader: the VS blocks are stubs on **both** Silverstone (all VS containers share GUID `f249031746a609a4`) and Nurburgring
  (the tail holds only the VS-like `00 00 00 00 10 02 c2 00` block, which decodes to a `vfetch`-less pass-through). `fm2_vfetch_scan` over the
  whole `.gpu` of model 22 finds **no stride-20 vfetch**. So the scale is probably not on the disc as shader code.

The handoff's lead was the VS microcode (vfetch format/exp) or a multiply by a literal. **But note: every `.data` VS container
shares one GUID (`f249031746a609a4`) — there is a single vertex shader for all FM2 draws — so the scale cannot be per-draw inside the VS.** That points
instead at the per-draw **state** records in the `.stream`, which are exactly where the two models differ and which nobody has decoded yet:
`0x05 [ptr, 0x480004]` (apron only), `0x08 [ptr, 0x10001]` (render state), `0x0c [addr, value]` (tree only: `[0x150,1]`, `[0x154,0x97]`), `0x19`,
`0x2a`, `0x1d`. A per-draw sampler / texcoord-scale state (a texture-tiling count or an address/wrap mode) would live there. **Decode the `0x05` /
`0x08` / `0x0c` records next** — diff apron (×32) vs tree (×1) gives a small, focused set. Note when reading the fetch:
`fh1_shaders::ucode::decode_fetch` splits the fetch into `format` (the `vfmt` table) and a separate `normalized` bit (`b>>13`, 1 = normalised)
plus `exp_adjust` (2^exp multiply) — the `SHORT2`-vs-`SHORT2N` (÷32767 vs ÷1024-ish) difference is the **`normalized` bit** and/or
`exp_adjust`, not the `format` number, so compare those between a ×1 and a ×32 model. Two probes are added for that:
`fm2_tail` (disassemble a `.gpu` byte range) and `fm2_vfetch_scan` (scan candidate start offsets with the real control-flow decoder and
print each `vfetch`'s format/sign/normalised/exp/stride). Note: **Nurburgring model 22's `.gpu` contains no stride-20 vfetch at all**, so its VS
is not in that file — it is shared or runtime, and the scale cannot be read from any single model. Locate the shared VS first (scan the other
models / the `pre_*` and `sky` models for a genuine stride-20 vfetch with a small offset, or find an explicit shader cache on the disc).
   **Shader-location findings from the Silverstone model 10 probes (`ss2_10.rmb`; not verified on Nurburgring):**
   - File layout: 0x190-byte CAFF header, then `.gpu` (0x3de0 bytes in model 10), `.data` (0x43e4), `.stream`. In `.gpu` the vertex/index buffers end at
     0x3150 and the shader code occupies 0x3150..0x3de0 (the "tail").
   - **Pixel shaders in the tail:** each is the 12 bytes `3d e9 78 d5 3e 99 16 87 3f 16 45 a2` (a fixed marker), then 4 bytes of flags
     (`00 00 00 00`), the control-flow words, then the ALU / fetch code, then `4e 4a 00 00` + the 8-byte GUID. The shader's GUID is the last 16 hex digits
     of the `.updb` path inside its `.data` container (`e:/ShaderPDBs/82771/65/65f96184-8551308f.updb` -> `65 f9 61 84 85 51 30 8f`). A shader's
     code runs from just after the previous shader's GUID (+ its 16 bytes of padding) to its own `4e4a0000`. `fm2_gpu_shaders` does exactly this.
   - **Vertex shaders are different:** their blocks have no 12-byte marker (`00 00 00 00 10 02 c2 00 12 ...`), are ~0x30 bytes, and **every VS container in
     the file shares one GUID (`f249031746a609a4`)**, so the same tiny block is found for all of them. That block disassembles to a pass-through
     (`vfetch`-less: nops, `alloc`, `mark_vs_fetch_done`); it is NOT the real vertex shader. Treat the `.data` VS containers as stubs, as noted above.
   - **Which container a draw uses:** the `.stream` type-`0x02` record's single word is the pixel-shader container offset in `.data` **minus 0x28**
     (model 10: records `0x518, 0x10a4, 0x1cfc, 0x2934` -> containers at `0x540, 0x10cc, 0x1d24, 0x295c`, the four `0x102A1100` containers). The
     type-`0x19` records after it hold the next record offsets, not shader data. The VS container for a draw is presumably the matching `0x102A1101`
     one but every VS shares the stub GUID, so that link does not lead to code.
   - Shaders look shared, not per model: the road pixel shader (Silverstone model 41) declares `s_shadowMapNear0` / `s_shadowMapFar0` (samplers 0, 1),
     `s_sampler2..5`, `c_constant0..4`, `c_specularLevel` (reg 5), `c_uvAtlas` (168, x3), `c_lightMapDark/LightColour` (184 / 185). Samplers 0 and 1 being the
     global shadow maps is why a road's `0x2d` record starts with two out-of-list items (12, 13). Terrain draws (model 10) use PS with `s_sampler0..3`
     (lightmap, mask, two colours) and `c_uvAtlas`, `c_lightMapDarkColour/LightColour`, `c_fogcolour`, `c_constant0`.
   - What I did not find: a vertex shader with a texcoord scale or a `vfetch` exp-adjust, the microcode start offset on Nurburgring (no terminator
     there), or any UV scale in the pixel shaders. My own early disassembly slice was wrong (garbage control flow), so ignore any "709 dwords" output from it;
     the saved `fm2_gpu_shaders` / `fm2_tail` examples are the reliable tools.
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
- **Roads (baked, approximate):** a road draw's `.stream` has TWO `0x2d` records; the first binds the pixel shader's samplers
  (`[12, 13, 0, 1, 2, 3]` = two system textures (the shadow maps, items past the model's list), then lightmap + three textures), a second re-binds
  slot 0. `parse_model` keeps the FIRST (the old last-wins made every road lightmap-only). Silverstone's start road (model 41) then binds three
  BC4 masks and no colour texture: 114 (2048x512 racing-line streaks over the whole road, uv x1), 105 (512x512 asphalt grain, tiling), 106. `Draw::constants`
  now keeps the `0x06` pixel-shader constants (`word1 = start register << 16 | count`, then vec4s, state carries over between draws): road draws set
  c0..c3 (`c_constant0..3`) + c5. They vary per track: Silverstone c1 0.039 / c2 0.3 / c3 0.7, Sebring c1 0.02 / c2 1.4 / c3 a blue tint, Mugello c1 0.08.
  `fm2_scenery.rs` bakes each colour-less mask draw into a 256x256 texture: `c1 * FM2_ROAD_GAIN (2.0)` modulated by the grain mask (`FM2_NO_ROAD=1`
  disables; `FM2_ROADS=1` prints the road draws with their constants). **UNVERIFIED:** which constant is what (c1 as the asphalt is a guess from the
  per-track values), the wear-streak mask (114) is not used, and c2/c3 (paint / tint?) are ignored. Result: Silverstone asphalt is now dark instead of
  the old flat mid grey. A faithful road needs the real pixel-shader code (see the UV-scale section for the microcode extraction).
- **CLM draws (colour + lightmap + one mask) = road strips with the racing-line wear mask (done, `Draw::separate_mask`):** the mask here is
  the 2048x512 wear-streak BC4 (Silverstone file 274), not a blend between two colours. These stride-24 draws carry THREE pairs: `@12` colour tiling
  uv (tiny span, x32), `@16` the lightmap (unorm, reaches 1.0 on the long axis) and `@20` the mask uv (signed, 0..1 across the strip; read as unorm it
  only reaches 0.5). The old "lightmap = last word" rule picked `@20` for them. They now draw as `h_blnd2_ao_lm` with A = the colour (an asphalt strip
  texture), B = A darkened (`FM2_WEAR_DARK`, default 0.55, a guess: rubber), mask = wear uv, lightmap on UV2 (`FM2_NO_WEAR=1` disables;
  `FM2_PATTERN_AT=CLM` lists draws by texture pattern, `FM2_PAIRS_MODEL=<n>` prints every uv pair's span). Silverstone's pit lane shows convincing streaks.
  Whether wear should darken or lighten is unverified. The same three-pair layout may exist on other strides: only stride 24 is handled.
- **Wear blend generalised (other mask patterns, 12 tracks):** pattern histogram over all tracks (`FM2_PATTERNS=1`; L lightmap, C colour, M BC4 mask):
  `LMM` 2896 (roads, baked), `LMCC` 1229 (two-colour terrain blend, done), `LCM` 931, `CLM` 793 (done), `CLMM` 397, `MLMC` 373, `LMMMM` 372, `CM` 315,
  `M` 232, `CCM` 218. Rule now (`fm2_scenery.rs`, pre-pass `wear_mask`): a draw whose base is ONE colour texture or the baked road, plus a mask of at least
  512x512 (the largest one = the wear; the shared 512x512 grain loses to a 2048x512 / 512x1024), is drawn as the wear blend (base + darkened copy by the mask,
  `FM2_NO_WEAR=1` disables). Plain `CM` (colour + one mask, no lightmap) is left alone (it may be an alpha mask on a decal). `LMMMM` roads (lightmap, 2048x512
  wear, grain, ...) now get streaks on the baked asphalt. **Not handled / unverified:** `MLMC` (mask first: nobody looked), `LCM` on Sebring (concrete: mask
  0xa3 512x512 is used as wear but nothing visibly changed at the start line), `CLMM` colour is a 64x256 edge strip, the 512x1024 mask per segment is
  assumed to be wear, whether any of these masks are really opacity (decals) is unknown.
- **`MLMC` / `MC` / `MMC` = tree sprites (done, Maple Valley's autumn trees):** the first mask (512x512 BC4) is a sheet of leaf / branch silhouettes (the
  OPACITY), the one colour texture is a smooth 128x128 gradient stretched over the sprite (autumn tint), and a second 256x256 mask (a shading / landscape map) is ignored.
  `fm2_scenery.rs` (`sprite_ids`) composes one RGBA texture per (mask, colour) and draws it as a cut-out card with uv x1 (cards are x1, see the UV-scale problem;
  `FM2_NO_SPRITE=1` disables). Detection: first usable candidate is a mask >= 256x256 and exactly one colour <= 128x128. So masks CAN be opacity: the plain
  `CM` pattern (colour + mask, 315 draws on all tracks, mostly Maple Valley) is the same sprite with the mask SECOND: a 128x128 autumn-gradient colour and a
  64x64..128x128 BC4 silhouette (Maple Valley file 701); the sprite rule now accepts one colour <= 128x128 plus one mask of 64x64..255x255. **Not handled:**
  `CM` draws whose "mask" is 32x32 (flags 1, a runtime stx texture, e.g. Maple Valley 0x2ad / 0xe4 on 80-150 m terrain pieces): that size is a
  lightmap crop, so those models carry their lightmap as a plain runtime texture (no flags 8/9/12 tile) and the draw's uv pair for it is unknown; and `CM`
  with a 512x256 colour + 128x64 mask (Maple Valley model 143 etc.), which is not a sprite. Some far trees on Maple Valley still render whitish.
- **Small runtime BC4 textures = per-model lightmaps (`small_lm`, first cut, UNVERIFIED visually):** a texture with PVS flags 1 (a `.stx.bin` entry), BC4 and
  <= 64x64 (32x32 on Maple Valley, e.g. file 0x2ad) is a model's own lightmap carried as a plain runtime texture instead of a flags 8/9/12 atlas tile. They
  are excluded from the mask / colour classification (`mask()` and every `usable` closure), replicated to RGB like the cropped tiles, and bound as the
  lightmap with the draw's lightmap uv (`lightmap_offset`, now also computed for draws with any flags-1 candidate). Open: which uv pair they use (the
  unorm "last word" rule is a guess; several of these draws have no pair reaching 1.0 in unorm, so they may use a signed pair or a sub-window of
  the 32x32), and whether they are lightmaps at all (alternative: a low-res macro / AO modulation). `FM2_NO_SMALL_LM=1` restores the old reading. I could not
  get a trustworthy in-game view of the affected terrain slabs (the teleport kept landing on the collision plane).
- **Winding and normals (investigated, no decoder found):** the importer flips a draw when most of its up/down-facing triangles face down
  (`fm2_scenery.rs`), then the remaster culls back faces. Stream state records between draws (`fh1_formats::fm2::Draw::state`: `0x05`, `0x08`, `0x0c`, `0x1d`)
  do NOT separate the cases: `cargo run -p fh1-formats --example fm2_winding -- <bin.zip>` prints, per state signature, how many stride-16 draws (the only ones
  with a packed normal at +12) have triangles agreeing with their normals: identical signatures (`0c:150.1 0c:154.97 ...`) give 74 agree / 54 flipped on Maple
  Valley. Those normals are also unreliable on foliage cards, so the "truth" is noisy (only ~60 conclusive draws outside Maple Valley); a signed-volume
  test agrees no better. Normals do not matter for FM2: its lightmap is the lighting and `FLAG_LM_LIGHT` materials are unlit (lightmap-less draws use
  `FH1_RM_LM_LIGHT_NOLM`). Two-sided drawing (`FM2_TWO_SIDED=1` at the remaster build) shows the bridge undersides the heuristic culls but also back copies of
  double-layer roads, which z-fight (banded roads on Nurburgring), so it is off. Open: a per-draw cull decision (the stream's `0x0c [0x150, 1]` / `[0x154, v]`
  writes, with v = 0x97 / 0xaa / 0x87, are the only per-draw state seen and are untested against cull) and what the double road layers are for.
- 3-layer blends / multi-mask draws still show one texture (`h_blnd3_*` exists in FH1). Real winding (render-state `0x08` cull records) and normals instead
  of the up-facing-majority heuristic. Brightness defaults are by eye; grass reads yellow-ochre (FM2 TrackSettings v27 is warm).
- Fix the P12 static world for FM2; props / LODs / zones are not handled; **FM2 cars**.
- Layout names are done: FM2's gamedb `_&n` keys carry file hash 6547 (no table's stem hash) and the low 16 bits are the id in `Tracks.str`;
  `StringTables::alias_hash(6547, "Tracks")` in `fm4::strings`, venue names from `fm2::venue_name`, the venue prefix is dropped when the layout name already starts with it.
  Probe: `cargo run -p fh1-ui --example gamedb_str -- data/work_fm2/disc EN <n>...`.
- Pit apron on Nurburgring is a see-through grid (its texture is partly transparent: probably real, unverified).

## FM2 cars: scoping (nothing implemented; first recon only)
Tried `import-fm2 --only cars` with the FM3 / FM4 importer wired in (reverted, it fails at once): `no such table: List_UpgradeCarBody`, and `Shared.zip`
textures are absent. What a real importer needs, in rough order of risk:
1. **The carbin is a different, older format.** `Media/cars/<car>.zip` holds `<car>.carbin` (3.9 MB for `acu_nsx_05`, type word 2, then a block of
   floats), `damage.xds`, `nodamage.xds`, `lights.xds`, `damagelights.xds`, `LiveryMasks/*.xds` (4 MB each), `Physics/MAXData.xml`, `CarAttribs.xml`,
   `shaderSettings.xml`. `fh1_formats::carbin::parse` fails ("no section table found"; try `cargo run -p fh1-formats --example carbin_try -- <file>`).
   Seen so far: a list of parts, each as `u32 length + name` (`glassRTL`, `bumperFrameR`, `taillightL`, ...), then per-LOD material records (`u32 len + name`:
   `detail_glass_red`, `textured_reflector`, `body`, `chrome`, `grille1`, `plastic2`, `carbon_fiber`, `race_kit_red`), then u16 index buffers that are
   **triangle strips with `0xFFFF` restarts** (not lists); the section / index area is ~0x6b000 bytes and the rest (to `cv7` at 0x29afe2) should be vertex
   pools whose stride and packing are unknown. Decode: section header fields (counts around `00 00 01 57 01 00 00 00 01 ...` before the first name), the vertex
   stride, then convert to the existing `.fxcar` / glTF path (`fh1setup/src/model.rs`, 847 lines, expects the FM3 / FM4 `Carbin` struct).
2. **The gamedb is organised differently but has the data.** FM2 `Data_Car` (310 cars, 159 columns), `Data_Engine` (321), `List_TorqueCurve` (1650, by
   `EngineID`), `List_Upgrade*` tables keyed by `Ordinal` = car id (Engine, Transmission, Differential, Clutch, Brakes, TireCompound / Width, RimSize,
   SpringDamper, AntiSway, Weight (Mass, CMHeight, CMBackFront), FrontBumper / RearWing aero ids ...), `List_TireCompound`, `List_TireFrictionCurve /
   MultiCurve`, `List_Wheels`, `Combo_Engines`. It has no `Data_CarBody` / `Data_Drivetrain` / `List_UpgradeCarBody` / camshaft tables the FM4 importer
   (`cars.rs` `car_physics`) joins on, so `physics.json` (the engine's `data.rs` reads `car`, `body.Pristine*`, `weight.Mass/CMHeight/CMBackFront/BlockDim*`,
   `trans.GearRatioN/FinalDriveRatio/NumGears`, `torque_curve.samples/torque_scale_nm/max_rpm`, `cam.RedlineRPM/StallRPM`, springs, tyres, `maxdata`, `camera`)
   needs a dedicated FM2 mapping; some keys (`SimTopSpeed`, `SteerMaxAngleFiltered`, aero, block dims) may be missing and need donors or defaults.
3. **Shared pieces:** `Media/cars/Driver`, `GlobalCarAttribs.xml`, `ShaderSettings`; textures are `.xds` (the `xds` decoder exists); FM2 car shaders are
   FM2's, not FH1's `.fxcar` ones, so a first version would map FM2 materials (`body`, `chrome`, glass, lights) onto the FM4 car shaders as the FM3 import does.
   **Carbin recon (`acu_nsx_05.carbin`, big-endian; probes were Python in the session scratchpad, not saved; the facts below are what a parser needs):**
   - File: `u32 type = 2`, a header of node data (four 16-float blocks at 0x04.., then 0x2ca / counts and rows like `1.0 0 -0.551 0.129 2.152 ...`: attachment points
     such as a lamp at (-0.55, 0.13, 2.15) and its mirror; tables of `u16` node pairs), then **55 sections** (parts: `glassRTL`, `bumperFrameR`, `taillightL`,
     `exhaustRa` ... ) from ~0x1ff0 to 0x6b59a, then two vertex pools to the end of the file.
   - **Section header** (immediately before `u32 name_len + name`): `... u16 first_vertex, u16 last_vertex, u8 1, u32 1` — the section's vertices are
     `first..=last` of the pool (consecutive sections tile it: bumperFrameR 0..498, taillightL 499..1092, taillightR 1093..1686, ...). After the name:
     `u32 2, u32 1, u32 0, u32 0x3c` (a pool header with count 0 here), then per material `u32 1, u32 N, u32 1, u32 3, u32 len + material name`
     (`detail_glass_red`, `textured_reflector`, `chrome`, `black`, `body`, `grille1`, `carbon_fiber`, `race_kit_red` ...) and, per LOD, index records
     `u32 6, u32 lod_tag (1..5 = LOD0..LOD4), u32 3, u32 index_count, u32 2` + `index_count` big-endian u16: triangle STRIPS with `0xFFFF` restarts. Indices are
     **local to the section** (0-based; add `first_vertex`). The same material list repeats once per LOD tag with shorter index lists. 895 index records in this file.
   - **Pools** (scan for `u32 2, u32 1, u32 count, u32 stride`): pool A at 0x6b599 (8810 x 60 bytes) and pool B at 0xec689 (49843 x 60). Layout of each pool:
     `16-byte header, 4-byte sentinel 0x000DECAF, count x 60 bytes, 4-byte trailer (crc?)` (so the vertex data starts at 0x6b5ad / 0xec69d: an earlier note that put it
     4 bytes too early is wrong). The file ends with the `06d7fa50 000decaf` trailer of pool B.
   - **Vertex (60 bytes), DECODED part: bytes 0..8 = four big-endian HALF floats `x y z w`: `(x, y, z)` is a UNIT DIRECTION and `w` the length, so
     position = (x, y, z) * w in metres** (VERIFIED: every vertex has |xyz| = 1 +- 0.05, and pool B's cloud is an NSX: x +-0.911, y +-0.514, z +-2.07 m, side view shows the
     wheel arches). Same trick as FM3's `fm3_convert`. Section vertices come out part-local (a windscreen glass is 0.64 x 0.24 x 0.08 m, centred on 0): **the
     per-part translation is not found yet** (not in the section header; try the node table near the file start / the attachment points). Bytes 8..12: four u8
     (mean 129 / 87 / 129 / 106: probably a packed tangent frame or the uv), 12..16 always zero, 16..28 six i16 snorm (~+-1: quaternion / uv pairs, not yet
     identified), 28..30 a positive u16 (<= 0.28), 30..60 fifteen i16 snorm of falling amplitude (+-0.65 .. +-0.2: spherical-harmonic lighting coefficients).
     **UVs were not located** (candidates: the six i16 at 16..28 or the four u8 at 8..12).
   - **UPDATE (supersedes the "pool A only" / "translation not found" notes above).** Each section header is `u16 first_vertex, u16 last_vertex, u8 flag, u32 1`
     before the name: **flag 1 = the range indexes pool A (8810 vertices), flag 0 = pool B (49843)**; ranges tile each pool consecutively. Glass, interior and some
     bumpers use A, most other parts B. With that, every part decodes to its true size (body 1.82 x 1.03 x 4.14 m, seat 0.5 x 0.82 x 0.66, mirror 0.18 x 0.14 x 0.19).
   - **Per-part world centre:** a 9-float big-endian record `centre xyz, local min xyz, local max xyz` sits **exactly 0x55 bytes before the section's `u32 name_len`**
     (46 parts, extent matches the decoded part within 0.001; it can be at an odd byte offset, read unaligned). World position = part-local position + centre.
     Seven glass parts (`glassRTL glassLTL glassR glassLF glassRF glassLHL glassRHL`) do not have it there (zeros): their records are in the header float table
     (first at 0x10c8, centre (+0.443, 0.634, 2.067) = glassRTL; 0x2aaf = glassLTL at -0.443). Naming is consistent: **L = -x, R = +x** (steering wheel at x -0.422).
     The same record also shows up right after the PREVIOUS section's last index run (it is written between sections), so read it before the name, not after the runs.
   - **Car frame:** +z = REAR, -z = front (headlights at z -1.8, taillights / exhaust at z +2.0), y up from the ground, x = +-. Assembled 48 parts: bbox
     x +-0.949, y -0.079..1.034, z -2.281..2.252 (MAXData body bbox is x -0.88..0.86, y -0.025..1.0076, z +-2.08; mirrors and the rear wing explain the excess).
     `Physics/MAXData.xml` (Wheelbase 2.527, track widths, ride heights, 20 CollSpheres) is the ground truth for scale and physics.
   - **UV FOUND (verified on the hood, plausible elsewhere):** bytes **8..12 are two big-endian u16, offset-binary: `u = (u16@8 - 0x8000)`, `v = (u16@10 - 0x8000)`**
     (that is, the high byte is ~0x80-0x84 and the low bits carry the value: I first misread them as four u8 / half floats). Plotted as wireframe, `hooda`'s
     LOD0 triangles unwrap into a clean hood-shaped layout, and the uv-area / world-area ratio is tight (IQR 0.23 in log). Hood extents: u 372, v 1018 units for
     1.1 x 1.48 m, so the scale is NOT the same on both axes (likely u/512, v/1024, i.e. a 2:1 texture; or u/1024 with another aspect): settle it against the body shader / texture size.
     A few vertices per part have wild values (stray points; the same field with the high byte != 0x80..0x8f): probably a second encoding or damage data, clamp / skip them.
     Parts using `nodamage.xds` (a 1024 x 1024 DXT5 detail atlas: NSX lettering, Acura badge, grille mesh, interior dashboard, engine bay, tyre / valve detail) did not
     land on their atlas islands with u,v / 1024: unresolved (needs the per-material uv transform; `seatL` u 1048..1119, v 123..280 vs the leather island at px x 812..880, y 90..220).
     The body paint is the livery masks (`LiveryMasks/{front,back,left,right,top,wing}.xds`, 1024 x 1024 x 4 projections), not this atlas.
   - Failed uv probes (do not repeat): `i16@16..26` pairs, `i16@20/22` (smooth, axis-cross scatter, not a layout), half floats and f32 at 8..60, u8 pairs at 8..12.
     No field is unit length as a normal / quaternion; the normal encoding is still unknown (try the u8 bytes' remaining correlation or derive normals from geometry).
     `fh1-formats/examples/xds_png.rs` decodes an `.xds` to PNG (+ raw RGBA with a third argument).
   - Still unknown: the uv transform per material and the normal / tangent encoding. Candidates: bytes 8..12 (four u8), the six i16 at 16..28; pools carry 15 more i16 (lighting). Pool B vs
     pool A: is B the render mesh and A the glass / interior? (index runs are local per section, both pools are referenced, LOD tags 1..5 per material).
   - **Parser written:** `fh1_formats::carbin_fm2::parse` (+ example `carbin_fm2_dump <car.carbin> [out.obj]`) returns every part with its world centre, part-local
     vertices (position, uv = (u16 - 0x8000) / 1280), materials and LOD runs as triangle lists (run.lod 1 = most detailed). On `acu_nsx_05` all 55 parts
     parse, all get a centre (the 7 glass parts via the header-table search), and a side / top render of the assembled stock parts is an unmistakable NSX (arches,
     door, mirror, wing, hood). Variants are separate parts: `bumperFa..Fd/Frace`, `bumperRa..Re/Rrace`, `hooda..d`, `wing a..g/race`, `skirtLa..Ld` -> pick letter `a` for stock
     (the FH1 code has `Kit` for this). UV scale 1/1280 is supported by the seat (island 50 x 114 px vs uv span 65 x 138) and the dashboard (v .08-.26, u ends at 1.0);
     the u offset against the atlas is still ~20 px off, and normals are not decoded (compute from triangles).
   - **DONE: `fh1setup import-fm2 <disc> --only cars`** (`crates/fh1setup/src/fm2_cars.rs`, ~5 s for all 310 cars, 0 skipped) writes `imported/fm2/cars/<MediaName>/`
     (`model.gltf`/`model.bin`, `model.json`, `physics.json`, wheel / tyre PNGs) + `index.json`; the engine lists them in car select and drives them
     (`--car ../imported/fm2/cars/ACU_NSX_05`; verified in game: NSX, Ferrari 250 GTO, Peugeot 206 RC). Real: body parts (stock letter `a` of each kit family, no `race` extras),
     part centres, uv, weight / CM / gearbox / torque curve / redline / tyre sizes (gamedb) and MAXData (collision spheres, track). Approximated: flat paint / material colours
     by material name (no textures yet, uv offset into the `.xds` atlases unsettled), wheels copied from the closest FH1 car (mass, drive, year) scaled to the FM2 tyre size,
     and everything FM2 has no table for (tyre curves, aero, suspension, diff, camera) from that donor's `physics.json` (`donor` key). No `fx/` folder is written, so the engine
     keeps the glTF body. Car names: FM2 gamedb keys use the stem hashes 43337 (`Data_Car`) and 13795 (`List_CarMake`), aliased in `fm2_cars.rs`.
   - **Textures, first pass (VERIFIED in game):** uv = value / 1280 exactly (an earlier ~20 px offset was my leather bbox, not the transform). `lights.xds` (512 x 512, fully
     opaque) is the lamp atlas: `textured_reflector` and `detail_glass_red` parts land on their reflector / lens islands (wireframe overlay) and are textured in `fm2_cars.rs`;
     `nodamage.xds` (1024 x 1024 DXT5, alpha = used area) carries `interior` (seats show the tan leather in game); the shared `media/cars/shared/bumper_frame.xds` is bound to
     `bumper_frame`. Alpha coverage per material (fraction of vertices on opaque texels) was the quick test for which atlas a material uses. Still flat: paint (`body`,
     `body_2`, `badge`, `black`: livery masks `LiveryMasks/*.xds` are 1024^2 x 4 projections + dirt / sparkle shared textures), `chrome`, `window`, `glass`, `carbon_fiber`
     (`shared/carbonFiber.xds`), `rubber_trim`, grilles (`shared/grille1/2.xds`).
   - **Paint:** the livery masks are blank white (decal areas for the livery editor), so FM2 paint is a plain colour: `Combo_Colors` (Ordinal = car id, first Sequence,
     RGB 0xRRGGBB, Metallic) is used for every car now (verified: the 250 GTO is red, the NSX silver). `carbonFiber.xds` and `grille1.xds` from `media/cars/shared` are bound to
     `carbon_fiber` / `grille1`; lamp materials (`detail_glass_*`, `head_light`, `tail_light*`, `reverse_light`, `reflector`, `red_glass`) use `lights.xds`. Material names seen
     across the 310 cars: body, window, interior, bumper_frame, chrome, glass, carbon_fiber, rubber_trim, matte_colors, textured_reflector, detail_glass_red, black, misc, ...
   - **Material check (examples `fm2_mat_survey`, `fm2_uv_overlay <car.zip> <out.png> [nodamage|lights] [materials]`):** over all 310 cars only `interior` lands on `nodamage.xds`
     (91% of vertices on opaque texels; random uv on this atlas gives ~15%); `misc` is the next best (78%, a few points near the centre console on the RX-7) but its overlay is mostly
     scatter, and `matte_colors`, `frame`, `badge`, `emblem`, `weatherstrip` scatter over EMPTY atlas areas (RX-7 overlay), so they do not sample this atlas: they stay flat colours.
     The `emblem` / `badge` points form a band above the RX-7 "MAZDA" lettering island: the logos probably live in another texture (not `damage.xds`, which is the shattered-glass
     damage version of the same atlas; `shared/glass_texture.xds` is a cracked-glass damage texture too, not needed). Glass and chrome need no extra mapping: the engine's PBR path
     reflects the sky (the NSX rear window shows the orange sunset band), so the glTF `metallic` / `roughness` values are enough.
   Next: (1) find where `emblem` / `badge` / `misc` textures live, tyre and rim textures from FM2 (`tireA*.xds`), FM2 suspension / tyre tables, (2) FM2 suspension / tyre tables instead of the donor's
   (`List_UpgradeSpringDamper`, `List_UpgradeTireCompound`, `List_TireFrictionCurve`), (3) FM2 wheels (`Media/wheels`), (4) cockpit view, (5) normals / smooth shading check.

4. Plausible staging: (a) carbin parser + glTF so a car appears; (b) `physics.json` from FM2 tables with an FH1 donor car filling missing keys so the car
   drives; (c) real physics / upgrades. Each stage is a project of its own (a) alone is several hours of format work.

## Gotchas
- Bash heredocs mangle backslashes and null bytes: write files with the editor tools. Shader paths in `materials.json` use `/`.
- The importers assume case-insensitive paths (Windows).
- Probe examples: `fm2_uv`, `dds_stats`, `png_diff` (`fh1-formats`), `fm2_gpu_shaders`, `fxobj_samplers` (`fh1-shaders`); run with `cargo run -p <crate> --example <name> -- ...`.
