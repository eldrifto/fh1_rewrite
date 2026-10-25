//! Probe: hex-dump a range of an FM2 model's `.data` (or `.gpu`) section as big-endian words. `fm2_data_dump <bin.zip> <model> <data|gpu> <offset-hex> <words>`
use fh1_formats::zip::Archive;
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
    let suffix = format!(".{:0>5}.rmb.bin", a[2].parse::<usize>().unwrap());
    let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
    let d = ar.read(&e).unwrap();
    let (g, dt) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap());
    let (start, len) = if a[3] == "gpu" { (0x190, g) } else { (0x190 + g, dt) };
    let sec = &d[start..start + len];
    let off = usize::from_str_radix(&a[4], 16).unwrap();
    let n: usize = a[5].parse().unwrap();
    println!("{} section {len:#x} bytes", a[3]);
    for r in 0..n.div_ceil(8) {
        let o = off + r * 32;
        let w: Vec<String> = (0..8).filter(|k| r * 8 + k < n).map(|k| be(sec, o + 4 * k).map_or("--------".into(), |v| format!("{v:08x}"))).collect();
        println!("{o:05x}: {}", w.join(" "));
    }
}
