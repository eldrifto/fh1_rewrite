//! Probe: frames, bone count and degenerate rotations of spectator `.anim.bin` clips (usage: anim_probe <file>...).
use fh1_formats::crowd::anim;

fn main() {
    for p in std::env::args().skip(1) {
        let (skel, c) = anim::parse(&std::fs::read(&p).unwrap()).unwrap();
        let mut zero = 0;
        let mut nan = 0;
        for f in &c.rotations {
            for q in f {
                let l = q.iter().map(|x| x * x).sum::<f32>();
                zero += (l < 1e-6) as usize;
                nan += q.iter().any(|x| !x.is_finite()) as usize;
            }
        }
        let tn = c.translations.iter().flatten().filter(|t| t.iter().any(|x| !x.is_finite())).count();
        println!("{p}: bones {}, frames {} ({} rotation frames), duration {}, zero quats {zero}, non-finite quats {nan}, non-finite translations {tn}", skel.bones.len(), c.frames, c.rotations.len(), c.duration);
    }
}
