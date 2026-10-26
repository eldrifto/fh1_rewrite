//! Scratch probe: find vertex-fetch instructions in an FM2 `.gpu` tail by trying candidate microcode start offsets and
//! running the real control-flow decoder. Prints each `vfetch`'s format / sign / normalised / exp_adjust / stride / const
//! index, so two models' VS texture-coord fetches can be compared (the per-draw UV scale is here or in a literal multiply).
//! `cargo run -p fh1-shaders --example fm2_vfetch_scan -- <bin.zip> <model> <gpu_start_hex> <gpu_end_hex>`.
use fh1_formats::zip::Archive;
use fh1_shaders::ucode::{self, Fetch, Instr};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (zip, number) = (&a[1], a[2].parse::<usize>().unwrap());
    let s = usize::from_str_radix(a[3].trim_start_matches("0x"), 16).unwrap();
    let end = usize::from_str_radix(a[4].trim_start_matches("0x"), 16).unwrap();
    let mut ar = Archive::open(zip).unwrap();
    let suffix = format!(".{number:0>5}.rmb.bin");
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    for start in (s..end.saturating_sub(0x40)).step_by(4) {
        let code: Vec<u32> = (start..end).step_by(4).map(|k| u32::from_be_bytes(d[k..k + 4].try_into().unwrap())).collect();
        let cfs = ucode::control_flow(&code);
        let mut vf = Vec::new();
        let mut alu = 0;
        for cf in &cfs {
            if cf.count == 0 {
                continue;
            }
            for (_addr, ins) in ucode::exec_instructions(&code, cf) {
                match ins {
                    Instr::Fetch(Fetch::Vertex(f)) => {
                        if matches!(f.stride, 16 | 20 | 24 | 28 | 32) {
                            vf.push(f);
                        }
                    }
                    Instr::Alu(..) => alu += 1,
                    _ => {}
                }
            }
        }
        if vf.is_empty() {
            continue;
        }
        println!("start {start:#x}: {alu} alu, {} vfetch", vf.len());
        for f in &vf {
            println!(
                "   fmt {} signed {} norm {} exp {} stride {} const {} off {}",
                f.format, f.signed, f.normalized, f.exp_adjust, f.stride, f.const_index, f.offset
            );
        }
    }
}
