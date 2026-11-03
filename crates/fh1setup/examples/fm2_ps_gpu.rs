//! fm2_ps_gpu: run one FM2 pixel shader on the REAL GPU through the WGSL translator (`fh1_shaders::wgsl`) and print
//! the colour it outputs, over the same grid and with the same inputs as the CPU interpreter (`fm2_ps_run`), so the
//! two can be compared directly.
//!
//! This is plan v8 P1's premise check. The whole "reproduce FM2 by running the game's own shaders" plan rests on the
//! claim that the translated WGSL computes what the game's microcode computes. The CPU interpreter is the known-good
//! reference (it reproduces the Xenia pixel at the road anchors, MODLOG 230/234); if the GPU translation does not
//! reproduce the interpreter here, the plan is wrong and no further work should be built on it.
//!
//! `cargo run -p fh1setup --release --example fm2_ps_gpu -- <bin.zip> <model> <group> <textures dir> [options]`
//!   `--tex N=ID`   sampler tfN reads texture file ID (`<dir>/<ID as 8 hex>.dds`); repeat
//!   `--c R=x,y,z,w` float constant cR; repeat
//!   `--r R=x,y,z,w` interpolator register rR; repeat
//!   `--b ADDR=0|1`  bool constant; repeat
//!   `--scale S`    the uv grid covers 0..S (default 1)
//!   `--n N`        grid is N x N points (default 16)
//!   `--dump`       print every sample, not just the mean
//!
//! The output line is formatted identically to `fm2_ps_run` so the two runs can be diffed.
use fh1_formats::{fm2, zip::Archive};
use fh1_shaders::container::{Interpolator, ShaderBlob, Stage};
use fh1_shaders::fm2 as sh;
use fh1_shaders::wgsl::{self, Resolver};
use std::collections::{BTreeMap, HashMap};

fn be(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}
fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}

struct Tex {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

fn load_tex(path: &std::path::Path) -> Option<Tex> {
    let b = std::fs::read(path).ok()?;
    let u = |o: usize| u32::from_le_bytes(b.get(o..o + 4).unwrap_or(&[0; 4]).try_into().unwrap()) as usize;
    let (h, w, fmt) = (u(12), u(16), u(128));
    let mut rgba = vec![0u8; w * h * 4];
    let f = match fmt {
        71 => texpresso::Format::Bc1,
        74 => texpresso::Format::Bc2,
        77 => texpresso::Format::Bc3,
        80 => texpresso::Format::Bc4,
        83 => texpresso::Format::Bc5,
        28 => {
            rgba.copy_from_slice(b.get(148..148 + w * h * 4)?);
            return Some(Tex { w, h, rgba });
        }
        _ => return None,
    };
    f.decompress(b.get(148..148 + f.compressed_size(w, h))?, w, h, &mut rgba);
    if fmt == 80 {
        // One channel: replicated, as the sampler hands it to the shader (the same rule as fm2_ps_run).
        for p in rgba.chunks_exact_mut(4) {
            (p[1], p[2], p[3]) = (p[0], p[0], 255);
        }
    }
    Some(Tex { w, h, rgba })
}

/// Bindings for the translated module: constant registers come from a uniform, textures from a texture array.
#[derive(Default)]
struct GpuResolver {
    textures: BTreeMap<(u32, u8), ()>,
}

impl Resolver for GpuResolver {
    fn float_const(&mut self, _: Stage, reg: u32) -> String {
        format!("fx.pc[{reg}]")
    }
    fn float_const_rel(&mut self, _: Stage, base: u32, index: &str) -> String {
        format!("fx.pc[clamp({base} + {index}, 0, 255)]")
    }
    fn bool_const(&mut self, _: Stage, addr: u32) -> String {
        format!("(fx.b[{}][{}] != 0u)", addr / 4, addr % 4)
    }
    fn int_const(&mut self, _: Stage, id: u32) -> String {
        format!("fx.i[{id}]")
    }
    fn texture(&mut self, _: Stage, tf: u32, dim: u8) -> Option<(String, String)> {
        self.textures.insert((tf, dim), ());
        Some((format!("t{tf}_{dim}"), "samp".into()))
    }
    fn vertex_input(&mut self, _: u8, _: u8) -> String {
        "vec4<f32>(0.0)".into()
    }
}

fn parse4(s: &str) -> [f32; 4] {
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0), v.get(3).copied().unwrap_or(0.0)]
}

/// Minimal blocking executor: wgpu's futures complete on internal threads, so polling with a short sleep is enough
/// and keeps this example free of an async runtime.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Noop;
    impl std::task::Wake for Noop {
        fn wake(self: std::sync::Arc<Self>) {}
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Noop));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = Box::pin(f);
    loop {
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => return v,
            std::task::Poll::Pending => std::thread::sleep(std::time::Duration::from_micros(200)),
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    // `--validate-all`: translate every pixel shader in every model of an archive and parse the result with naga
    // (the same front end the GPU driver uses). This is plan v8 P2's coverage report: a shader the translator gets
    // wrong is found here, without a window and without a reference picture.
    if a.len() > 1 && a.iter().any(|x| x == "--validate-all") {
        let mut ar = Archive::open(&a[1]).unwrap();
        let names: Vec<String> = ar.entries.iter().map(|e| e.name.clone()).filter(|n| n.to_ascii_lowercase().ends_with(".rmb.bin")).collect();
        let (mut ok, mut bad, mut total, mut models) = (0usize, 0usize, 0usize, 0usize);
        let mut fails: Vec<String> = Vec::new();
        for name in &names {
            let Some(e) = ar.entries.iter().find(|e| &e.name == name).cloned() else { continue };
            let Ok(d) = ar.read(&e) else { continue };
            let (Ok(g), Ok(dt)) = (section(&d, b".gpu").ok_or(()), section(&d, b".data").ok_or(())) else { continue };
            let Some(m) = fm2::parse_model(&d).ok() else { continue };
            let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
            let shaders = sh::tail_shaders(gpu, m.tail_start);
            let pairs = sh::pair_groups(&sh::data_groups(data), &shaders);
            models += 1;
            for (gi, (vsx, psx)) in pairs.iter().enumerate() {
                // Vertex stage first: same translator, entry point returns the clip position.
                if let Some(vi) = vsx {
                    let s = &shaders[*vi];
                    let words: Vec<u32> = (0..s.dwords).filter_map(|k| be(gpu, s.code_offset + 4 * k)).collect();
                    if !words.is_empty() {
                        let blob = ShaderBlob {
                            stage: Stage::Vertex,
                            file_offset: s.code_offset,
                            constants: Vec::new(),
                            float_defs: Vec::new(),
                            int_defs: Vec::new(),
                            vertex_elements: Vec::new(),
                            interpolators: Vec::new(),
                            vpos_register: None,
                            ps_outputs: 0,
                            microcode: words,
                            target: String::new(),
                        };
                        let mut r = GpuResolver::default();
                        let t = wgsl::translate(&blob, "fm2_vs", &mut r);
                        let mut src = String::from("diagnostic(off, derivative_uniformity);\n");
                        src += wgsl::PRELUDE;
                        src += "struct Fx { pc: array<vec4<f32>, 256>, r: array<vec4<f32>, 16>, b: array<vec4<u32>, 64>, i: array<vec4<i32>, 32> }\n";
                        src += "@group(0) @binding(0) var<uniform> fx: Fx;\n@group(0) @binding(1) var samp: sampler;\n";
                        for (k, &(tf, dim)) in r.textures.keys().enumerate() {
                            let ty = match dim {
                                1 | 0 => "texture_2d<f32>",
                                2 => "texture_3d<f32>",
                                _ => "texture_cube<f32>",
                            };
                            src += &format!("@group(1) @binding({k}) var t{tf}_{dim}: {ty};\n");
                        }
                        src += &t.code;
                        src += "@vertex fn main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> { return fm2_vs(vi).pos; }\n";
                        total += 1;
                        let res = naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src)).and_then(|module| {
                            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                                .validate(&module)
                                .map(|_| ())
                                .map_err(|e| format!("{e:?}"))
                        });
                        match res {
                            Ok(()) => ok += 1,
                            Err(e) => {
                                bad += 1;
                                if fails.len() < 10 {
                                    fails.push(format!("{name} group {gi} VS: {}", e.lines().take(3).collect::<Vec<_>>().join(" | ")));
                                }
                            }
                        }
                    }
                }
                let Some(pi) = psx else { continue };
                let s = &shaders[*pi];
                let words: Vec<u32> = (0..s.dwords).filter_map(|k| be(gpu, s.code_offset + 4 * k)).collect();
                if words.is_empty() {
                    continue;
                }
                let blob = ShaderBlob {
                    stage: Stage::Pixel,
                    file_offset: s.code_offset,
                    constants: Vec::new(),
                    float_defs: Vec::new(),
                    int_defs: Vec::new(),
                    vertex_elements: Vec::new(),
                    interpolators: (0..16u8).map(|reg| Interpolator { usage: 0, usage_index: 0, reg }).collect(),
                    vpos_register: None,
                    ps_outputs: 0,
                    microcode: words,
                    target: String::new(),
                };
                let mut r = GpuResolver::default();
                let t = wgsl::translate(&blob, "fm2_ps", &mut r);
                let mut src = String::from("diagnostic(off, derivative_uniformity);\n");
                src += wgsl::PRELUDE;
                src += "struct Fx { pc: array<vec4<f32>, 256>, r: array<vec4<f32>, 16>, b: array<vec4<u32>, 64>, i: array<vec4<i32>, 32> }\n";
                src += "@group(0) @binding(0) var<uniform> fx: Fx;\n@group(0) @binding(1) var samp: sampler;\n";
                for (k, &(tf, dim)) in r.textures.keys().enumerate() {
                    let ty = match dim {
                        1 | 0 => "texture_2d<f32>",
                        2 => "texture_3d<f32>",
                        _ => "texture_cube<f32>",
                    };
                    src += &format!("@group(1) @binding({k}) var t{tf}_{dim}: {ty};\n");
                }
                src += &t.code;
                src += "@fragment fn main(@builtin(position) p: vec4<f32>, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {\n  var i: FxPsIn;\n";
                src += "  i.r0 = fx.r[0]; i.r1 = fx.r[1]; i.r2 = fx.r[2]; i.r3 = fx.r[3]; i.r4 = fx.r[4]; i.r5 = fx.r[5]; i.r6 = fx.r[6]; i.r7 = fx.r[7];\n";
                src += "  i.r8 = fx.r[8]; i.r9 = fx.r[9]; i.r10 = fx.r[10]; i.r11 = fx.r[11]; i.r12 = fx.r[12]; i.r13 = fx.r[13]; i.r14 = fx.r[14]; i.r15 = fx.r[15];\n";
                src += "  i.vpos = p; i.front_facing = ff;\n  return fm2_ps(i).c0;\n}\n";
                total += 1;
                let res = naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src)).and_then(|module| {
                    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                        .validate(&module)
                        .map(|_| ())
                        .map_err(|e| format!("{e:?}"))
                });
                match res {
                    Ok(()) => ok += 1,
                    Err(e) => {
                        bad += 1;
                        if fails.len() < 10 {
                            fails.push(format!("{name} group {gi}: {}", e.lines().take(3).collect::<Vec<_>>().join(" | ")));
                        }
                    }
                }
            }
        }
        println!("{ok}/{total} shaders (vertex + pixel) translated and naga-validated, {bad} failed, across {models} models");
        for f in &fails {
            println!("  FAIL {f}");
        }
        return;
    }
    // `--dump-data <offset> <len>`: hex words of the model's `.data` section, for following a stream record's
    // pointer by hand (the vertex-fetch constants of `0x16` records are such pointers).
    if a.iter().any(|x| x == "--dump-data") {
        let i = a.iter().position(|x| x == "--dump-data").unwrap();
        let off = usize::from_str_radix(a[i + 1].trim_start_matches("0x"), 16).unwrap();
        let len: usize = a.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(64);
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", a[2]);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
        let data = &d[0x190 + g..0x190 + g + dt];
        println!(".data len {} — words at {:#x}:", data.len(), off);
        let mut o = off;
        while o < (off + len).min(data.len()) {
            let w = be(data, o).unwrap_or(0);
            println!("  {:#06x}: {:08X}  (u32 {})", o, w, w);
            o += 4;
        }
        return;
    }
    // `--dump-raw <offset> <len>`: hex words of the model FILE (header included), so the 0x190-byte header can be
    // inspected for a table the sections do not carry.
    if a.iter().any(|x| x == "--dump-raw") {
        let i = a.iter().position(|x| x == "--dump-raw").unwrap();
        let off = usize::from_str_radix(a[i + 1].trim_start_matches("0x"), 16).unwrap();
        let len: usize = a.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(64);
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", a[2]);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        println!("file len {} — words at {:#x}:", d.len(), off);
        let mut o = off;
        while o < (off + len).min(d.len()) {
            println!("  {:#06x}: {:08X}  (u32 {})", o, be(&d, o).unwrap_or(0), be(&d, o).unwrap_or(0));
            o += 4;
        }
        return;
    }
    // `--decode <draw> <vertex>`: decode one vertex of a draw with the INFERRED layout (see `infer_layout`) and
    // compare its position against the parser's `Draw::positions`, which is the one attribute the format docs call
    // verified. If they agree, the position part of the inferred layout is confirmed by an independent path.
    if a.iter().any(|x| x == "--decode") {
        let i = a.iter().position(|x| x == "--decode").unwrap();
        let di: usize = a[i + 1].parse().unwrap();
        let vn: usize = a[i + 2].parse().unwrap();
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", a[2]);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let m = fm2::parse_model(&d).unwrap();
        let dr = &m.draws[di];
        let base = vn * dr.stride;
        let f = |o: usize| f32::from_bits(be(&dr.vertices, base + o).unwrap_or(0));
        let h = |o: usize| (be(&dr.vertices, base + o).unwrap_or(0) >> 16) as u16;
        let l = |o: usize| (be(&dr.vertices, base + o).unwrap_or(0) & 0xFFFF) as u16;
        println!("draw {di} vertex {vn}: stride {}", dr.stride);
        println!("  parser position {:?}", dr.positions.get(vn));
        println!("  INFERRED decode x={} y={} z={}", f(0), f(4), f(8));
        match dr.stride {
            20 => {
                println!("  INFERRED uv0   {:?}", [l(12), h(12)]);
                println!("  INFERRED lm uv {:?}", [l(16), h(16)]);
            }
            24 => {
                println!("  INFERRED normal {:?}", [l(12), h(12), l(16)]);
                println!("  INFERRED uv     {:?}", [l(20), h(20)]);
            }
            32 => {
                // MEASURED (MODLOG 268): position f32 x3 (bytes 0-11), then five dwords each holding one packed
                // texture coordinate pair as (u: i16, v: i16). Confirmed on the importer's resolved uv for register 7
                // across 12 vertices (max error 0.000017). The other four pairs are the same packing by structure;
                // only register 7 could be cross-checked, because `fm2_uv`'s value for register 8 is its own known
                // degenerate case ("Sunset model 77").
                println!("  MEASURED layout: position + 5 packed (u:i16 high half, v:i16 low half) at bytes 12/16/20/24/28:");
                for k in 0..5 {
                    let raw = l(12 + 4 * k) as u32 | ((h(12 + 4 * k) as u32) << 16);
                    let u = (raw >> 16) as u16 as i16 as f32 / 32767.0;
                    let v = (raw & 0xFFFF) as u16 as i16 as f32 / 32767.0;
                    println!("    pair {k} (dword {}): u {u:.6} v {v:.6}  raw [{:04X},{:04X}]", 3 + k, raw >> 16, raw & 0xFFFF);
                }
            }
            other => println!("  INFERRED: stride {other} not in the known set (20, 24)"),
        }
        return;
    }
    // `--containers`: scan the model's `.data` for shader containers (`102A1100` pixel, `102A1101` vertex) and
    // report what each declares. A vertex container carries the **vertex declaration**, which the raw tail microcode
    // does not -- this is the last place the attribute layout can come from.
    if a.iter().any(|x| x == "--containers") {
        use fh1_shaders::container::{ShaderBlob, MAGIC_PS, MAGIC_VS};
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", a[2]);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
        let data = &d[0x190 + g..0x190 + g + dt];
        let mut o = 0usize;
        let mut found = 0;
        while o + 4 <= data.len() {
            let w = be(data, o).unwrap_or(0);
            if w == MAGIC_VS || w == MAGIC_PS {
                match ShaderBlob::parse(&data[o..], o) {
                    Ok(b) => {
                        let len = ShaderBlob::container_len(&data[o..]).unwrap_or(4);
                        println!(
                            "{:#06x}: {:?} len {} dwords {} vertex_elements {:?} interpolators {:?} vpos {:?}",
                            o, b.stage, len, b.microcode.len(), b.vertex_elements, b.interpolators, b.vpos_register
                        );
                        // The constant table: a container carries parameter defaults, and a VS matrix could live here.
                        // This is the lead from MODLOG 279 -- the game's vertex inputs were thought unrecoverable.
                        if !b.constants.is_empty() {
                            println!("    {} constants:", b.constants.len());
                            for c in b.constants.iter().take(40) {
                                let def = c.default.as_ref().map(|d| {
                                    let f: Vec<String> = d.iter().map(|w| format!("{:.4}", f32::from_bits(*w))).collect();
                                    f.join(",")
                                });
                                println!(
                                    "      c{} {:?} ty{} {}x{} count{} default {:?}",
                                    c.register, c.class, c.ty, c.rows, c.columns, c.count, def
                                );
                            }
                        }
                        found += 1;
                        o += len.max(4);
                        continue;
                    }
                    Err(e) => println!("{:#06x}: magic {:08X} but parse failed: {e}", o, w),
                }
            }
            o += 4;
        }
        println!("{found} shader containers in .data");
        return;
    }
    // `--uvprobe <n>` (with --pvs and --draw): print, for vertices 0..n, the texture coordinates the importer's
    // `fm2_uv` resolved for each sampler register, next to the raw 16-bit values of that vertex. The point is to find
    // which offset and decode rule reproduce the resolved numbers, so the stride-32 layout stops being a guess.
    if a.iter().any(|x| x == "--uvprobe") {
        use fh1setup::fm2_uv::{self, ItemKind};
        let i = a.iter().position(|x| x == "--uvprobe").unwrap();
        let nv: usize = a.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(8);
        let model: usize = a[2].parse().unwrap();
        let arg = |k: &str| -> Option<String> { a.iter().position(|x| x == k).and_then(|j| a.get(j + 1)).cloned() };
        let (Some(pp), Some(di)) = (arg("--pvs"), arg("--draw").and_then(|v| v.parse::<usize>().ok())) else {
            println!("--uvprobe needs --pvs and --draw");
            return;
        };
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", a[2]);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let m = fm2::parse_model(&d).unwrap();
        let pvs = fm2::parse_pvs(&std::fs::read(pp).unwrap()).unwrap();
        let list = &pvs.models[model];
        let kind = |it: u32| match list.get(it as usize).and_then(|&t| pvs.textures.get(t as usize)) {
            None => ItemKind::System,
            Some(t) if matches!(t.flags, 8 | 9 | 12) => ItemKind::Tile,
            Some(_) => ItemKind::Other,
        };
        let uvs = fm2_uv::resolve(&d, &m, &kind);
        let Some((u, ps)) = uvs.get(di).and_then(|u| u.ps.as_ref().map(|ps| (u, ps))) else {
            println!("draw {di}: no resolved shader");
            return;
        };
        let dr = &m.draws[di];
        println!("# draw {di} stride {} vertices {}", dr.stride, dr.positions.len());
        for (item, coords) in &u.slots {
            let (tf, reg, atlas) = (
                ps.item_sampler.get(item).copied(),
                ps.item_sampler.get(item).and_then(|t| ps.sampler_reg.get(t)).copied(),
                u.atlas.get(item).copied(),
            );
            println!("# item {item} tf{tf:?} r{reg:?} atlas {atlas:?}");
        }
        for v in 0..nv.min(dr.positions.len()) {
            let base = v * dr.stride;
            let mut raw = Vec::new();
            for k in 0..(dr.stride / 2) {
                let w = be(&dr.vertices, base + 4 * (k / 2)).unwrap_or(0);
                raw.push(if k % 2 == 0 { w >> 16 } else { w & 0xFFFF });
            }
            for (item, coords) in &u.slots {
                let Some(&uv) = coords.get(v) else { continue };
                let tf = ps.item_sampler.get(item).copied();
                let reg = ps.item_sampler.get(item).and_then(|t| ps.sampler_reg.get(t)).copied();
                let atlas = u.atlas.get(item).is_some();
                println!("V{v} item {item} tf{tf:?} r{reg:?} atlas {atlas} uv [{:.6},{:.6}]", uv[0], uv[1]);
            }
            println!("W{v} {}", raw.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "));
        }
        return;
    }
    // `--vs <n>`: run the draw's own vertex shader on the GPU over vertices 0..n and dump its output registers.
    // The vertex declaration FM2 does not store is synthesised from the MEASURED stride-32 layout (MODLOG 268):
    // attribute 0 = position, attributes 1..5 = the five packed texture coordinate pairs. Which fetch gets which
    // attribute is INFERRED by order, which is what this run exists to test.
    if a.iter().any(|x| x == "--vs") {
        use fh1_shaders::container::VertexElement;
        use fh1_shaders::ucode::{self, CfOp, Fetch, Instr};
        use fh1_shaders::wgsl::Resolver as _;
        let i = a.iter().position(|x| x == "--vs").unwrap();
        let nv: usize = a.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(8);
        let model: usize = a[2].parse().unwrap();
        let group: usize = a[3].parse().unwrap();
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", model);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
        let m = fm2::parse_model(&d).unwrap();
        let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
        let shaders = sh::tail_shaders(gpu, m.tail_start);
        let pairs = sh::pair_groups(&sh::data_groups(data), &shaders);
        let Some(vs_idx) = pairs[group].0 else { println!("group {group} has no vertex shader"); return };
        let vs = &shaders[vs_idx];
        let words: Vec<u32> = (0..vs.dwords).filter_map(|k| be(gpu, vs.code_offset + 4 * k)).collect();
        // Every vfetch instruction address, in program order.
        let mut addrs: Vec<u32> = Vec::new();
        for cf in ucode::control_flow(&words) {
            if !matches!(cf.op, CfOp::Exec { .. } | CfOp::CondExec { .. } | CfOp::CondExecPred { .. }) {
                continue;
            }
            for (addr, ins) in ucode::exec_instructions(&words, &cf) {
                if let Instr::Fetch(Fetch::Vertex(_)) = ins {
                    if !addrs.contains(&addr) {
                        addrs.push(addr);
                    }
                }
            }
        }
        println!("VS {} dwords, {} literals, {} distinct vertex fetches at {:?}", vs.dwords, vs.literals.len(), addrs.len(), addrs);
        for (k, l) in vs.literals.iter().enumerate() {
            println!("  literal {k}: {l:.4?}");
        }
        let elements: Vec<VertexElement> = addrs.iter().enumerate().map(|(k, &address)| VertexElement { address, usage: k as u8, usage_index: 0 }).collect();
        let blob = ShaderBlob {
            stage: Stage::Vertex,
            file_offset: vs.code_offset,
            constants: Vec::new(),
            float_defs: Vec::new(),
            int_defs: Vec::new(),
            vertex_elements: elements,
            interpolators: Vec::new(),
            vpos_register: None,
            ps_outputs: 0,
            microcode: words.clone(),
            target: String::new(),
        };
        struct VsResolver;
        impl Resolver for VsResolver {
            fn float_const(&mut self, _: Stage, reg: u32) -> String {
                format!("fx.pc[{reg}]")
            }
            fn float_const_rel(&mut self, _: Stage, base: u32, index: &str) -> String {
                format!("fx.pc[clamp({base} + {index}, 0, 255)]")
            }
            fn bool_const(&mut self, _: Stage, addr: u32) -> String {
                format!("(fx.b[{}][{}] != 0u)", addr / 4, addr % 4)
            }
            fn int_const(&mut self, _: Stage, id: u32) -> String {
                format!("fx.i[{id}]")
            }
            fn texture(&mut self, _: Stage, _tf: u32, _d: u8) -> Option<(String, String)> {
                None
            }
            fn vertex_input(&mut self, usage: u8, _index: u8) -> String {
                format!("vs_attr[vertex_index * 8u + {}u]", usage)
            }
        }
        let t = wgsl::translate(&blob, "fm2_vs", &mut VsResolver);
        let mut src = String::from("diagnostic(off, derivative_uniformity);\n");
        src += wgsl::PRELUDE;
        src += "struct Fx { pc: array<vec4<f32>, 256>, r: array<vec4<f32>, 16>, b: array<vec4<u32>, 64>, i: array<vec4<i32>, 32>, count: u32 }\n";
        src += "@group(0) @binding(0) var<uniform> fx: Fx;\n";
        src += "@group(0) @binding(1) var<storage, read> vs_attr: array<vec4<f32>>;\n";
        src += "@group(0) @binding(2) var<storage, read_write> vs_out: array<vec4<f32>>;\n";
        src += &t.code;
        src += "@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) g: vec3<u32>) {\n";
        src += "  let i = g.x;\n  if (i >= fx.count) { return; }\n  let o = fm2_vs(i);\n  let b = i * 17u;\n";
        src += "  vs_out[b] = o.pos;\n";
        for k in 0..16 {
            src += &format!("  vs_out[b + {}u] = o.o{};\n", k + 1, k);
        }
        src += "}\n";
        std::fs::create_dir_all("data/work").ok();
        std::fs::write("data/work/fm2_vs_gpu.wgsl", &src).ok();

        // ---- device ----
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()),
            flags: wgpu::InstanceFlags::from_env_or_default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("fm2_vs"), ..Default::default() })).unwrap();

        let di: usize = a.iter().position(|x| x == "--draw").and_then(|j| a.get(j + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
        let dr = &m.draws[di];
        let count = nv.min(dr.positions.len());
        let stride = dr.stride;
        // Attributes: 0 = position, 1..5 = the five packed uv pairs (MEASURED packing, INFERRED order).
        let mut attr = vec![0f32; count * 8 * 4];
        for v in 0..count {
            let base = v * stride;
            let f = |o: usize| f32::from_bits(be(&dr.vertices, base + o).unwrap_or(0));
            let u32w = |o: usize| be(&dr.vertices, base + o).unwrap_or(0);
            let put = |a: &mut Vec<f32>, slot: usize, x: [f32; 4]| {
                for k in 0..4 {
                    a[v * 32 + slot * 4 + k] = x[k];
                }
            };
            put(&mut attr, 0, [f(0), f(4), f(8), 1.0]);
            for p in 0..5 {
                let raw = u32w(12 + 4 * p);
                let u = (raw >> 16) as u16 as i16 as f32 / 32767.0;
                let vv = (raw & 0xFFFF) as u16 as i16 as f32 / 32767.0;
                put(&mut attr, 1 + p, [u, vv, 0.0, 1.0]);
            }
        }
        let mut ub = vec![0u8; 256 * 16 + 16 * 16 + 64 * 16 + 32 * 16 + 16];
        for (reg, val) in [(0u32, [0f32; 4])].iter() {
            let _ = (reg, val);
        }
        let n = count as u32;
        ub[256 * 16 + 16 * 16 + 64 * 16 + 32 * 16..][..4].copy_from_slice(&n.to_le_bytes());
        let mk = |size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false });
        let uniform = mk(ub.len() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        queue.write_buffer(&uniform, 0, &ub);
        let attr_buf = mk((attr.len() * 4) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        queue.write_buffer(&attr_buf, 0, attr.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>().as_slice());
        let out_buf = mk((count * 17 * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let staging = mk((count * 17 * 16) as u64, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: attr_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: out_buf.as_entire_binding() },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            cp.set_pipeline(&pipeline);
            cp.set_bind_group(0, &bg, &[]);
            cp.dispatch_workgroups((n + 63) / 64, 1, 1);
        }
        queue.submit([enc.finish()]);
        let mut enc2 = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc2.copy_buffer_to_buffer(&out_buf, 0, &staging, 0, (count * 17 * 16) as u64);
        queue.submit([enc2.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let raw = staging.slice(..).get_mapped_range().to_vec();
        let val = |v: usize, reg: usize, k: usize| f32::from_le_bytes(raw[(v * 17 + reg) * 16 + k * 4..][..4].try_into().unwrap());
        for v in 0..count {
            println!("VS vertex {v}: pos [{:.4},{:.4},{:.4},{:.4}]", val(v, 0, 0), val(v, 0, 1), val(v, 0, 2), val(v, 0, 3));
            for reg in 0..16usize {
                let x = [val(v, reg + 1, 0), val(v, reg + 1, 1), val(v, reg + 1, 2), val(v, reg + 1, 3)];
                if x.iter().any(|c| c.abs() > 1e-6) {
                    println!("   o{reg} [{:.6},{:.6},{:.6},{:.6}]", x[0], x[1], x[2], x[3]);
                }
            }
        }
        return;
    }
    // `--render <draw>`: draw one FM2 draw properly -- our own vertex shader over the draw's real vertices
    // (MEASURED layout, MODLOG 268) and the GAME'S OWN pixel shader per pixel. The vertex shader is ours only because
    // the game's is unreproducible from the disc (its matrices are not stored, MODLOG 269); everything that decides
    // what a surface looks like is still the game's code. Writes `data/work/fm2_draw.ppm` and prints statistics.
    if a.iter().any(|x| x == "--render") {
        let i = a.iter().position(|x| x == "--render").unwrap();
        let di: usize = a.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let model: usize = a[2].parse().unwrap();
        let group: usize = a[3].parse().unwrap();
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", model);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
        let m = fm2::parse_model(&d).unwrap();
        let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
        let shaders = sh::tail_shaders(gpu, m.tail_start);
        let pairs = sh::pair_groups(&sh::data_groups(data), &shaders);
        let ps = &shaders[pairs[group].1.expect("no pixel shader")];
        let words: Vec<u32> = (0..ps.dwords).filter_map(|k| be(gpu, ps.code_offset + 4 * k)).collect();
        let samplers = sh::ps_samplers(ps);
        let dr = &m.draws[di];
        // Constants: the draw's own, plus the track's lightmap colours if given.
        let mut consts: HashMap<u32, [f32; 4]> = HashMap::new();
        let nl = ps.literals.len() as u32;
        for (k, l) in ps.literals.iter().enumerate() {
            consts.insert(256 - nl + k as u32, *l);
        }
        for (k, v) in dr.constants.iter() {
            consts.insert(*k, *v);
        }
        if let Ok(v) = std::env::var("FM2_RENDER_C184") {
            consts.insert(184, parse4(&v));
        }
        if let Ok(v) = std::env::var("FM2_RENDER_C185") {
            consts.insert(185, parse4(&v));
        }
        // `c184`/`c185` are the shader's light colours. The importer's oracle sets them from the TRACK's own
        // `fx_globals.json` (V2LightmapColor1 = dark, V2LightmapColor2 = light) and that is how it reproduces the Xenia
        // pixel (MODLOG 218). Earlier runs here passed hand-written guesses instead; read the real values unless an
        // explicit override is given.
        let texdir_early = std::path::PathBuf::from(&a[4]);
        let fx_path = texdir_early.parent().and_then(|p| p.parent()).map(|p| p.join("tracks/fx_globals.json"));
        let mut fx_dark_light: Option<([f32; 3], [f32; 3])> = None;
        if let Some(v) = fx_path.and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
            let col = |k: &str| -> Option<[f32; 3]> {
                let mut out = [0f32; 3];
                for i in 0..3 {
                    out[i] = v["globals"][k][i].as_f64()? as f32;
                }
                Some(out)
            };
            if let (Some(d), Some(l)) = (col("V2LightmapColor1"), col("V2LightmapColor2")) {
                fx_dark_light = Some((d, l));
                if std::env::var_os("FM2_RENDER_C184").is_none() {
                    consts.insert(184, [d[0], d[1], d[2], 1.0]);
                }
                if std::env::var_os("FM2_RENDER_C185").is_none() {
                    consts.insert(185, [l[0], l[1], l[2], 1.0]);
                }
                println!("  c184 (dark)  [{:.4},{:.4},{:.4}]  c185 (light) [{:.4},{:.4},{:.4}]  <- track fx_globals", d[0], d[1], d[2], l[0], l[1], l[2]);
            }
        }
        let _ = &fx_dark_light;
        // Textures from the draw's PVS items, as `--pvs`/`--draw` does.
        let arg = |k: &str| -> Option<String> { a.iter().position(|x| x == k).and_then(|j| a.get(j + 1)).cloned() };
        let texdir = std::path::PathBuf::from(&a[4]);
        let mut tex: HashMap<u32, Tex> = HashMap::new();
        if let Some(pp) = arg("--pvs") {
            let pvs = fm2::parse_pvs(&std::fs::read(pp).unwrap()).unwrap();
            let list = &pvs.models[model];
            let mut slots: Vec<u32> = samplers.iter().map(|s| s.index).collect();
            slots.sort();
            slots.dedup();
            for (k, item) in dr.items.iter().enumerate() {
                let Some(&slot) = slots.get(k) else { continue };
                let Some(&ti) = list.get(*item as usize) else { continue };
                let t = &pvs.textures[ti as usize];
                if let Some(img) = load_tex(&texdir.join(format!("{:08x}.dds", t.file_id))) {
                    tex.insert(slot, img);
                }
            }
        }
        let pairs_n: usize = (dr.stride.saturating_sub(12)) / 4;
        println!("render: model {model} group {group} draw {di}, stride {}, {} vertices, {} indices, {} textures, {} packed pairs",
                 dr.stride, dr.positions.len(), dr.indices.len(), tex.len(), pairs_n);
        // Which packed pair feeds which register, and on what scale: fit `resolved = a*(i16/32767) + b` for every
        // pair against the coordinates `fm2_uv` resolved, and keep the pair whose residual reaches the noise floor.
        // The scale is per register (MODLOG 273: 1, 32 and 256 all occur), so it is fitted, never assumed.
        let sig = |p: usize, v: usize, hi: bool| -> f32 {
            let raw = be(&dr.vertices, v * dr.stride + 12 + 4 * p).unwrap_or(0);
            let h = if hi { (raw >> 16) as u16 } else { (raw & 0xFFFF) as u16 };
            (h as i16) as f32 / 32767.0
        };
        let fit = |p: usize, v_ids: &[usize], ys: &[f32], hi: bool| -> Option<(f32, f32, f32)> {
            let n = v_ids.len() as f32;
            if n < 4.0 {
                return None;
            }
            let xs: Vec<f32> = v_ids.iter().map(|&v| sig(p, v, hi)).collect();
            let (mx, my) = (xs.iter().sum::<f32>() / n, ys.iter().sum::<f32>() / n);
            let (mut cov, mut var) = (0.0f32, 0.0f32);
            for k in 0..xs.len() {
                cov += (xs[k] - mx) * (ys[k] - my);
                var += (xs[k] - mx) * (xs[k] - mx);
            }
            if var < 1e-12 {
                return None;
            }
            let a = cov / var;
            let b = my - a * mx;
            let res = (0..xs.len()).fold(0.0f32, |m, k| m.max((a * xs[k] + b - ys[k]).abs()));
            Some((a, b, res))
        };
        // reg -> (pair, a0, b0, a1, b1)
        let mut reg_fits: Vec<(u32, usize, f32, f32, f32, f32)> = Vec::new();
        if let Some(pp) = arg("--pvs") {
            use fh1setup::fm2_uv::{self, ItemKind};
            let pvs = fm2::parse_pvs(&std::fs::read(pp).unwrap()).unwrap();
            let list = &pvs.models[model];
            let kind = |it: u32| match list.get(it as usize).and_then(|&t| pvs.textures.get(t as usize)) {
                None => ItemKind::System,
                Some(t) if matches!(t.flags, 8 | 9 | 12) => ItemKind::Tile,
                Some(_) => ItemKind::Other,
            };
            let uvs = fm2_uv::resolve(&d, &m, &kind);
            if let Some((u, ps)) = uvs.get(di).and_then(|u| u.ps.as_ref().map(|ps| (u, ps))) {
                let nv = dr.positions.len();
                for (item, coords) in &u.slots {
                    let Some(&tf) = ps.item_sampler.get(item) else { continue };
                    let Some(&reg) = ps.sampler_reg.get(&tf) else { continue };
                    if coords.len() != nv || reg_fits.iter().any(|f| f.0 == reg) {
                        continue;
                    }
                    let ids: Vec<usize> = (0..nv).collect();
                    let ys0: Vec<f32> = coords.iter().map(|c| c[0]).collect();
                    let ys1: Vec<f32> = coords.iter().map(|c| c[1]).collect();
                    let mut best: Option<(f32, usize, f32, f32, f32, f32)> = None;
                    for p in 0..pairs_n {
                        let (Some((a0, b0, r0)), Some((a1, b1, r1))) = (fit(p, &ids, &ys0, true), fit(p, &ids, &ys1, false)) else { continue };
                        let res = r0.max(r1);
                        if res < 2e-3 && best.as_ref().is_none_or(|b| res < b.0) {
                            best = Some((res, p, a0, b0, a1, b1));
                        }
                    }
                    match best {
                        Some((res, p, a0, b0, a1, b1)) => {
                            println!("  r{reg} <- pair {p}, scale ({a0:.4}, {a1:.4}), bias ({b0:.4}, {b1:.4})  [residual {res:.1e}]");
                            reg_fits.push((reg, p, a0, b0, a1, b1));
                        }
                        None => println!("  r{reg}: no pair fits on any scale {} vertices", coords.len()),
                    }
                }
            }
        }
        reg_fits.sort_by_key(|f| f.0);
        println!("  {} register(s) mapped by measurement; the rest stay neutral", reg_fits.len());

        let blob = ShaderBlob {
            stage: Stage::Pixel,
            file_offset: ps.code_offset,
            constants: Vec::new(),
            float_defs: Vec::new(),
            int_defs: Vec::new(),
            vertex_elements: Vec::new(),
            interpolators: (0..16u8).map(|reg| Interpolator { usage: 0, usage_index: 0, reg }).collect(),
            vpos_register: None,
            ps_outputs: 0,
            microcode: words.clone(),
            target: String::new(),
        };
        let mut r = GpuResolver::default();
        let t = wgsl::translate(&blob, "fm2_ps", &mut r);
        const W: u32 = 512;
        const H: u32 = 512;
        let mut src = String::from("diagnostic(off, derivative_uniformity);\n");
        src += wgsl::PRELUDE;
        src += "struct Fx { pc: array<vec4<f32>, 256>, r: array<vec4<f32>, 16>, b: array<vec4<u32>, 64>, i: array<vec4<i32>, 32> }\n";
        src += "@group(0) @binding(0) var<uniform> fx: Fx;\n@group(0) @binding(1) var samp: sampler;\n";
        for (k, &(tf, dim)) in r.textures.keys().enumerate() {
            let ty = match dim {
                1 | 0 => "texture_2d<f32>",
                2 => "texture_3d<f32>",
                _ => "texture_cube<f32>",
            };
            src += &format!("@group(1) @binding({k}) var t{tf}_{dim}: {ty};\n");
        }
        src += &t.code;
        // Our vertex shader: position through a fixed transform, and one varying per register that measurement
        // mapped. The number of varyings comes from the fit, not from a constant, so this works on any draw.
        let nuv = reg_fits.len();
        let mut vin = String::from("struct Vin { @location(0) p: vec3<f32>");
        let mut vout = String::from("struct Vout { @builtin(position) pos: vec4<f32>");
        for k in 0..nuv {
            vin += &format!(", @location({}) uv{k}: vec2<f32>", k + 1);
            vout += &format!(", @location({k}) uv{k}: vec2<f32>");
        }
        vin += " };\n";
        vout += " };\n";
        src += &vin;
        src += &vout;
        // Orthographic down the draw's THINNEST axis, so a vertical surface (a wall, a kerb face) still fills the frame.
        // Always using the x/z plane collapsed draw 0 to a line (MODLOG 275).
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &dr.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let thin = (0..3).min_by(|&a, &b| ext[a].partial_cmp(&ext[b]).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(1);
        let others: Vec<usize> = (0..3).filter(|&k| k != thin).collect();
        let (ax, ay) = (others[0], others[1]);
        let span = ext[ax].max(ext[ay]).max(1e-3);
        let (cx, cy) = ((lo[ax] + hi[ax]) * 0.5, (lo[ay] + hi[ay]) * 0.5);
        let ld = lo[thin];
        let c = ["x", "y", "z"];
        println!("  projection: screen ({}, {}), span {span:.1}, thin axis {} (extent {:.2})", c[ax], c[ay], c[thin], ext[thin]);
        src += &format!(
            "@vertex fn vs(v: Vin) -> Vout {{\n  var o: Vout;\n  let sx = (v.p.{axc} - {cx}) / {span};\n  let sy = (v.p.{ayc} - {cy}) / {span};\n  o.pos = vec4<f32>(sx * 2.0, sy * 2.0, (v.p.{dc} - {ld}) / {span}, 1.0);\n",
            axc = c[ax],
            ayc = c[ay],
            dc = c[thin],
            cx = cx,
            cy = cy,
            span = span,
            ld = ld
        );
        for k in 0..nuv {
            src += &format!("  o.uv{k} = v.uv{k};\n");
        }
        src += "  return o;\n}\n";
        // The pixel shader: every register measurement could map gets its own varying; the rest get a constant that is
        // INFERRED, because the disc does not carry them (MODLOG 269/270).
        src += "@fragment fn main(o: Vout, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {\n";
        src += "  var i: FxPsIn;\n";
        for (k, f) in reg_fits.iter().enumerate() {
            src += &format!("  i.r{} = vec4<f32>(o.uv{k}, 0.0, 1.0);\n", f.0);
        }
        for reg in 0..16usize {
            if reg_fits.iter().any(|f| f.0 as usize == reg) {
                continue;
            }
            if std::env::var("FM2_RENDER_NEUTRAL").is_ok() {
                src += &format!("  i.r{reg} = vec4<f32>(0.5, 0.5, 0.5, 1.0);\n");
            } else {
                src += &format!("  i.r{reg} = vec4<f32>(0.0, 0.0, 0.0, 1.0);\n");
            }
        }
        src += "  i.vpos = vec4<f32>(0.0); i.front_facing = ff;\n";
        if std::env::var("FM2_RENDER_FLAT").is_ok() {
            // Geometry check only: paint the interpolated coordinate instead of running the shader, so "nothing
            // drawn" can be told apart from "the shader returned black".
            src += "  return vec4<f32>(fract(o.uv0 * 8.0), 0.0, 1.0);\n}\n";
        } else {
            src += "  return fm2_ps(i).c0;\n}\n";
        }
        std::fs::create_dir_all("data/work").ok();
        std::fs::write("data/work/fm2_draw.wgsl", &src).ok();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()),
            flags: wgpu::InstanceFlags::from_env_or_default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("fm2_render"), ..Default::default() })).unwrap();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });

        // Vertex buffer: position, then one fitted coordinate pair per mapped register.
        let mut vb: Vec<f32> = Vec::with_capacity(dr.positions.len() * (3 + 2 * nuv));
        for v in 0..dr.positions.len() {
            let base = v * dr.stride;
            let f = |o: usize| f32::from_bits(be(&dr.vertices, base + o).unwrap_or(0));
            vb.extend_from_slice(&[f(0), f(4), f(8)]);
            for f2 in &reg_fits {
                vb.extend_from_slice(&[f2.2 * sig(f2.1, v, true) + f2.3, f2.4 * sig(f2.1, v, false) + f2.5]);
            }
        }
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (vb.len() * 4) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        queue.write_buffer(&vbuf, 0, vb.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>().as_slice());
        let ibuf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (dr.indices.len() * 4) as u64, usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        queue.write_buffer(&ibuf, 0, dr.indices.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>().as_slice());

        let mut ub = vec![0u8; 256 * 16 + 16 * 16 + 64 * 16 + 32 * 16];
        for (reg, v) in consts.iter() {
            for (k, x) in v.iter().enumerate() {
                ub[*reg as usize * 16 + k * 4..*reg as usize * 16 + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
            }
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: ub.len() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        queue.write_buffer(&uniform, 0, &ub);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let mut views = Vec::new();
        let mut tex_entries = Vec::new();
        for (k, &(tf, _d)) in r.textures.keys().enumerate() {
            let (w, h, rgba) = match tex.get(&tf) {
                Some(t) => (t.w as u32, t.h as u32, t.rgba.clone()),
                None => (1, 1, vec![0, 0, 0, 255]),
            };
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &rgba,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            views.push(t.create_view(&wgpu::TextureViewDescriptor::default()));
            tex_entries.push(wgpu::BindGroupLayoutEntry {
                binding: k as u32,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            });
        }
        let bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &tex_entries });
        let entries: Vec<wgpu::BindGroupEntry> = views.iter().enumerate().map(|(k, v)| wgpu::BindGroupEntry { binding: k as u32, resource: wgpu::BindingResource::TextureView(v) }).collect();
        let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &bgl1, entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl0), Some(&bgl1)], immediate_size: 0 });
        let mut attrs: Vec<wgpu::VertexAttribute> = vec![wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }];
        for k in 0..nuv {
            attrs.push(wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 12 + 8 * k as u64,
                shader_location: k as u32 + 1,
            });
        }
        let vstride = (3 + 2 * nuv) as u64 * 4;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: vstride, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba32Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let tv = target.create_view(&wgpu::TextureViewDescriptor::default());
        let padded = (W * 16).div_ceil(256) * 256;
        let out = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (padded * H) as u64, usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let staging = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (padded * H) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tv,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&pipeline);
            rp.set_bind_group(0, &bg0, &[]);
            if !views.is_empty() {
                rp.set_bind_group(1, &bg1, &[]);
            }
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..dr.indices.len() as u32, 0, 0..1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &out, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(H) } },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        );
        let mut enc2 = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc2.copy_buffer_to_buffer(&out, 0, &staging, 0, (padded * H) as u64);
        queue.submit([enc.finish(), enc2.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let px = staging.slice(..).get_mapped_range().to_vec();
        // Write a PPM (no encoder needed) and report coverage + colour.
        let mut ppm = format!("P3\n{W} {H}\n255\n").into_bytes();
        let (mut covered, mut sum) = (0usize, [0.0f64; 3]);
        for y in 0..H {
            for x in 0..W {
                let o = (y * padded + x * 16) as usize;
                let f = |k: usize| f32::from_le_bytes(px[o + k * 4..o + k * 4 + 4].try_into().unwrap());
                let c = [f(0), f(1), f(2)];
                if c.iter().any(|v| *v > 1e-4) {
                    covered += 1;
                    for k in 0..3 {
                        sum[k] += c[k] as f64;
                    }
                }
                ppm.extend_from_slice(format!("{} {} {} ", (c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8).as_bytes());
            }
        }
        std::fs::write("data/work/fm2_draw.ppm", &ppm).ok();
        let n = covered.max(1) as f64;
        println!("rendered {W}x{H}: {covered} covered pixels ({:.1}%), mean colour [{:.4},{:.4},{:.4}]",
                 100.0 * covered as f64 / (W * H) as f64, sum[0] / n, sum[1] / n, sum[2] / n);
        // The track's own lightmap light: FM2 lights a surface by lerp(V2LightmapColor1, V2LightmapColor2, tile),
        // where the tile value comes from the lightmap atlas (MODLOG 283). The tile value is NOT sampled here, so it is
        // an input -- which makes the two bounding cases measurable and keeps the honest figure honest.
        let fx_path = texdir.parent().and_then(|p| p.parent()).map(|p| p.join("tracks/fx_globals.json"));
        if let Some(v) = fx_path.and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
            let col = |k: &str, d: f32| [0, 1, 2].map(|i| v["globals"][k][i].as_f64().map_or(d, |x| x as f32));
            let dark = col("V2LightmapColor1", 0.0);
            let light = col("V2LightmapColor2", 1.0);
            let t: f32 = std::env::var("FM2_LMAP_T").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let lit = [0, 1, 2].map(|k| (sum[k] / n) as f32 * (dark[k] + (light[k] - dark[k]) * t));
            println!("  lightmap: dark {dark:.4?} light {light:.4?} tile t={t:.3} (FM2_LMAP_T; NOT sampled)");
            println!("  lit mean colour [{:.4},{:.4},{:.4}] (as sRGB 8-bit: {:?})",
                     lit[0], lit[1], lit[2],
                     lit.map(|x| (255.0 * x.clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8));
        }
        // FM2's own final combine: the parameters are read through `fh1_remaster::post::Fm2Post`, the same struct the
        // renderer uses, so there is one parse of TrackSettings.xml. NOTE: the curve itself is WGSL (post.rs), not Rust
        // -- this prints the inputs; applying the curve on the CPU would be a second implementation of it.
        let ts_path = texdir.parent().and_then(|p| p.parent()).map(|p| p.join("tracks/TrackSettings.xml"));
        if let Some(xml) = ts_path.and_then(|p| std::fs::read_to_string(p).ok()) {
            match fh1_remaster::post::Fm2Post::from_xml(&xml) {
                Some(p) => println!(
                    "  Fm2Post: desat {} lum {:?} tint {:?} dark {:?} light {:?} lum_min {} lum_inv_range {} t {}",
                    p.desat, p.lum, p.tint, p.dark, p.light, p.lum_min, p.lum_inv_range, p.t
                ),
                None => println!("  Fm2Post: TrackSettings.xml did not parse"),
            }
        }
        println!("wrote data/work/fm2_draw.ppm");
        return;
    }
    // `--vs-asm`: print the draw group's vertex shader disassembly, to classify which instruction shapes write the
    // interpolator registers that `fm2_uv` does not resolve (MODLOG 277: coverage, not a defect).
    if a.iter().any(|x| x == "--vs-asm") {
        let model: usize = a[2].parse().unwrap();
        let group: usize = a[3].parse().unwrap();
        let mut ar = Archive::open(&a[1]).unwrap();
        let suffix = format!(".{:0>5}.rmb.bin", model);
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let d = ar.read(&e).unwrap();
        let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
        let m = fm2::parse_model(&d).unwrap();
        let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
        let shaders = sh::tail_shaders(gpu, m.tail_start);
        let pairs = sh::pair_groups(&sh::data_groups(data), &shaders);
        match pairs[group].0 {
            Some(vi) => {
                let vs = &shaders[vi];
                println!("VS {} dwords, {} literals", vs.dwords, vs.literals.len());
                // Print the whole disassembly: the output writes are what this probe exists to classify.
                println!("literals ({}):", vs.literals.len());
                for (k, l) in vs.literals.iter().enumerate() {
                    println!("  literal {k}: {l:.6?}");
                }
                println!("resolved uv sources (register -> dword, map):");
                for (reg, src) in sh::vs_uv_sources(vs) {
                    println!("  o{reg} <- dword {}, {:?}", src.dword, src.map);
                }
                println!("--- disassembly ---");
                println!("{}", vs.text);
            }
            None => println!("group {group} has no vertex shader"),
        }
        return;
    }
    // `--scan-sig <hex>`: walk every model in the zip and report the (model, group) whose pixel-shader signature
    // matches. This is how a named shader family (`FAMILY_GAMMA` in the importer keys on these) is located without
    // the importer's own probe.
    if a.len() > 3 && a.iter().any(|x| x == "--scan-sig") {
        let want: u64 = a
            .iter()
            .position(|x| x == "--scan-sig")
            .and_then(|i| a.get(i + 1))
            .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .expect("--scan-sig <hex>");
        let mut ar = Archive::open(&a[1]).unwrap();
        let names: Vec<String> = ar.entries.iter().map(|e| e.name.clone()).filter(|n| n.to_ascii_lowercase().ends_with(".rmb.bin")).collect();
        let sig = |w: &[u32]| -> u64 { w.iter().flat_map(|w| w.to_be_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) };
        let mut hits = 0;
        for name in &names {
            let Some(e) = ar.entries.iter().find(|e| &e.name == name).cloned() else { continue };
            let Ok(d) = ar.read(&e) else { continue };
            let (Ok(g), Ok(dt)) = (section(&d, b".gpu").ok_or(()), section(&d, b".data").ok_or(())) else { continue };
            let Some(m) = fm2::parse_model(&d).ok() else { continue };
            let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
            let shaders = sh::tail_shaders(gpu, m.tail_start);
            let pairs = sh::pair_groups(&sh::data_groups(data), &shaders);
            for (gi, (_vs, psx)) in pairs.iter().enumerate() {
                let Some(pi) = psx else { continue };
                let s = &shaders[*pi];
                let words: Vec<u32> = (0..s.dwords).filter_map(|k| be(gpu, s.code_offset + 4 * k)).collect();
                if sig(&words) == want {
                    let sl = sh::ps_samplers(s);
                    let model = name.rsplit('.').nth(2).unwrap_or("?");
                    println!("model {model} group {gi}: {} dwords, samplers {:?}", s.dwords, sl.iter().map(|x| (x.index, x.direct)).collect::<Vec<_>>());
                    hits += 1;
                    if hits > 40 {
                        println!("... stopping at 40 hits");
                        return;
                    }
                }
            }
        }
        println!("{hits} matches for {want:016x} in {} models", names.len());
        return;
    }
    if a.len() < 5 {
        eprintln!("usage: fm2_ps_gpu <bin.zip> <model> <group> <textures dir> [--tex N=ID] [--c R=v] [--r R=v] [--n N] [--scale S] [--dump]");
        std::process::exit(2);
    }
    let (zip, model, group, texdir) = (&a[1], a[2].parse::<usize>().unwrap(), a[3].parse::<usize>().unwrap(), std::path::PathBuf::from(&a[4]));
    let (mut texmap, mut cset, mut rset, mut bset): (Vec<(u32, u32)>, Vec<(u32, [f32; 4])>, Vec<(u32, [f32; 4])>, Vec<(u32, bool)>) = Default::default();
    let (mut scale, mut n, mut dump) = (1.0f32, 16usize, false);
    let (mut pvs_path, mut draw_ix): (Option<String>, Option<usize>) = (None, None);
    let mut vertex_at: Option<usize> = None;
    let mut i = 5;
    while i < a.len() {
        let kv = |s: &str| -> (u32, String) {
            let (k, v) = s.split_once('=').unwrap();
            (k.parse().unwrap(), v.to_owned())
        };
        match a[i].as_str() {
            "--tex" => {
                i += 1;
                let (k, v) = kv(&a[i]);
                texmap.push((k, v.parse().unwrap()));
            }
            "--c" => {
                i += 1;
                let (k, v) = kv(&a[i]);
                cset.push((k, parse4(&v)));
            }
            "--r" => {
                i += 1;
                let (k, v) = kv(&a[i]);
                rset.push((k, parse4(&v)));
            }
            "--b" => {
                i += 1;
                let (k, v) = kv(&a[i]);
                bset.push((k, v == "1"));
            }
            "--scale" => {
                i += 1;
                scale = a[i].parse().unwrap();
            }
            "--n" => {
                i += 1;
                n = a[i].parse().unwrap();
            }
            "--dump" => dump = true,
            "--pvs" => {
                i += 1;
                pvs_path = Some(a[i].clone());
            }
            "--draw" => {
                i += 1;
                draw_ix = Some(a[i].parse().unwrap());
            }
            "--list" => {}
            "--vertex" => {
                i += 1;
                vertex_at = Some(a[i].parse().unwrap());
            }
            other => eprintln!("ignoring unknown option {other}"),
        }
        i += 1;
    }

    // ---- the shader, exactly as fm2_ps_run finds it ----
    let mut ar = Archive::open(zip).unwrap();
    let suffix = format!(".{model:0>5}.rmb.bin");
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
    let m = fm2::parse_model(&d).unwrap();
    let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
    let shaders = sh::tail_shaders(gpu, m.tail_start);
    let groups = sh::data_groups(data);
    let pairs = sh::pair_groups(&groups, &shaders);
    let ps = &shaders[pairs[group].1.expect("no pixel shader for the group")];
    let words: Vec<u32> = (0..ps.dwords).map(|k| be(gpu, ps.code_offset + 4 * k).unwrap()).collect();
    let samplers = sh::ps_samplers(ps);
    println!("PS: {} dwords, {} literals, samplers {:?}", ps.dwords, ps.literals.len(), samplers.iter().map(|s| (s.index, s.direct)).collect::<Vec<_>>());

    // `--list`: every group of this model with its pixel-shader signature (FNV-1a of the microcode, the same
    // `ps_sig` the importer logs and `FAMILY_GAMMA` keys on), so a named shader family can be found by signature.
    if std::env::args().any(|x| x == "--list") {
        let sig = |w: &[u32]| -> u64 { w.iter().flat_map(|w| w.to_be_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) };
        for (gi, (_vs, psx)) in pairs.iter().enumerate() {
            if let Some(pi) = psx {
                let s = &shaders[*pi];
                let words: Vec<u32> = (0..s.dwords).map(|k| be(gpu, s.code_offset + 4 * k).unwrap()).collect();
                let sl = sh::ps_samplers(s);
                println!("group {gi}: ps_sig {:016x} {} dwords, samplers {:?}", sig(&words), s.dwords, sl.iter().map(|x| (x.index, x.direct)).collect::<Vec<_>>());
            }
        }
        return;
    }

    // ---- inputs ----
    let mut consts: HashMap<u32, [f32; 4]> = HashMap::new();
    let nl = ps.literals.len() as u32;
    for (k, l) in ps.literals.iter().enumerate() {
        consts.insert(256 - nl + k as u32, *l);
    }
    // `--pvs` + `--draw`: bind the draw's own textures to the samplers and load its pixel-shader constants, exactly
    // as `fm2_ps_run` does, so both tools are driven from one command line.
    let mut draw_consts: Vec<(u32, [f32; 4])> = Vec::new();
    if let (Some(pp), Some(di)) = (&pvs_path, draw_ix) {
        let pvs = fm2::parse_pvs(&std::fs::read(pp).unwrap()).unwrap();
        let dd = &m.draws[di];
        let list = &pvs.models[model];
        let resolve = |k: u32| list.get(k as usize).map(|&t| (pvs.textures[t as usize].file_id, pvs.textures[t as usize].flags));
        // The PVS's per-model block (12 words + marker): the one per-model record the importer does not use, and
        // therefore the only per-model place a vertex layout could hide.
        if let Some((extra, mark)) = pvs.model_extra.get(model) {
            println!("pvs model {model} extra: {:08X?} marker {:08X}", extra, mark);
        }
        println!("draw {di}: items {:?} -> {:?}", dd.items, dd.items.iter().map(|&k| resolve(k)).collect::<Vec<_>>());
        // The lightmap atlas tiles (PVS flags 8/9/12): file id and the [su, sv, ou, ov] rectangle inside the page.
        for (k, item) in dd.items.iter().enumerate() {
            if let Some(&ti) = list.get(*item as usize) {
                let t = &pvs.textures[ti as usize];
                if matches!(t.flags, 8 | 9 | 12) {
                    println!("  item {k}: lightmap tile file {} flags {} rect {:?}", t.file_id, t.flags, t.tile);
                }
            }
        }
        let mut slots: Vec<u32> = samplers.iter().map(|s| s.index).collect();
        slots.sort();
        slots.dedup();
        for (k, item) in dd.items.iter().enumerate() {
            if let (Some(&slot), Some((id, _))) = (slots.get(k), resolve(*item)) {
                if !texmap.iter().any(|t| t.0 == slot) {
                    texmap.push((slot, id));
                }
            }
        }
        for (r, v) in &dd.constants {
            draw_consts.push((*r, *v));
        }
        println!("draw constants: {:?}", dd.constants);
    }
    for (k, v) in draw_consts.iter().chain(cset.iter()) {
        consts.insert(*k, *v);
    }
    // `--vertex N`: drive the texture coordinates from the draw's OWN vertex N, resolved by the importer's
    // `fm2_uv` (which sampler each item feeds, through which atlas rectangle). This is the difference between
    // "the shader agrees with the interpreter on invented inputs" and "the shader agrees on the inputs the game
    // would give it" -- and it lets the probe be compared against the reference picture.
    let mut uv_from_vertex = false;
    if let (Some(vn), Some(pp), Some(di)) = (vertex_at, &pvs_path, draw_ix) {
        use fh1setup::fm2_uv::{self, ItemKind};
        let pvs = fm2::parse_pvs(&std::fs::read(pp).unwrap()).unwrap();
        let list = &pvs.models[model];
        let kind = |it: u32| match list.get(it as usize).and_then(|&t| pvs.textures.get(t as usize)) {
            None => ItemKind::System,
            Some(t) if matches!(t.flags, 8 | 9 | 12) => ItemKind::Tile,
            Some(_) => ItemKind::Other,
        };
        let uvs = fm2_uv::resolve(&d, &m, &kind);
        match uvs.get(di).and_then(|u| u.ps.as_ref().map(|ps| (u, ps))) {
            Some((u, ps)) => {
                let dd = &m.draws[di];
                for (item, coords) in &u.slots {
                    let Some(&uv) = coords.get(vn) else { continue };
                    let Some(&tf) = ps.item_sampler.get(item) else { continue };
                    let Some(&reg) = ps.sampler_reg.get(&tf) else { continue };
                    // The atlas sub-rectangle the shader maps the coordinate into: frac(uv) * c.xy + c.zw.
                    let uv = match u.atlas.get(item).and_then(|c| dd.constants.get(c)) {
                        Some(c) => [uv[0].fract() * c[0] + c[2], uv[1].fract() * c[1] + c[3]],
                        None => uv,
                    };
                    rset.push((reg, [uv[0], uv[1], 0.0, 1.0]));
                    println!("vertex {vn}: item {item} -> tf{tf} r{reg} uv [{:.4},{:.4}]", uv[0], uv[1]);
                }
                uv_from_vertex = true;
            }
            None => println!("--vertex: draw {di} has no resolved shader"),
        }
    }
    let mut tex: HashMap<u32, Tex> = HashMap::new();
    for (slot, id) in &texmap {
        match load_tex(&texdir.join(format!("{id:08x}.dds"))) {
            Some(t) => {
                println!("tf{slot} = texture {id} ({}x{}, rgba)", t.w, t.h);
                tex.insert(*slot, t);
            }
            None => println!("tf{slot} = texture {id}: not loaded (bound black)"),
        }
    }

    // ---- translate ----
    let blob = ShaderBlob {
        stage: Stage::Pixel,
        file_offset: ps.code_offset,
        constants: Vec::new(),
        float_defs: Vec::new(),
        int_defs: Vec::new(),
        vertex_elements: Vec::new(),
        // Every register is fed from the uniform array: the probe sets the uv registers itself, exactly as the CPU
        // interpreter does, so the translator must copy all 16 in.
        interpolators: (0..16u8).map(|reg| Interpolator { usage: 0, usage_index: 0, reg }).collect(),
        vpos_register: None,
        ps_outputs: 0,
        microcode: words.clone(),
        target: String::new(),
    };
    let mut r = GpuResolver::default();
    let t = wgsl::translate(&blob, "fm2_ps", &mut r);

    // Registers whose .xy the probe drives with the uv (the same rule as fm2_ps_run).
    let mut uv_regs: Vec<u32> = samplers.iter().filter_map(|s| s.direct.map(|x| x as u32)).collect();
    uv_regs.sort();
    uv_regs.dedup();

    let mut wgsl_src = String::from("diagnostic(off, derivative_uniformity);\n");
    wgsl_src += wgsl::PRELUDE;
    wgsl_src += "struct Fx { pc: array<vec4<f32>, 256>, r: array<vec4<f32>, 16>, b: array<vec4<u32>, 64>, i: array<vec4<i32>, 32> }\n";
    wgsl_src += "@group(0) @binding(0) var<uniform> fx: Fx;\n";
    wgsl_src += "@group(0) @binding(1) var samp: sampler;\n";
    let tex_keys: Vec<(u32, u8)> = r.textures.keys().copied().collect();
    for (k, &(tf, dim)) in tex_keys.iter().enumerate() {
        let ty = match dim {
            1 | 0 => "texture_2d<f32>",
            2 => "texture_3d<f32>",
            _ => "texture_cube<f32>",
        };
        wgsl_src += &format!("@group(1) @binding({k}) var t{tf}_{dim}: {ty};\n");
    }
    wgsl_src += &t.code;
    wgsl_src += "@vertex fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {\n";
    wgsl_src += "  var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));\n";
    wgsl_src += "  return vec4<f32>(p[vi], 0.0, 1.0);\n}\n";
    wgsl_src += &format!(
        "@fragment fn main(@builtin(position) p: vec4<f32>, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {{\n  var i: FxPsIn;\n  i.r0 = fx.r[0]; i.r1 = fx.r[1]; i.r2 = fx.r[2]; i.r3 = fx.r[3];\n  i.r4 = fx.r[4]; i.r5 = fx.r[5]; i.r6 = fx.r[6]; i.r7 = fx.r[7];\n  i.r8 = fx.r[8]; i.r9 = fx.r[9]; i.r10 = fx.r[10]; i.r11 = fx.r[11];\n  i.r12 = fx.r[12]; i.r13 = fx.r[13]; i.r14 = fx.r[14]; i.r15 = fx.r[15];\n  let uv = p.xy / f32({n});\n  i.vpos = p; i.front_facing = ff;\n  return fm2_ps(i).c0;\n}}\n"
    );
    // The uv injection has to name the registers, which are only known now: insert the assignments into the body.
    // With `--vertex` the coordinates come from the draw's own vertices instead, so the grid injection is skipped.
    if !uv_from_vertex && !uv_regs.is_empty() {
        let mut inject = String::new();
        for reg in &uv_regs {
            // WGSL has no swizzle assignment: overwrite the whole vector, keeping z/w from the uniform.
            inject += &format!("  i.r{reg} = vec4<f32>(uv * f32({scale}), fx.r[{reg}].z, fx.r[{reg}].w);\n");
        }
        wgsl_src = wgsl_src.replace("  i.vpos = p; i.front_facing = ff;", &format!("{inject}  i.vpos = p; i.front_facing = ff;"));
    }
    // The generated module is written next to the other working files (git-ignored), for inspection when a shader
    // fails to compile.
    std::fs::create_dir_all("data/work").ok();
    std::fs::write("data/work/fm2_ps_gpu.wgsl", &wgsl_src).ok();

    // ---- run ----
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()),
        flags: wgpu::InstanceFlags::from_env_or_default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });
    let adapter = match block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Ok(x) => x,
        Err(e) => {
            println!("no GPU adapter: {e:?}");
            std::process::exit(1);
        }
    };
    let info = adapter.get_info();
    println!("GPU: {} ({:?})", info.name, info.backend);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("fm2_ps_gpu"),
        ..Default::default()
    }))
    .unwrap();

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(wgsl_src.clone().into()),
    });

    // Uniform: the constants one probe point uses (the grid varies only the interpolators, as in fm2_ps_run).
    let mut ub = vec![0u8; 256 * 16 + 16 * 16 + 64 * 16 + 32 * 16];
    let mut put = |off: usize, v: [f32; 4]| {
        for (k, x) in v.iter().enumerate() {
            ub[off + k * 4..off + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
        }
    };
    for (reg, v) in consts.iter() {
        put(*reg as usize * 16, *v);
    }
    for (reg, v) in rset.iter() {
        put(256 * 16 + *reg as usize * 16, *v);
    }
    for (addr, v) in bset.iter() {
        let w = if *v { 1u32 } else { 0 };
        ub[256 * 16 + 16 * 16 + (*addr as usize / 4) * 16 + (*addr as usize % 4) * 4..][..4].copy_from_slice(&w.to_le_bytes());
    }
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fx"),
        size: ub.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&uniform, 0, &ub);

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        ..Default::default()
    });

    let bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl0,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });

    // One texture per sampler the shader reads; anything the probe did not load is bound 1x1 black.
    // `PS_WHITE=<list>`: those samplers read solid white, the same convention as `fm2_ps_run` (a runtime / shadow
    // sampler the draw does not bind a texture for).
    let white: Vec<u32> = std::env::var("PS_WHITE")
        .map(|l| l.split(',').filter_map(|x| x.trim().parse::<u32>().ok()).collect())
        .unwrap_or_default();
    let mut views = Vec::new();
    let mut tex_bgl_entries = Vec::new();
    for (k, &(tf, dim)) in tex_keys.iter().enumerate() {
        let t = tex.get(&tf);
        let (w, h, rgba) = match t {
            _ if white.contains(&tf) => (1, 1, vec![255, 255, 255, 255]),
            Some(t) => (t.w as u32, t.h as u32, t.rgba.clone()),
            None => (1, 1, vec![0, 0, 0, 255]),
        };
        let tex_obj = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex_obj, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        views.push(tex_obj.create_view(&wgpu::TextureViewDescriptor::default()));
        tex_bgl_entries.push(wgpu::BindGroupLayoutEntry {
            binding: k as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        });
        let _ = dim;
    }
    let bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &tex_bgl_entries });
    let entries: Vec<wgpu::BindGroupEntry> = views.iter().enumerate().map(|(k, v)| wgpu::BindGroupEntry { binding: k as u32, resource: wgpu::BindingResource::TextureView(v) }).collect();
    let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &bgl1, entries: &entries });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bgl0), Some(&bgl1)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let n32 = n as u32;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: n32, height: n32, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let padded = (n32 * 16).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded as u64) * (n32 as u64),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&pipeline);
        rp.set_bind_group(0, &bg0, &[]);
        if !views.is_empty() {
            rp.set_bind_group(1, &bg1, &[]);
        }
        rp.draw(0..3, 0..1);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(n32) },
        },
        wgpu::Extent3d { width: n32, height: n32, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);

    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    let data_out = readback.slice(..).get_mapped_range().to_vec();

    // ---- report, in fm2_ps_run's format ----
    let mut sum = [0.0f64; 3];
    let mut cnt = 0usize;
    for y in 0..n {
        for x in 0..n {
            let o = y * padded as usize + x * 16;
            let f = |k: usize| f32::from_le_bytes(data_out[o + k * 4..o + k * 4 + 4].try_into().unwrap());
            let px = [f(0), f(1), f(2)];
            if dump {
                let (u, v) = ((x as f32 + 0.5) / n as f32 * scale, (y as f32 + 0.5) / n as f32 * scale);
                // Unclamped, like the CPU interpreter's dump, so the two can be compared value for value.
                println!("({u:.3},{v:.3}) -> [{:.6},{:.6},{:.6}]", px[0], px[1], px[2]);
            }
            for k in 0..3 {
                sum[k] += px[k] as f64;
            }
            cnt += 1;
        }
    }
    let mean: Vec<f64> = sum.iter().map(|s| s / cnt as f64).collect();
    println!("GPU mean oC0.rgb over {cnt} points: {:.3?}   (as sRGB 0..255: {:?})", mean, mean.iter().map(|v| (255.0 * v.clamp(0.0, 1.0)).round() as u8).collect::<Vec<_>>());
}
