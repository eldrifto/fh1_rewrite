//! Probe: find the shader blocks in the `.gpu` tail of FM2 models (a 16-byte literal vec4 `c255` starting with 1.0, then microcode) and print each
//! vertex shader's literal and its vertex fetches / texture-coordinate maths, to see where the per-draw uv scale lives.
//! `cargo run -p fh1-shaders --example fm2_vs_scan -- <bin.zip> [model number ...]`
use fh1_formats::zip::Archive;
use fh1_shaders::container::{ShaderBlob, Stage};
use std::collections::BTreeMap;

fn be(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let only: Vec<usize> = a[2..].iter().filter_map(|v| v.parse().ok()).collect();
    let mut census: BTreeMap<String, usize> = BTreeMap::new();
    for e in ar.entries.clone() {
        let name = e.name.to_ascii_lowercase();
        let Some(num) = name.strip_suffix(".rmb.bin").and_then(|n| n.rsplit('.').next()).and_then(|n| n.parse::<usize>().ok()) else { continue };
        if !only.is_empty() && !only.contains(&num) {
            continue;
        }
        let d = ar.read(&e).unwrap();
        let Some(g) = section(&d, b".gpu") else { continue };
        let gpu = &d[0x190..0x190 + g];
        // Candidate block starts: 16-aligned `1.0` literals in the last part of the gpu.
        let from = g.saturating_sub(0x1800) & !15;
        let mut starts: Vec<usize> = (from..g.saturating_sub(32)).step_by(16).filter(|&o| be(gpu, o) == Some(0x3f800000)).collect();
        starts.push(g);
        for w in starts.windows(2) {
            let (o, next) = (w[0], w[1]);
            let lit: Vec<f32> = (0..4).map(|k| f32::from_bits(be(gpu, o + 4 * k).unwrap())).collect();
            let words: Vec<u32> = (o + 16..next).step_by(4).filter_map(|k| be(gpu, k)).collect();
            let cf = fh1_shaders::ucode::control_flow(&words);
            if !cf.first().is_some_and(|c| c.is_exec() && c.address > 0 && (c.address as usize) * 3 < words.len()) {
                continue;
            }
            let blob = ShaderBlob { stage: Stage::Vertex, file_offset: 0, constants: Vec::new(), float_defs: Vec::new(), int_defs: Vec::new(), vertex_elements: Vec::new(), interpolators: Vec::new(), vpos_register: None, ps_outputs: 0, microcode: words, target: String::new() };
            let text = fh1_shaders::disasm::disassemble(&blob);
            let is_vs = text.contains("vfetch");
            let key: Vec<String> = text
                .lines()
                .filter(|l| is_vs && (l.contains("vfetch") || l.contains("c255") || l.contains("exp")))
                .map(|l| l.trim().split_whitespace().skip(1).collect::<Vec<_>>().join(" "))
                .collect();
            let summary = format!("{} lit {:?} :: {}", if is_vs { "VS" } else { "PS" }, lit, key.join(" | "));
            if only.is_empty() {
                *census.entry(summary).or_default() += 1;
            } else {
                println!("model {num} gpu {o:#x}: {summary}");
            }
        }
    }
    for (k, n) in census {
        println!("{n:5} x {k}");
    }
}
