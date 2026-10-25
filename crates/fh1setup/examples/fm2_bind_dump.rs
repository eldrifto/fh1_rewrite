//! Probe: print every `.stream` record of an FM2 model (type + words), marking draws (0x01) so the 0x2d texture-binding records
//! (items + the second word of each pair) can be compared between draws. `fm2_bind_dump <bin.zip> <model> [type-hex ...]`
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
    let (g, dt, s) = (section(&d, b".gpu").unwrap(), section(&d, b".data").unwrap(), section(&d, b".stream").unwrap());
    let s0 = 0x190 + g + dt;
    let stream = &d[s0..s0 + s];
    let only: Vec<u8> = a[3..].iter().map(|x| u8::from_str_radix(x, 16).unwrap()).collect();
    let mut o = be(stream, 0).unwrap() as usize;
    let mut draw = 0;
    while o + 4 <= stream.len() {
        let (len, ty) = (stream[o + 1] as usize, stream[o + 2]);
        if len < 4 || o + len > stream.len() {
            break;
        }
        if only.is_empty() || only.contains(&ty) {
            let words: Vec<String> = (0..(len - 4) / 4).map(|k| format!("{:x}", be(stream, o + 4 + 4 * k).unwrap())).collect();
            println!("draw~{draw} @{o:x} type {ty:02x} len {len}: {}", words.join(" "));
        }
        if ty == 0x01 {
            draw += 1;
        }
        o += len;
    }
}
