//! Scratch probe: disassemble the FM2 shader microcode that lives in the `.gpu` tail (the `.data` containers are stubs;
//! each container names its `.updb` GUID, and the `.gpu` blob ending in `4e4a0000` + that GUID holds the code).
//! `cargo run -p fh1-shaders --example fm2_gpu_shaders -- <bin.zip> <model number> [name substring]`.
use fh1_formats::zip::Archive;
use fh1_shaders::container::ShaderBlob;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (zip, number) = (&a[1], a[2].parse::<usize>().unwrap());
    let filter = a.get(3).cloned();
    let mut ar = Archive::open(zip).unwrap();
    let suffix = format!(".{number:0>5}.rmb.bin");
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    // Every `4e4a0000`+GUID terminator in the file.
    let mut terms: Vec<(usize, [u8; 8])> = Vec::new();
    let mut i = 0;
    while i + 12 <= d.len() {
        if &d[i..i + 4] == b"\x4e\x4a\x00\x00" {
            terms.push((i, d[i + 4..i + 12].try_into().unwrap()));
        }
        i += 4;
    }
    let mut o = 0;
    while o + 8 <= d.len() {
        let m = u32::from_be_bytes(d[o..o + 4].try_into().unwrap());
        if m == 0x102A_1100 || m == 0x102A_1101 {
            match ShaderBlob::parse(&d[o..], o) {
                Ok(mut b) => {
                    let hit = filter.as_ref().is_none_or(|f| b.constants.iter().any(|c| c.name.contains(f.as_str())));
                    if hit {
                        // The container's .updb path holds the microcode GUID (8 bytes, 18 hex/dash chars).
                        let region = &d[o..(o + 0x600).min(d.len())];
                        let guid = region.windows(5).position(|w| w == b".updb").and_then(|p| {
                            let g = std::str::from_utf8(region.get(p - 18..p)?).ok()?;
                            let hex: String = g.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                            (hex.len() == 16).then_some(hex)
                        });
                        let g = guid.as_ref().map(|h| hex16(h));
                        let term = g.and_then(|g| terms.iter().find(|(_, t)| *t == g));
                        if let Some((tp, _)) = term {
                            // The blob runs from the previous terminator (or the .gpu start) to `tp`.
                            let prev = terms.iter().filter(|(p, _)| *p < *tp).map(|(p, _)| *p).max().unwrap_or(0x190);
                            let (s, end) = (prev + 16, *tp);
                            b.microcode = (s..end).step_by(4).map(|k| u32::from_be_bytes(d[k..k + 4].try_into().unwrap())).collect();
                            println!("[gpu microcode {s:#x}..{end:#x} = {} dwords]", b.microcode.len());
                        }
                        println!("== {} @{o:#x} target {} guid {:?}", if m == 0x102A_1101 { "VS" } else { "PS" }, b.target, guid);
                        for c in &b.constants {
                            println!("   {:?} reg {} {:?} {:?}", c.set, c.register, c.class, c.name);
                        }
                        if b.interpolators.len() > 0 || !b.vertex_elements.is_empty() {
                            println!("   elements {:?} interp {:?}", b.vertex_elements, b.interpolators);
                        }
                        println!("{}", fh1_shaders::disasm::disassemble(&b));
                    }
                    o += ShaderBlob::container_len(&d[o..]).unwrap_or(4).max(4);
                    continue;
                }
                Err(err) => println!("  @{o:#x}: {err}"),
            }
        }
        o += 4;
    }
}

fn hex16(h: &str) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    out
}
