//! Scratch probe: per-channel stats of a DX10 DDS (BC1/BC3/BC4/BC5), to see how much a lightmap varies.
//! `cargo run -p fh1-formats --example dds_stats -- file.dds ...`
fn main() {
    for p in std::env::args().skip(1) {
        let d = std::fs::read(&p).unwrap();
        if d.get(0..4) != Some(b"DDS ") {
            println!("{p}: not DDS");
            continue;
        }
        let h = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let (w, ht) = (h(16), h(12));
        let dxgi = h(128);
        let data = &d[148..];
        let mut px = vec![0u32; (w * ht) as usize];
        let ok = match dxgi {
            71 => texture2ddecoder::decode_bc1(data, w as usize, ht as usize, &mut px).is_ok(),
            77 => texture2ddecoder::decode_bc3(data, w as usize, ht as usize, &mut px).is_ok(),
            80 => texture2ddecoder::decode_bc4(data, w as usize, ht as usize, &mut px).is_ok(),
            83 => texture2ddecoder::decode_bc5(data, w as usize, ht as usize, &mut px).is_ok(),
            _ => {
                println!("{p}: dxgi {dxgi}");
                false
            }
        };
        if !ok {
            println!("{p}: decode failed");
            continue;
        }
        let n = px.len() as f64;
        let mut sum = [0f64; 4];
        let (mut lo, mut hi) = ([255f64; 3], [0f64; 3]);
        for &p in &px {
            let [b, g, r, a] = p.to_le_bytes();
            let v = [r as f64, g as f64, b as f64, a as f64];
            for c in 0..4 {
                sum[c] += v[c];
            }
            for c in 0..3 {
                lo[c] = lo[c].min(v[c]);
                hi[c] = hi[c].max(v[c]);
            }
        }
        println!(
            "{p}: {w}x{ht} dxgi {dxgi} mean R {:.0} G {:.0} B {:.0} A {:.0}, range R {:.0}..{:.0}",
            sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n, lo[0], hi[0]
        );
    }
}
