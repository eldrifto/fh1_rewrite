//! fm2_ps_run: run one FM2 pixel shader (a draw group's tail shader) on the CPU over a grid of texture coordinates and print the colour it
//! outputs. A probe for the road shaders whose colour is built from constants and several textures (Test Track).
//!
//! `cargo run -p fh1setup --example fm2_ps_run -- <bin.zip> <model> <group> <textures dir> [options]`
//!   `--tex N=ID`          sampler tfN reads texture file ID (`<dir>/<ID as 8 hex>.dds`); repeat
//!   `--c R=x,y,z,w`       float constant cR (overrides the literals); repeat
//!   `--scale S`           the uv grid covers 0..S in both axes (default 1)
//!   `--n N`               grid is N x N points (default 16)
//!   `--r R=x,y,z,w`       interpolator register rR (non-uv inputs: normal, view vector...); repeat
//!   `--b ADDR=0|1`        bool constant (default all 0)
//!   `--dump`              print every sample, not just the mean
//!
//! The Xenos ALU semantics follow `crates/fh1-shaders/src/wgsl.rs` (which spells them out for the WGSL translator).
use fh1_formats::{fm2, zip::Archive};
use fh1_shaders::fm2 as sh;
use fh1_shaders::ucode::{self, alu_swizzle, Alu, CfOp, Fetch, Instr};
use std::collections::HashMap;

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
    fmt: usize,
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
            return Some(Tex { w, h, rgba, fmt });
        }
        _ => return None,
    };
    f.decompress(b.get(148..148 + f.compressed_size(w, h))?, w, h, &mut rgba);
    if fmt == 80 {
        // One channel: replicated, as the sampler hands it to the shader.
        for p in rgba.chunks_exact_mut(4) {
            (p[1], p[2], p[3]) = (p[0], p[0], 255);
        }
    }
    Some(Tex { w, h, rgba, fmt })
}

fn sample(t: &Tex, uv: [f32; 2]) -> [f32; 4] {
    let (fx, fy) = (uv[0] * t.w as f32 - 0.5, uv[1] * t.h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let px = |x: f32, y: f32| {
        let (xi, yi) = ((x as i64).rem_euclid(t.w as i64) as usize, (y as i64).rem_euclid(t.h as i64) as usize);
        let p = &t.rgba[(yi * t.w + xi) * 4..][..4];
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
    };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1.0, y0), px(x0, y0 + 1.0), px(x0 + 1.0, y0 + 1.0));
    std::array::from_fn(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty)
}

fn fx_mul(a: f32, b: f32) -> f32 {
    if a == 0.0 || b == 0.0 { 0.0 } else { a * b }
}
fn fx_max(a: f32, b: f32) -> f32 {
    if a >= b { a } else { b }
}
fn fx_min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}
const FLT_MAX: f32 = 3.4028235e38;

struct M<'a> {
    r: [[f32; 4]; 64],
    p0: bool,
    ps: f32,
    a0: i32,
    consts: HashMap<u32, [f32; 4]>,
    bools: HashMap<u32, bool>,
    tex: &'a HashMap<u32, Tex>,
    out: [f32; 4],
    killed: bool,
    unsupported: std::collections::BTreeSet<String>,
}

impl M<'_> {
    fn base(&self, a: &Alu, i: usize) -> [f32; 4] {
        let reg = a.src_reg[i] as u32;
        if a.src_is_temp[i] {
            let v = self.r[(reg & 0x3F) as usize];
            if reg & 0x80 != 0 { v.map(f32::abs) } else { v }
        } else {
            let v = self.consts.get(&reg).copied().unwrap_or([0.0; 4]);
            if a.abs_constants { v.map(f32::abs) } else { v }
        }
    }
    fn src_vec(&self, a: &Alu, i: usize) -> [f32; 4] {
        let b = self.base(a, i);
        let mut v: [f32; 4] = std::array::from_fn(|k| b[alu_swizzle(a.src_swiz[i], k as u32) as usize]);
        if a.src_neg[i] {
            v = v.map(|x| -x);
        }
        v
    }
    fn src_scalar(&self, a: &Alu, k: u32) -> f32 {
        let b = self.base(a, 2);
        let e = b[alu_swizzle(a.src_swiz[2], k) as usize];
        if a.src_neg[2] { -e } else { e }
    }

    fn alu(&mut self, a: &Alu) {
        if a.predicated && self.p0 != a.pred_condition {
            return;
        }
        let vop = a.vector_op as usize;
        let sop = a.scalar_op;
        let nargs = ucode::VECTOR_OP_ARGS[vop] as usize;
        let s: Vec<[f32; 4]> = (0..nargs).map(|i| self.src_vec(a, i)).collect();
        let shape = ucode::scalar_op_shape(sop);
        let (mut sa, mut sb) = (0.0f32, 0.0f32);
        match shape {
            1 => sa = self.src_scalar(a, 3),
            2 => {
                sa = self.src_scalar(a, 3);
                sb = self.src_scalar(a, 0);
            }
            3 => {
                let reg = a.src_reg[2] as u32;
                let c = self.consts.get(&reg).copied().unwrap_or([0.0; 4]);
                let t = self.r[(a.scalar_const_temp_reg() & 0x3F) as usize];
                let (ca, tb) = (alu_swizzle(a.src_swiz[2], 3) as usize, alu_swizzle(a.src_swiz[2], 0) as usize);
                let wrap = |e: f32| {
                    let e = if a.abs_constants { e.abs() } else { e };
                    if a.src_neg[2] { -e } else { e }
                };
                sa = wrap(c[ca]);
                sb = wrap(t[tb]);
            }
            _ => {}
        }
        let f4 = |f: &dyn Fn(f32, f32) -> f32, x: [f32; 4], y: [f32; 4]| -> [f32; 4] { std::array::from_fn(|k| f(x[k], y[k])) };
        let b = |c: bool| if c { 1.0 } else { 0.0 };
        let mut pv = None;
        let vr: Option<[f32; 4]> = match vop {
            0 => Some(f4(&|x, y| x + y, s[0], s[1])),
            1 => Some(f4(&fx_mul, s[0], s[1])),
            2 | 29 => Some(f4(&fx_max, s[0], s[1])),
            3 => Some(f4(&fx_min, s[0], s[1])),
            4 => Some(f4(&|x, y| b(x == y), s[0], s[1])),
            5 => Some(f4(&|x, y| b(x > y), s[0], s[1])),
            6 => Some(f4(&|x, y| b(x >= y), s[0], s[1])),
            7 => Some(f4(&|x, y| b(x != y), s[0], s[1])),
            8 => Some(s[0].map(|x| x - x.floor())),
            9 => Some(s[0].map(f32::trunc)),
            10 => Some(s[0].map(f32::floor)),
            11 => Some(std::array::from_fn(|k| fx_mul(s[0][k], s[1][k]) + s[2][k])),
            12 => Some(std::array::from_fn(|k| if s[0][k] == 0.0 { s[1][k] } else { s[2][k] })),
            13 => Some(std::array::from_fn(|k| if s[0][k] >= 0.0 { s[1][k] } else { s[2][k] })),
            14 => Some(std::array::from_fn(|k| if s[0][k] > 0.0 { s[1][k] } else { s[2][k] })),
            15 => {
                let d = (0..4).map(|k| fx_mul(s[0][k], s[1][k])).sum::<f32>();
                Some([d; 4])
            }
            16 => {
                let d = (0..3).map(|k| fx_mul(s[0][k], s[1][k])).sum::<f32>();
                Some([d; 4])
            }
            17 => {
                let d = fx_mul(s[0][0], s[1][0]) + fx_mul(s[0][1], s[1][1]) + s[2][0];
                Some([d; 4])
            }
            19 => Some([s[0].iter().copied().fold(f32::MIN, f32::max); 4]),
            20..=23 => {
                let cmp = |x: f32, y: f32| match vop {
                    20 => x == y,
                    21 => x != y,
                    22 => x > y,
                    _ => x >= y,
                };
                pv = Some(s[0][3] == 0.0 && cmp(s[1][3], 0.0));
                let z = s[0][0] == 0.0 && cmp(s[1][0], 0.0);
                Some(if z { [0.0; 4] } else { [s[0][0] + 1.0; 4] })
            }
            24..=27 => {
                let any = (0..4).any(|k| match vop {
                    24 => s[0][k] == s[1][k],
                    25 => s[0][k] > s[1][k],
                    26 => s[0][k] >= s[1][k],
                    _ => s[0][k] != s[1][k],
                });
                if any {
                    self.killed = true;
                }
                Some([b(any); 4])
            }
            28 => Some([1.0, fx_mul(s[0][1], s[1][1]), s[0][2], s[1][3]]),
            _ => {
                self.unsupported.insert(format!("vector op {vop}"));
                None
            }
        };
        let ps = self.ps;
        let sexpr: Option<f32> = match sop {
            0 => Some(sa + sb),
            1 => Some(sa + ps),
            2 => Some(fx_mul(sa, sb)),
            3 => Some(fx_mul(sa, ps)),
            5 | 23 | 24 => Some(if sa >= sb { sa } else { sb }),
            6 => Some(if sa < sb { sa } else { sb }),
            7 => Some(b(sa == 0.0)),
            8 => Some(b(sa > 0.0)),
            9 => Some(b(sa >= 0.0)),
            10 => Some(b(sa != 0.0)),
            11 => Some(sa - sa.floor()),
            12 => Some(sa.trunc()),
            13 => Some(sa.floor()),
            14 => Some(sa.exp2()),
            15 | 16 => {
                let l = sa.log2();
                Some(if l.is_finite() { l } else if sop == 15 { -FLT_MAX } else { l })
            }
            17..=19 => {
                let r = 1.0 / sa;
                Some(if r.is_finite() { r } else { FLT_MAX })
            }
            20..=22 => {
                let r = 1.0 / sa.sqrt();
                Some(if r.is_finite() { r } else { FLT_MAX })
            }
            25 => Some(sa - sb),
            26 => Some(sa - ps),
            27 => Some(b(sa != 0.0)),
            28 => Some(b(sa == 0.0)),
            29 => Some(b(sa <= 0.0)),
            30 => Some(b(sa < 0.0)),
            31 | 32 | 33 | 34 => Some(0.0),
            40 => Some(sa.sqrt()),
            42 | 43 => Some(fx_mul(sa, sb)),
            44 | 45 => Some(sa + sb),
            46 | 47 => Some(sa - sb),
            48 => Some(sa.sin()),
            49 => Some(sa.cos()),
            50 => Some(ps),
            _ => {
                self.unsupported.insert(format!("scalar op {sop}"));
                None
            }
        };
        if let Some(p) = pv {
            self.p0 = p;
        }
        match sop {
            27 => self.p0 = sa == 0.0,
            28 => self.p0 = sa != 0.0,
            29 => self.p0 = sa > 0.0,
            30 => self.p0 = sa >= 0.0,
            33 => self.p0 = false,
            _ => {}
        }
        let sr = sexpr.unwrap_or(ps);
        let clamp = |v: [f32; 4], c: bool| if c { v.map(|x| x.clamp(0.0, 1.0)) } else { v };
        let write = |m: &mut M, export: bool, dest: u8, mask: u8, v: [f32; 4]| {
            if mask == 0 {
                return;
            }
            let target: &mut [f32; 4] = if export {
                if dest <= 3 {
                    if dest == 0 {
                        &mut m.out
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            } else {
                &mut m.r[(dest & 0x3F) as usize]
            };
            for k in 0..4 {
                if mask & (1 << k) != 0 {
                    target[k] = v[k];
                }
            }
        };
        if let Some(v) = vr {
            write(self, a.export, a.vector_dest, a.vector_result_mask(), clamp(v, a.vector_clamp));
        }
        write(self, a.export, if a.export { a.vector_dest } else { a.scalar_dest }, a.scalar_result_mask(), clamp([sr; 4], a.scalar_clamp));
        if a.export {
            let (c0, c1) = (a.const0_mask(), a.const1_mask());
            write(self, true, a.vector_dest, c0, [0.0; 4]);
            write(self, true, a.vector_dest, c1, [1.0; 4]);
        }
        if sop != 50 && sexpr.is_some() {
            self.ps = if a.scalar_clamp { sr.clamp(0.0, 1.0) } else { sr };
        }
    }

    fn tfetch(&mut self, t: &ucode::TextureFetch) {
        if t.predicated && self.p0 != t.pred_condition {
            return;
        }
        let src = self.r[(t.src_reg & 0x3F) as usize];
        let sw: Vec<usize> = (0..3).map(|i| ((t.src_swiz >> (2 * i)) & 3) as usize).collect();
        if t.opcode != 1 {
            if t.opcode == 24 {
                return;
            }
            self.unsupported.insert(format!("fetch opcode {}", t.opcode));
            return;
        }
        let value = match self.tex.get(&(t.const_index as u32)) {
            Some(tex) if t.dimension == 1 => sample(tex, [src[sw[0]], src[sw[1]]]),
            _ => [0.0; 4],
        };
        let d = &mut self.r[(t.dst_reg & 0x3F) as usize];
        for k in 0..4 {
            let s = (t.dst_swiz >> (3 * k)) & 7;
            match s {
                0..=3 => d[k] = value[s as usize],
                4 => d[k] = 0.0,
                5 => d[k] = 1.0,
                7 => {}
                _ => d[k] = 0.0,
            }
        }
    }

    fn run(&mut self, words: &[u32]) {
        let cfs = ucode::control_flow(words);
        let mut pc = 0usize;
        let mut guard = 0;
        while pc < cfs.len() && guard < 4096 {
            guard += 1;
            let cf = cfs[pc];
            match cf.op {
                CfOp::Exec { .. } | CfOp::CondExec { .. } | CfOp::CondExecPred { .. } => {
                    let go = match cf.op {
                        CfOp::CondExec { bool_addr, condition, .. } => self.bools.get(&(bool_addr as u32)).copied().unwrap_or(false) == condition,
                        CfOp::CondExecPred { condition, .. } => self.p0 == condition,
                        _ => true,
                    };
                    if go {
                        for (_, ins) in ucode::exec_instructions(words, &cf) {
                            match ins {
                                Instr::Alu(a) => self.alu(&a),
                                Instr::Fetch(Fetch::Texture(t)) => self.tfetch(&t),
                                Instr::Fetch(Fetch::Vertex(_)) => {}
                            }
                        }
                    }
                    if cf.ends() && go {
                        return;
                    }
                    pc += 1;
                }
                CfOp::CondJmp { target, unconditional, predicated, bool_addr, condition } => {
                    let c = if unconditional {
                        true
                    } else if predicated {
                        self.p0 == condition
                    } else {
                        self.bools.get(&(bool_addr as u32)).copied().unwrap_or(false) == condition
                    };
                    pc = if c { target as usize } else { pc + 1 };
                }
                _ => pc += 1,
            }
        }
    }
}

fn parse4(s: &str) -> [f32; 4] {
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0), v.get(3).copied().unwrap_or(0.0)]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (zip, model, group, texdir) = (&a[1], a[2].parse::<usize>().unwrap(), a[3].parse::<usize>().unwrap(), std::path::PathBuf::from(&a[4]));
    let (mut texmap, mut cset, mut rset, mut bset): (Vec<(u32, u32)>, Vec<(u32, [f32; 4])>, Vec<(u32, [f32; 4])>, Vec<(u32, bool)>) = Default::default();
    let (mut scale, mut n, mut dump) = (1.0f32, 16usize, false);
    let (mut pvs_path, mut draw_ix): (Option<String>, Option<usize>) = (None, None);
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
            _ => {}
        }
        i += 1;
    }
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

    // --pvs + --draw: the draw's items become the textures of the samplers from tf2 up (tf0 / tf1 are the shadow samplers), and its
    // pixel-shader constants are loaded.
    let mut draw_consts: Vec<(u32, [f32; 4])> = Vec::new();
    if let (Some(pp), Some(di)) = (&pvs_path, draw_ix) {
        let pvs = fm2::parse_pvs(&std::fs::read(pp).unwrap()).unwrap();
        let d = &m.draws[di];
        let list = &pvs.models[model];
        let resolve = |k: u32| list.get(k as usize).map(|&t| (pvs.textures[t as usize].file_id, pvs.textures[t as usize].flags));
        println!("draw {di}: items {:?} -> {:?}", d.items, d.items.iter().map(|&k| resolve(k)).collect::<Vec<_>>());
        // Item k feeds the k-th sampler (by index); items beyond the model's list (shared shadow inputs) have no texture here.
        let mut slots: Vec<u32> = samplers.iter().map(|s| s.index).collect();
        slots.sort();
        slots.dedup();
        for (k, item) in d.items.iter().enumerate() {
            if let (Some(&slot), Some((id, _))) = (slots.get(k), resolve(*item)) {
                if !texmap.iter().any(|t| t.0 == slot) {
                    texmap.push((slot, id));
                }
            }
        }
        for (r, v) in &d.constants {
            draw_consts.push((*r, *v));
        }
        println!("draw constants: {:?}", d.constants);
    }
    let mut tex: HashMap<u32, Tex> = HashMap::new();
    for (slot, id) in &texmap {
        match load_tex(&texdir.join(format!("{id:08x}.dds"))) {
            Some(t) => {
                println!("tf{slot} = texture {id} ({}x{}, fmt {})", t.w, t.h, t.fmt);
                tex.insert(*slot, t);
            }
            None => println!("tf{slot} = texture {id}: not loaded"),
        }
    }
    let mut consts: HashMap<u32, [f32; 4]> = HashMap::new();
    let nl = ps.literals.len() as u32;
    for (k, l) in ps.literals.iter().enumerate() {
        consts.insert(256 - nl + k as u32, *l);
    }
    for (k, v) in draw_consts.iter().chain(cset.iter()) {
        consts.insert(*k, *v);
    }
    let bools: HashMap<u32, bool> = bset.into_iter().collect();

    let mut sum = [0.0f64; 3];
    let mut cnt = 0;
    let mut unsupported = std::collections::BTreeSet::new();
    for yi in 0..n {
        for xi in 0..n {
            let (u, v) = ((xi as f32 + 0.5) / n as f32 * scale, (yi as f32 + 0.5) / n as f32 * scale);
            let mut mm = M { r: [[0.0; 4]; 64], p0: false, ps: 0.0, a0: 0, consts: consts.clone(), bools: bools.clone(), tex: &tex, out: [0.0; 4], killed: false, unsupported: Default::default() };
            for (r, val) in &rset {
                mm.r[*r as usize] = *val;
            }
            for s in &samplers {
                if let Some(r) = s.direct {
                    mm.r[r as usize][0] = u;
                    mm.r[r as usize][1] = v;
                }
            }
            mm.run(&words);
            if std::env::var_os("PS_REGS").is_some() && xi == 0 && yi == 0 {
                for (k, r) in mm.r.iter().enumerate().take(16) {
                    println!("r{k} = {r:.3?}");
                }
                println!("p0={} ps={} out={:?} killed={}", mm.p0, mm.ps, mm.out, mm.killed);
            }
            let _ = mm.a0;
            unsupported.extend(mm.unsupported.iter().cloned());
            if dump {
                println!("({u:.3},{v:.3}) -> {:.3?}{}", mm.out, if mm.killed { " (killed)" } else { "" });
            }
            for k in 0..3 {
                sum[k] += mm.out[k] as f64;
            }
            cnt += 1;
        }
    }
    let mean: Vec<f64> = sum.iter().map(|s| s / cnt as f64).collect();
    println!("mean oC0.rgb over {cnt} points: {:.3?}   (as sRGB 0..255: {:?})", mean, mean.iter().map(|v| (255.0 * v.clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8).collect::<Vec<_>>());
    if !unsupported.is_empty() {
        println!("unsupported: {unsupported:?}");
    }
}
