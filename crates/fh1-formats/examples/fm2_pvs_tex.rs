//! Probe: PVS texture records by index. `fm2_pvs_tex <track.pvs> <index>...`
use fh1_formats::fm2;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pvs = fm2::parse_pvs(&std::fs::read(&a[1]).unwrap()).unwrap();
    for i in a[2..].iter().filter_map(|v| v.parse::<usize>().ok()) {
        let t = pvs.textures[i];
        println!("tex {i}: file {:#x} flags {} object {} tile {:?}", t.file_id, t.flags, t.object, t.tile);
    }
}
