//! Probe: print the constant table (name, register, default) of each pixel-shader container in an FM2 model's `.data`.
//! `fm2_ps_consts <bin.zip> <model> [group]`
use fh1_formats::zip::Archive;
use fh1_shaders::container::ShaderBlob;
use fh1_shaders::fm2 as sh;
fn be(d: &[u8], o: usize) -> Option<u32> { d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) }
fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let suffix = format!(".{:0>5}.rmb.bin", a[2].parse::<usize>().unwrap());
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
    let data = &d[0x190 + g..0x190 + g + dt];
    for (gi, grp) in sh::data_groups(data).iter().enumerate() {
        if a.get(3).is_some_and(|x| x.parse::<usize>().ok() != Some(gi)) { continue; }
        let mut copy = data[grp.ps..].to_vec();
        copy[3] = 0;
        let Ok(b) = ShaderBlob::parse(&copy, grp.ps) else { println!("group {gi}: parse failed"); continue };
        println!("group {gi} ps @{:#x} ({} dwords)", grp.ps, b.microcode.len());
        for c in &b.constants {
            println!("  {:?} c{} x{} {} ty{} default {:?}", c.set, c.register, c.count, c.name, c.ty, c.default.as_ref().map(|w| w.iter().map(|v| f32::from_bits(*v)).collect::<Vec<_>>()));
        }
        println!("  float defs {:?}", b.float_defs);
    }
}
