//! Probe: where do given big-endian f32 values occur in an FM2 model's .gpu / .data / .stream?  `fm2_findf <bin.zip> <name-substr> <f32>...`
use fh1_formats::zip::Archive;
fn be(d: &[u8], o: usize) -> Option<u32> { d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) }
fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let vals: Vec<u32> = a[3..].iter().map(|v| v.parse::<f32>().unwrap().to_bits()).collect();
    for e in ar.entries.clone() {
        if !e.name.to_ascii_lowercase().contains(&a[2].to_ascii_lowercase()) || !e.name.ends_with(".rmb.bin") { continue; }
        let d = ar.read(&e).unwrap();
        let (g, dt, st) = (section(&d, b".gpu").unwrap_or(0), section(&d, b".data").unwrap_or(0), section(&d, b".stream").unwrap_or(0));
        println!("{} gpu {g:#x} data {dt:#x} stream {st:#x}", e.name);
        for (i, v) in vals.iter().enumerate() {
            for (sec, s, len) in [("gpu", 0x190, g), ("data", 0x190 + g, dt), ("stream", 0x190 + g + dt, st)] {
                let hits: Vec<String> = (0..len.saturating_sub(3)).filter(|k| be(&d, s + k) == Some(*v)).map(|k| format!("{k:#x}")).collect();
                if !hits.is_empty() { println!("  {} in {sec}: {}", a[3 + i], hits.join(" ")); }
            }
        }
    }
}
