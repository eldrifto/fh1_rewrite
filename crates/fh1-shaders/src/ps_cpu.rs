//! A CPU interpreter for FM2 pixel shaders (Xenos microcode), used to read what colour a road shader produces from its constants and textures.
//! The ALU semantics are the ones `wgsl.rs` spells out for the WGSL translator (legacy `0 * x = 0` multiplies, the scalar `ps` register,
//! predicates, export write masks). Only what the road shaders use is implemented: a texture fetch is handed to the caller's `sample`
//! closure (sampler index, uv), loops and calls are not supported, `kill` only sets a flag.

use crate::ucode::{self, alu_swizzle, Alu, CfOp, Fetch, Instr};
use std::collections::HashMap;

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

pub struct Machine<'a> {
    pub r: [[f32; 4]; 64],
    pub p0: bool,
    pub ps: f32,
    pub a0: i32,
    pub consts: HashMap<u32, [f32; 4]>,
    pub bools: HashMap<u32, bool>,
    sample: &'a dyn Fn(u32, [f32; 2]) -> [f32; 4],
    pub out: [f32; 4],
    pub killed: bool,
    pub unsupported: std::collections::BTreeSet<String>,
}

impl Machine<'_> {
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
        let write = |m: &mut Machine, export: bool, dest: u8, mask: u8, v: [f32; 4]| {
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
        let value = if t.dimension == 1 { (self.sample)(t.const_index as u32, [src[sw[0]], src[sw[1]]]) } else { [0.0; 4] };
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

    pub fn run(&mut self, words: &[u32]) {
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


impl<'a> Machine<'a> {
    pub fn new(consts: HashMap<u32, [f32; 4]>, bools: HashMap<u32, bool>, sample: &'a dyn Fn(u32, [f32; 2]) -> [f32; 4]) -> Self {
        Machine { r: [[0.0; 4]; 64], p0: false, ps: 0.0, a0: 0, consts, bools, sample, out: [0.0; 4], killed: false, unsupported: Default::default() }
    }
}

/// The register a pixel shader multiplies its final colour by (the fog factor interpolator): the temp source of the last `mad oC0.xyz`.
pub fn fog_register(words: &[u32]) -> Option<u32> {
    let mut found = None;
    for cf in ucode::control_flow(words).iter().filter(|c| c.is_exec()) {
        for (_, ins) in ucode::exec_instructions(words, cf) {
            if let Instr::Alu(a) = ins {
                if a.export && a.vector_dest == 0 && a.vector_op == 11 && a.src_is_temp[1] {
                    found = Some((a.src_reg[1] & 0x3F) as u32);
                }
            }
        }
    }
    found
}
