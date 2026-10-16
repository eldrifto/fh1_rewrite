//! Probe: dump every `.stream` record of FM2 models (type, length, payload words) so the per-draw state of a known x1 draw and a known x32 draw can be diffed.
//! `cargo run -p fh1-formats --example fm2_stream_dump -- <bin.zip> <model-name substring> [...]`   (no substring: a census of record types over all models)
use fh1_formats::zip::Archive;
use std::collections::BTreeMap;

fn be(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}

fn stream_of(d: &[u8]) -> Option<(&[u8], &[u8])> {
    let (g, dt, st) = (section(d, b".gpu")?, section(d, b".data")?, section(d, b".stream")?);
    let s0 = 0x190 + g + dt;
    Some((d.get(0x190 + g..s0)?, d.get(s0..s0 + st)?))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    let pats: Vec<String> = a[2..].iter().map(|p| p.to_ascii_lowercase()).collect();
    let mut census: BTreeMap<u8, (usize, BTreeMap<usize, usize>)> = BTreeMap::new();
    let mut payloads: BTreeMap<u8, BTreeMap<String, usize>> = BTreeMap::new();
    for e in ar.entries.clone() {
        let lname = e.name.to_ascii_lowercase();
        if !lname.ends_with(".rmb.bin") {
            continue;
        }
        let show = pats.iter().any(|p| lname.contains(p));
        if !pats.is_empty() && !show {
            continue;
        }
        let bytes = ar.read(&e).unwrap();
        let Some((_, stream)) = stream_of(&bytes) else { continue };
        let mut o = be(stream, 0).unwrap_or(0x24) as usize;
        if show {
            println!("=== {} (stream {} bytes, start {o:#x})", e.name, stream.len());
        }
        while o + 4 <= stream.len() {
            let (len, ty) = (stream[o + 1] as usize, stream[o + 2]);
            if len < 4 || o + len > stream.len() {
                break;
            }
            let c = census.entry(ty).or_default();
            c.0 += 1;
            *c.1.entry(len).or_default() += 1;
            if pats.is_empty() && len <= 44 {
                let words: Vec<String> = (0..(len - 4) / 4).map(|k| format!("{:x}", be(stream, o + 4 + 4 * k).unwrap_or(0))).collect();
                *payloads.entry(ty).or_default().entry(format!("[{:#04x},{:#04x}] {}", stream[o], stream[o + 3], words.join(" "))).or_default() += 1;
            }
            if show {
                let words: Vec<String> = (0..(len - 4) / 4).map(|k| format!("{:x}", be(stream, o + 4 + 4 * k).unwrap_or(0))).collect();
                println!("{o:#06x} ty {ty:#04x} len {len:3} [b0={:#04x} b3={:#04x}] {}", stream[o], stream[o + 3], words.join(" "));
            }
            o += len;
        }
    }
    if pats.is_empty() {
        for (ty, (n, lens)) in census {
            println!("ty {ty:#04x}: {n} records, lengths {lens:?}");
            if let Some(p) = payloads.get(&ty) {
                let mut v: Vec<_> = p.iter().collect();
                v.sort_by(|a, b| b.1.cmp(a.1));
                println!("    {} distinct payloads; top: {:?}", v.len(), &v[..v.len().min(6)]);
            }
        }
    }
}
