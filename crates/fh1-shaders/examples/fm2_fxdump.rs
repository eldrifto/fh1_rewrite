//! Probe: print one technique's pass shaders (constant table + disassembly) from the embedded effects of an FM2 `default.xex`.
//! `fm2_fxdump <default.xex> <technique>` (output is derived from game code: keep it in git-ignored `data/work`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let x = fh1_shaders::xex::load(&d).unwrap();
    for (va, bytes) in x.embedded_effects() {
        let Ok((fx, _)) = fh1_shaders::effect::Effect::parse_embedded(&bytes) else { continue };
        let Some(t) = fx.technique(&a[2]) else { continue };
        println!("effect @{va:08x} technique {}", t.name);
        for p in &t.passes {
            println!("pass {} states {:x?}", p.name, p.render_states);
            for (label, ix) in [("VS", p.vs), ("PS", p.ps)] {
                let Some(i) = ix else { continue };
                let sh = &fx.shaders[i];
                println!("--- {label} constants:");
                for c in &sh.constants {
                    println!("  {:?} c{} x{} {} ty{} default {:?}", c.set, c.register, c.count, c.name, c.ty, c.default.as_ref().map(|w| w.iter().map(|v| f32::from_bits(*v)).collect::<Vec<_>>()));
                }
                println!("  defs {:?}", sh.float_defs);
                println!("--- {label} disassembly:\n{}", fh1_shaders::disasm::disassemble(sh));
            }
        }
        return;
    }
}
