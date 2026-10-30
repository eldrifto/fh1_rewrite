//! Probe: bounding box, bone-index range and LOD sizes of a spectator `.skinbin` (usage: skin_probe <file>...).
use fh1_formats::crowd::parse_skinbin;

fn main() {
    for p in std::env::args().skip(1) {
        let m = parse_skinbin(&std::fs::read(&p).unwrap()).unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let (mut bmax, mut wsum0) = (0u8, 0usize);
        for v in &m.vertices {
            for a in 0..3 {
                lo[a] = lo[a].min(v.position[a]);
                hi[a] = hi[a].max(v.position[a]);
            }
            bmax = bmax.max(v.bones[0]).max(v.bones[1]);
            wsum0 += (v.weights[0] as usize + v.weights[1] as usize != 255) as usize;
        }
        println!("{p}: {} vertices, lods {:?}, bbox {:?} .. {:?}, max bone {bmax}, weights not summing to 255: {wsum0}", m.vertices.len(), m.lods, lo, hi);
    }
}
