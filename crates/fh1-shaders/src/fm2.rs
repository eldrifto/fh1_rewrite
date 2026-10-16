//! Forza Motorsport 2 shaders that live in the `.gpu` tail of a `.rmb.bin` model (the `.data` containers are stubs).
//!
//! VERIFIED on Nurburgring and Silverstone: after the vertex and index buffers the tail holds, per draw group, a vertex shader (up to eight
//! literal `vec4`s = the registers `c248..c255`, then microcode), a generic 0x30-byte pass-through stub vertex shader, and a pixel shader
//! (the same literal prefix). The vertex shaders fetch the texture coordinates as **integers** (`16_16 signed int`) and multiply them by
//! a literal (`mul oN.xyz_, rK.xyzz, c255.yyyy`): that literal is the per-draw uv scale (1/32768 for plain normalised uv, 1/256 or 1/16
//! for tiling surfaces, ...). The pixel shader's `tfetch` says which interpolator `oN` feeds which sampler.

use crate::container::{ShaderBlob, Stage};
use crate::disasm;
use crate::ucode::{self, Instr};

/// One shader found in the tail.
#[derive(Debug, Clone)]
pub struct TailShader {
    pub vertex: bool,
    /// Byte offset of the first microcode dword in the `.gpu` section.
    pub code_offset: usize,
    /// The literal vec4s before the code; the last one is `c255`.
    pub literals: Vec<[f32; 4]>,
    /// Microcode length in dwords.
    pub dwords: usize,
    pub text: String,
}

fn be(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

/// Length in dwords of a well-formed microcode program starting at `words[0]`, or None when it does not decode as one.
fn code_len(words: &[u32]) -> Option<usize> {
    let cf = ucode::control_flow(words);
    let execs: Vec<_> = cf.iter().filter(|c| c.is_exec() && c.count > 0).collect();
    // Every compiled shader opens with an exec (a literal vec4 read as control flow starts with no-ops).
    if !cf.first().is_some_and(|c| c.is_exec()) {
        return None;
    }
    let first = execs.iter().map(|c| c.address as usize).min()?;
    // The control-flow section is exactly the dwords before the first instruction (two 48-bit entries per three dwords).
    if first < 2 || cf.len() != 2 * first {
        return None;
    }
    // Exec ranges must tile the instruction area without leaving it.
    let end = execs.iter().map(|c| c.address as usize + c.count as usize).max()?;
    if end * 3 > words.len() || !cf.iter().any(|c| c.ends()) {
        return None;
    }
    // Every exec range starts where an earlier one ended (instructions are laid out in execution order).
    let mut next = first;
    let mut ranges: Vec<(usize, usize)> = execs.iter().map(|c| (c.address as usize, c.address as usize + c.count as usize)).collect();
    ranges.sort();
    for (a, b) in ranges {
        if a != next {
            return None;
        }
        next = b;
    }
    Some(end * 3)
}

fn disassemble(words: Vec<u32>, vertex: bool) -> String {
    let blob = ShaderBlob {
        stage: if vertex { Stage::Vertex } else { Stage::Pixel },
        file_offset: 0,
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
    disasm::disassemble(&blob)
}

/// The shaders of the tail starting at byte `from` of the `.gpu` section (after the last vertex / index buffer), in file order. The generic
/// stub vertex shader is skipped.
pub fn tail_shaders(gpu: &[u8], from: usize) -> Vec<TailShader> {
    let mut out = Vec::new();
    // Not 16-aligned: stubs and terminators shift the grid (the tail of a big model starts at an arbitrary byte).
    let mut t = (from + 3) & !3;
    while t + 16 <= gpu.len() {
        let d = be(gpu, t).unwrap_or(0);
        if d == 0 {
            t += 4;
            continue;
        }
        // The pass-through stub vertex shader (8 dwords from `0x1002c200`) and `4e4a0000` + GUID terminators.
        if d == 0x1002_c200 {
            t += 0x20;
            continue;
        }
        if d == 0x4e4a_0000 {
            t += 12;
            continue;
        }
        let mut found = None;
        for n in 0..=8usize {
            let s = t + 16 * n;
            if s + 12 > gpu.len() {
                break;
            }
            let words: Vec<u32> = (s..gpu.len().saturating_sub(3).min(s + 0x2000)).step_by(4).filter_map(|k| be(gpu, k)).collect();
            if let Some(len) = code_len(&words) {
                found = Some((n, s, len, words));
                break;
            }
        }
        let Some((n, s, len, mut words)) = found else {
            t += 4;
            continue;
        };
        words.truncate(len);
        let literals: Vec<[f32; 4]> = (0..n).map(|k| [0, 1, 2, 3].map(|j| f32::from_bits(be(gpu, t + 16 * k + 4 * j).unwrap_or(0)))).collect();
        let vertex = ucode::control_flow(&words).iter().filter(|c| c.is_exec() && c.count > 0).any(|c| {
            ucode::exec_instructions(&words, c).iter().any(|(_, i)| matches!(i, Instr::Fetch(ucode::Fetch::Vertex(_))))
        });
        let dwords = words.len();
        let text = disassemble(words, vertex);
        // A block that is neither a vertex shader (vfetch) nor a pixel shader (writes a colour) is a pass-through stub.
        if vertex || text.contains("oC0") {
            out.push(TailShader { vertex, code_offset: s, literals, dwords, text });
        }
        t = s + len * 4;
    }
    out
}

/// How a vertex shader output (interpolator) is computed from a 16-bit texture coordinate pair `(h0, h1)` (the first and second halfword of the
/// vertex dword, `u` and `v` as the importer reads them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UvMap {
    /// `(h0, h1) * scale`.
    Scale(f32),
    /// The raw integer pair (`max oN, rK, rK`): tile counts when small, not a coordinate at all when large.
    Raw,
    /// `u' = M0.x h0 + M0.y h1 + M0.w`, `v' = M1.x h0 + M1.y h1 + M1.w` with the matrix rows in the vertex float constants `reg` and `reg + 1`
    /// (the material's `c_place2d_N_Mtx`, values from the model's parameter blocks).
    Matrix { reg: u32 },
}

/// Where one vertex shader output (interpolator) comes from: the 16-bit pair at dword `dword` of the vertex, mapped by `map`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvSource {
    pub dword: u32,
    pub map: UvMap,
}

fn tokens(line: &str) -> Vec<&str> {
    let mut t: Vec<&str> = line.split_whitespace().collect();
    // Leading instruction index, `+` (co-issued scalar op) and predicate tokens.
    while t.first().is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()) || *s == "+" || s.starts_with("(p0)") || s.starts_with("(!p0)")) {
        t.remove(0);
    }
    t
}

fn reg_of(tok: &str, prefix: char) -> Option<u32> {
    let t = tok.trim_matches(|c| c == ',' || c == '|' || c == '-');
    let t = t.strip_prefix(prefix)?;
    t.split('.').next()?.parse().ok()
}

/// The texture coordinate sources of a vertex shader: interpolator register -> source, for the outputs that are a plain
/// `mul oN.xyz_, rK.xyzz, cC.kkkk` of a 16-bit fetch by a literal, or a `dp4` pair by consecutive constant registers (a placement matrix).
pub fn vs_uv_sources(sh: &TailShader) -> std::collections::BTreeMap<u32, UvSource> {
    let mut fetched: std::collections::BTreeMap<u32, u32> = Default::default();
    let mut out = std::collections::BTreeMap::new();
    // (output, component) -> (source register, constant register) of `dp4 oN.c___, rK.wzxy, cM.wzxy`.
    let mut rows: std::collections::BTreeMap<(u32, char), (u32, u32)> = Default::default();
    for line in sh.text.lines() {
        let t = tokens(line);
        match t.first().copied() {
            Some("vfetch_full") | Some("vfetch_mini") => {
                let Some(dst) = t.get(1).and_then(|d| reg_of(d, 'r')) else { continue };
                let off = t.iter().find_map(|s| s.strip_prefix("off=")?.parse::<u32>().ok());
                if t.iter().any(|s| *s == "fmt=16_16") {
                    if let Some(off) = off {
                        fetched.insert(dst, off);
                    }
                }
            }
            Some("mul") if t.len() >= 4 && t[1].starts_with('o') && !t[1].starts_with("oPos") => {
                let (Some(o), Some(src), Some(c)) = (reg_of(t[1], 'o'), reg_of(t[2], 'r'), t[3].trim_end_matches(',').strip_prefix('c')) else { continue };
                let (num, comp) = c.split_once('.').unwrap_or((c, "x"));
                let (Ok(creg), Some(k)) = (num.parse::<usize>(), comp.chars().next().and_then(|ch| "xyzw".find(ch))) else { continue };
                if creg > 255 {
                    continue;
                }
                let Some(idx) = sh.literals.len().checked_sub(1 + 255usize - creg) else { continue };
                if let (Some(dword), Some(lit)) = (fetched.get(&src), sh.literals.get(idx)) {
                    out.insert(o, UvSource { dword: *dword, map: UvMap::Scale(lit[k]) });
                }
            }
            // `max oN, rK, rK` is a move: the raw integer pair, unscaled (tile counts).
            Some("max") if t.len() >= 4 && t[1].starts_with('o') && !t[1].starts_with("oPos") => {
                if let (Some(o), Some(a), Some(b)) = (reg_of(t[1], 'o'), reg_of(t[2], 'r'), reg_of(t[3], 'r')) {
                    if a == b {
                        if let Some(dword) = fetched.get(&a) {
                            out.insert(o, UvSource { dword: *dword, map: UvMap::Raw });
                        }
                    }
                }
            }
            Some("dp4") if t.len() >= 4 && t[1].starts_with('o') && !t[1].starts_with("oPos") => {
                let (Some(o), Some(src), Some(c)) = (reg_of(t[1], 'o'), reg_of(t[2], 'r'), reg_of(t[3], 'c')) else { continue };
                // The destination mask names the component: `o4.x___`.
                let comp = t[1].split('.').nth(1).and_then(|m| m.chars().zip("xyzw".chars()).find(|(c, _)| *c != '_').map(|(_, n)| n));
                if let Some(comp) = comp {
                    rows.insert((o, comp), (src, c));
                }
            }
            _ => {}
        }
    }
    for (&(o, comp), &(src, c)) in &rows {
        if comp != 'x' {
            continue;
        }
        if let (Some(&(src_y, c_y)), Some(dword)) = (rows.get(&(o, 'y')), fetched.get(&src)) {
            if src_y == src && c_y == c + 1 {
                out.entry(o).or_insert(UvSource { dword: *dword, map: UvMap::Matrix { reg: c } });
            }
        }
    }
    out
}

/// Component mask of a register operand token: `r1.___w` (a destination mask, `_` = untouched) or `r1.xyx` (a source swizzle).
fn comps(tok: &str) -> u8 {
    let t = tok.trim_matches(|c| c == ',' || c == '|' || c == '-');
    let Some((_, sw)) = t.split_once('.') else { return 0xF };
    // A destination mask marks the components it writes by position (`r1.___x` writes w, taking the fetch's x); a source swizzle has no `_`.
    if sw.contains('_') {
        return sw.chars().enumerate().filter(|(_, c)| *c != '_').fold(0u8, |m, (i, _)| m | (1 << i));
    }
    sw.chars().filter_map(|c| "xyzw".find(c)).fold(0u8, |m, i| m | (1 << i))
}

/// The register components a 2D texture fetch reads its coordinate from: the first two letters of the source swizzle (`r2.zwz` reads z and w).
fn coord_comps(tok: &str) -> u8 {
    let t = tok.trim_matches(|c| c == ',' || c == '|' || c == '-');
    let Some((_, sw)) = t.split_once('.') else { return 0b11 };
    sw.chars().take(2).filter_map(|c| "xyzw".find(c)).fold(0u8, |m, i| m | (1 << i))
}

/// One `tfetch` of a pixel shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsSampler {
    pub index: u32,
    pub cube: bool,
    /// The interpolator register the coordinate is read from directly (the components the fetch reads were not written by any earlier
    /// instruction); None when the coordinate is computed (an atlas transform).
    pub direct: Option<u32>,
    /// An atlas lookup: the coordinate is `frac(interpolator) * cA.xy + cA.zw` (or without the `frac`): (interpolator register, constant
    /// register A). The draw's constant A holds the sub-rectangle `[scale u, scale v, offset u, offset v]` of the texture.
    pub atlas: Option<(u32, u32)>,
}

/// Every texture fetch of the pixel shader, in sampler order.
pub fn ps_samplers(sh: &TailShader) -> Vec<PsSampler> {
    let mut written: std::collections::BTreeMap<u32, u8> = Default::default();
    let mut out: std::collections::BTreeMap<u32, PsSampler> = Default::default();
    // Registers holding `frac(interpolator)` and registers holding an atlas coordinate (interpolator, constant register).
    let mut fracs: std::collections::BTreeMap<u32, u32> = Default::default();
    let mut atlas: std::collections::BTreeMap<u32, (u32, u32)> = Default::default();
    let direct_src = |written: &std::collections::BTreeMap<u32, u8>, tok: &str| -> Option<u32> {
        let r = reg_of(tok, 'r')?;
        (written.get(&r).copied().unwrap_or(0) & coord_comps(tok) == 0).then_some(r)
    };
    for line in sh.text.lines() {
        // A co-issued scalar op after a `+` is skipped here (the atlas arithmetic is vector ops).
        let t = tokens(line);
        let Some(op) = t.first().copied() else { continue };
        if op.starts_with("tfetch") {
            // tfetch2D rD.mask, rS.swz, tfN
            let (Some(src), Some(tf)) = (t.get(2).and_then(|s| reg_of(s, 'r')), t.get(3).and_then(|s| s.trim_start_matches("tf").trim_end_matches(',').parse::<u32>().ok())) else { continue };
            let cube = op.starts_with("tfetchCube");
            let need = if cube { 0b1110 } else { coord_comps(t[2]) };
            let direct = (!cube && written.get(&src).copied().unwrap_or(0) & need == 0).then_some(src);
            let at = if cube || direct.is_some() { None } else { atlas.get(&src).copied() };
            out.entry(tf).or_insert(PsSampler { index: tf, cube, direct, atlas: at });
        }
        if let (Some(d), true) = (t.get(1).and_then(|s| reg_of(s, 'r')), !line.trim_start().starts_with('+')) {
            if op == "frc" {
                if let Some(s) = t.get(2).and_then(|tok| direct_src(&written, tok)) {
                    fracs.insert(d, s);
                }
            } else if op == "mad" {
                // mad rD.xy__, rA.xyyy, cC.xyyy, cC.zwww
                let (a, c1, c2) = (t.get(2).and_then(|s| reg_of(s, 'r')), t.get(3).and_then(|s| reg_of(s, 'c')), t.get(4).and_then(|s| reg_of(s, 'c')));
                if let (Some(a), Some(c1), Some(c2)) = (a, c1, c2) {
                    if c1 == c2 {
                        if let Some(i) = fracs.get(&a).copied().or_else(|| t.get(2).and_then(|tok| direct_src(&written, tok))) {
                            atlas.insert(d, (i, c1));
                        }
                    }
                }
            }
        }
        if let Some(d) = t.get(1).and_then(|s| reg_of(s, 'r')) {
            *written.entry(d).or_insert(0) |= comps(t[1]);
        }
    }
    out.into_values().collect()
}

/// The pixel shader's samplers that read their coordinate directly: sampler index -> interpolator register.
pub fn ps_sampler_sources(sh: &TailShader) -> std::collections::BTreeMap<u32, u32> {
    ps_samplers(sh).into_iter().filter_map(|s| Some((s.index, s.direct?))).collect()
}

/// One draw group in a model's `.data`: the pixel shader container, the real vertex shader container (`0x102A1141`) and the placement matrices
/// (`c_place2d_N_Mtx`) its constant table names.
#[derive(Debug, Clone)]
pub struct Group {
    pub ps: usize,
    pub vs: Option<usize>,
    /// Microcode sizes the containers declare (dwords), to find their code in the tail.
    pub ps_dwords: usize,
    pub vs_dwords: usize,
    /// (name, first register) of the vertex shader's `c_place2d_*` constants in constant table order.
    pub place2d: Vec<(String, u32)>,
}

/// The shader container groups of a model's `.data`, in order. A group is a pixel container (`0x102A1100`) followed by its vertex containers.
pub fn data_groups(data: &[u8]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for o in (0..data.len().saturating_sub(8)).step_by(4) {
        let m = be(data, o).unwrap_or(0);
        if m & 0xFFFF_FF00 != 0x102A_1100 {
            continue;
        }
        match m & 0xFF {
            0x00 => {
                let mut copy = data[o..].to_vec();
                copy[3] = 0;
                let ps_dwords = ShaderBlob::parse(&copy, o).map(|b| b.microcode.len()).unwrap_or(0);
                out.push(Group { ps: o, vs: None, ps_dwords, vs_dwords: 0, place2d: Vec::new() })
            }
            0x41 => {
                let Some(g) = out.last_mut() else { continue };
                g.vs = Some(o);
                let mut copy = data[o..].to_vec();
                copy[3] = 1;
                if let Ok(b) = ShaderBlob::parse(&copy, o) {
                    g.vs_dwords = b.microcode.len();
                    g.place2d = b.constants.iter().filter(|c| c.name.starts_with("c_place2d_")).map(|c| (c.name.clone(), c.register as u32)).collect();
                }
            }
            _ => {}
        }
    }
    out
}

/// The placement matrices in a model's `.data` (the material parameter blocks): the first three rows (`[a, b, 0, tx]`, `[c, d, 0, ty]`,
/// `[0, 0, e, 0]`; a 3D one for the tangent frame has other entries) of every 4x4 block whose last row is `(0, 0, 0, 1)`, in file order. The
/// header's bounding block (offset 0x34) is skipped.
pub fn matrix_blocks(data: &[u8]) -> Vec<[[f32; 4]; 3]> {
    let f = |o: usize| -> Option<[f32; 4]> {
        let r = [0, 1, 2, 3].map(|k| f32::from_bits(be(data, o + 4 * k).unwrap_or(0x7fc0_0000)));
        r.iter().all(|x| x.is_finite()).then_some(r)
    };
    let mut out = Vec::new();
    let mut o = 0;
    while o + 64 <= data.len() {
        let rows: Option<Vec<[f32; 4]>> = (0..4).map(|k| f(o + 16 * k)).collect();
        match rows {
            Some(r) if o >= 0x80 && r[3] == [0.0, 0.0, 0.0, 1.0] && r[0].iter().any(|x| *x != 0.0) && r.iter().flatten().all(|x| x.abs() < 1e4) => {
                out.push([r[0], r[1], r[2]]);
                o += 64;
            }
            _ => o += 4,
        }
    }
    out
}

/// Finds the tail vertex and pixel shader of every group by microcode size (a container declares its program 3 dwords longer than the tail
/// copy). Several groups with the same size take the tail copies of that size in order; when the tail holds fewer copies than groups
/// (identical shaders are stored once) the extra groups reuse the first. Returns indices into `shaders`.
pub fn pair_groups(groups: &[Group], shaders: &[TailShader]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut used: std::collections::BTreeMap<(bool, usize), usize> = Default::default();
    let mut pick = |vertex: bool, declared: usize| -> Option<usize> {
        let want = declared.checked_sub(3)?;
        let cands: Vec<usize> = (0..shaders.len()).filter(|&i| shaders[i].vertex == vertex && shaders[i].dwords == want).collect();
        let n = used.entry((vertex, want)).or_insert(0);
        let r = cands.get(*n).or(cands.first()).copied();
        *n += 1;
        r
    };
    let mut out: Vec<(Option<usize>, Option<usize>)> = groups.iter().map(|g| (pick(true, g.vs_dwords), pick(false, g.ps_dwords))).collect();
    // A group without a real vertex container (`vs_dwords == 0`) takes the tail vertex shaders no other group claimed, in order.
    for vertex in [true, false] {
        let claimed: std::collections::BTreeSet<usize> = out.iter().filter_map(|p| if vertex { p.0 } else { p.1 }).collect();
        let mut spare = (0..shaders.len()).filter(|i| shaders[*i].vertex == vertex && !claimed.contains(i));
        for (g, p) in groups.iter().zip(out.iter_mut()) {
            let missing = if vertex { p.0.is_none() && g.vs_dwords == 0 } else { p.1.is_none() && g.ps_dwords == 0 };
            if missing {
                let s = spare.next();
                if vertex { p.0 = s } else { p.1 = s }
            }
        }
    }
    out
}

/// Whether a pixel shader's output alpha (`oC0.w`) is a constant register rather than something computed from a texture: FM2 packs other
/// data (occlusion, gloss) into the alpha of colour atlases, and those shaders ignore it for transparency. False when the shader never
/// writes a constant alpha, or writes anything else to it.
pub fn ps_alpha_constant(sh: &TailShader) -> bool {
    let (mut constant, mut other) = (0, 0);
    for line in sh.text.lines() {
        // A co-issued scalar op follows a `+` on the same line.
        for part in line.split(" + ") {
            let t = tokens(part);
            let Some(dst) = t.get(1).filter(|d| d.starts_with("oC0")) else { continue };
            let mask = dst.split('.').nth(1).unwrap_or("xyzw").trim_end_matches(',');
            if mask.chars().nth(3) != Some('w') {
                continue;
            }
            let src = t.get(2).map(|x| x.trim_start_matches(|c| c == '-' || c == '|')).unwrap_or("");
            if src.starts_with('c') {
                constant += 1;
            } else {
                other += 1;
            }
        }
    }
    constant > 0 && other == 0
}
