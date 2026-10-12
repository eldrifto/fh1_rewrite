//! Scratch probe: sampler registers + roles of a track `.fxobj`.
//! `cargo run -p fh1-shaders --example fxobj_samplers -- <file.fxobj>`
use fh1_shaders::container::RegisterSet;
use fh1_shaders::effect::Effect;

fn main() {
    let p = std::env::args().nth(1).unwrap();
    let d = std::fs::read(&p).unwrap();
    let fx = Effect::parse(&d).unwrap();
    println!("{p}: techniques {:?}", fx.techniques.iter().map(|t| &t.name).collect::<Vec<_>>());
    for t in &fx.techniques {
        for (pi, pass) in t.passes.iter().enumerate() {
            println!(" technique {} pass {pi}: ps {:?} vs {:?}", t.name, pass.ps, pass.vs);
            for (stage, sh) in [("vs", pass.vs), ("ps", pass.ps)] {
                if let Some(sh) = sh.and_then(|i| fx.shaders.get(i)) {
                    for c in sh.constants.iter().filter(|c| c.set == RegisterSet::Sampler) {
                        println!("    {stage} sampler reg {} {:?} {}", c.register, c.class, c.name);
                    }
                    let cf: Vec<_> = sh.constants.iter().filter(|c| c.set == RegisterSet::Float4).map(|c| format!("{}@{}", c.name, c.register)).collect();
                    println!("    {stage} float4: {cf:?}");
                    if stage == "vs" && t.name == "Default" {
                        println!("    vs elements {:?}", sh.vertex_elements);
                        println!("    vs interp   {:?}", sh.interpolators);
                    }
                    if stage == "ps" && t.name == "Default" {
                        println!("    ps interp   {:?}", sh.interpolators);
                    }
                }
            }
        }
    }
}
