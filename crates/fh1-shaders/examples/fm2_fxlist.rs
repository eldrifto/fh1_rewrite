//! Probe: list the techniques of the embedded effects in an FM2 `default.xex` whose names contain a filter. `fm2_fxlist <default.xex> [filter]`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let x = fh1_shaders::xex::load(&d).unwrap();
    let filt = a.get(2).map(|s| s.to_lowercase());
    for (va, bytes) in x.embedded_effects() {
        let Ok((fx, _)) = fh1_shaders::effect::Effect::parse_embedded(&bytes) else { continue };
        for t in &fx.techniques {
            if filt.as_ref().is_none_or(|f| t.name.to_lowercase().contains(f)) {
                println!("effect @{va:08x} technique {} passes {}", t.name, t.passes.len());
            }
        }
    }
}
