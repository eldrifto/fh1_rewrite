//! Probe: the render-state stream records seen before each draw of an FM2 model, with its up-facing / down-facing triangle counts.
//! `cargo run -p fh1-formats --example fm2_state -- <bin.zip> <model>...`
use fh1_formats::{fm2, zip::Archive};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut ar = Archive::open(&a[1]).unwrap();
    for n in &a[2..] {
        let suffix = format!(".{:0>5}.rmb.bin", n.parse::<usize>().unwrap());
        let e = ar.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(&suffix)).unwrap().clone();
        let m = fm2::parse_model(&ar.read(&e).unwrap()).unwrap();
        for (i, d) in m.draws.iter().enumerate() {
            if std::env::var_os("CENSUS").is_some() {
                let (mut up, mut down) = (0, 0);
                for t in d.indices.chunks_exact(3) {
                    let [p, q, r] = [t[0], t[1], t[2]].map(|i| d.positions[i as usize]);
                    let (u, v) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
                    let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-9);
                    if c[1].abs() >= 0.7 * l { if c[1] > 0.0 { up += 1 } else { down += 1 } }
                }
                if up + down >= 30 && (up * 5 < down || down * 5 < up) {
                    let key: Vec<String> = d.state.iter().map(|(t, w)| format!("{t:x}:{:x}", w.get(1).copied().unwrap_or(0))).collect();
                    println!("{} {}", if up > down { "UP" } else { "DOWN" }, key.join(","));
                }
                continue;
            }
            let (mut up, mut down, mut side) = (0, 0, 0);
            for t in d.indices.chunks_exact(3) {
                let [p, q, r] = [t[0], t[1], t[2]].map(|i| d.positions[i as usize]);
                let (u, v) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
                let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-9);
                if c[1].abs() < 0.7 * l { side += 1 } else if c[1] > 0.0 { up += 1 } else { down += 1 }
            }
            println!("model {n} draw {i}: tris {} up {up} down {down} side {side} state {:x?}", d.indices.len() / 3, d.state);
        }
    }
}
