//! Probe: print the VS and PS disassembly of a model's group. `fm2_show <bin.zip> <model> <group>`
use fh1_formats::{fm2, zip::Archive};
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
    let model = fm2::parse_model(&d).unwrap();
    let (gpu, data) = (&d[0x190..0x190 + g], &d[0x190 + g..0x190 + g + dt]);
    let shaders = sh::tail_shaders(gpu, model.tail_start);
    let groups = sh::data_groups(data);
    let pairs = sh::pair_groups(&groups, &shaders);
    let gi: usize = a[3].parse().unwrap();
    let (v, p) = pairs[gi];
    println!("VS literals {:?}\n{}", shaders[v.unwrap()].literals, shaders[v.unwrap()].text);
    println!("PS literals {:?}\n{}", shaders[p.unwrap()].literals, shaders[p.unwrap()].text);
}
