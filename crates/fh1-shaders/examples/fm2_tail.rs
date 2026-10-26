//! Scratch probe: disassemble a byte range of an FM2 model's `.gpu` as Xenos microcode (the real shaders live in the
//! `.gpu` tail; on some tracks, e.g. Nurburgring, there is no `4e4a0000`+GUID terminator to delimit them).
//! `cargo run -p fh1-shaders --example fm2_tail -- <bin.zip> <model> <start_hex> <end_hex> [vs|ps] [align]`.
use fh1_formats::zip::Archive;
use fh1_shaders::container::{ShaderBlob, Stage};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (zip, number) = (&a[1], a[2].parse::<usize>().unwrap());
    let start = usize::from_str_radix(a[3].trim_start_matches("0x"), 16).unwrap();
    let end = usize::from_str_radix(a[4].trim_start_matches("0x"), 16).unwrap();
    let vs = a.get(5).map(|s| s == "vs").unwrap_or(false);
    let align = a.get(6).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let mut ar = Archive::open(zip).unwrap();
    let suffix = format!(".{number:0>5}.rmb.bin");
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let s = (start + align).min(d.len());
    let words: Vec<u32> = (s..end.min(d.len())).step_by(4).map(|k| u32::from_be_bytes(d[k..k + 4].try_into().unwrap())).collect();
    let blob = ShaderBlob {
        stage: if vs { Stage::Vertex } else { Stage::Pixel },
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
    println!("// {} .. {}, {} dwords", s, end.min(d.len()), blob.microcode.len());
    println!("{}", fh1_shaders::disasm::disassemble(&blob));
}
