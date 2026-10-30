//! Probe: FM6 `.swatchbin` textures. `swatch_probe <file> <bc1|bc3|bc4|bc5|bc7|rgba> <out.png>` decodes mip 0 as if it were
//! stored linearly (row by row) to see whether that guess gives a picture. Output goes to a git-ignored folder.
use fh1_formats::grub;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let g = grub::parse(&d).unwrap();
    let c = g.chunks.iter().find(|c| c.tag == "TXCB").unwrap();
    let meta = g.meta_of(c).unwrap();
    let pix = g.data_of(c).unwrap();
    let r = &meta[24..];
    let u16at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]) as usize;
    let (w, h, mips) = (u16at(0), u16at(2), r[12]);
    println!("{w}x{h} mips {mips} flags {:02x?} pixels {} bytes", &r[15..19], pix.len());
    // optional 4th arg: tile mode. `m<N>` = micro tiles of NxN blocks, Morton order inside (x first), tiles row by row;
    // `my<N>` = same with y first. The blocks are moved back into row order before decoding.
    let bpb = match a[2].as_str() { "bc1" | "bc4" => 8, "rgba" => 0, _ => 16 };
    let owned;
    let mut pix = pix;
    if let (Some(mode), true) = (a.get(4), bpb > 0) {
        let xpos: Option<Vec<usize>> = mode.strip_prefix("x=").map(|l| l.split(',').map(|v| v.parse().unwrap()).collect());
        let yfirst = mode.starts_with("my");
        let gcn = mode.starts_with('g');
        let t: usize = if xpos.is_some() { 1 } else { mode.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().unwrap() };
        let (bw, bh) = (w / 4, h / 4);
        let tiles_x = bw.div_ceil(t);
        let mut out = vec![0u8; bw * bh * bpb];
        for by in 0..bh {
            for bx in 0..bw {
                let (tx, ty, ix, iy) = (bx / t, by / t, bx % t, by % t);
                let mut m = 0;
                if let Some(xp) = &xpos {
                    // pure bit interleave: block-x bits go to the index positions in `xp`, block-y bits to the rest
                    let total = (bw * bh).trailing_zeros() as usize;
                    let yp: Vec<usize> = (0..total).filter(|p| !xp.contains(p)).collect();
                    for (i, p) in xp.iter().enumerate() { m |= ((bx >> i) & 1) << p; }
                    for (i, p) in yp.iter().enumerate() { m |= ((by >> i) & 1) << p; }
                    let src = m * bpb;
                    if src + bpb <= pix.len() { out[(by * bw + bx) * bpb..][..bpb].copy_from_slice(&pix[src..src + bpb]); }
                    continue;
                }
                if mode.starts_with('l') {
                    // linear inside the micro tile (x fastest)
                    m = iy * t + ix;
                }
                if gcn {
                    // AddrLib 1D thin micro tile: index bits LSB first, by element size
                    let pat: [(char, u32); 6] = if bpb == 8 {
                        [('x', 0), ('y', 0), ('x', 1), ('x', 2), ('y', 1), ('y', 2)]
                    } else {
                        [('x', 0), ('y', 0), ('x', 1), ('y', 1), ('x', 2), ('y', 2)]
                    };
                    for (i, (ax, b)) in pat.iter().enumerate() {
                        m |= (((if *ax == 'x' { ix } else { iy }) >> b) & 1) << i;
                    }
                }
                for bit in 0..if gcn { 0 } else { 8 } {
                    let (xb, yb) = ((ix >> bit) & 1, (iy >> bit) & 1);
                    m |= if yfirst { (yb << (2 * bit)) | (xb << (2 * bit + 1)) } else { (xb << (2 * bit)) | (yb << (2 * bit + 1)) };
                }
                let src = ((ty * tiles_x + tx) * t * t + m) * bpb;
                if src + bpb <= pix.len() {
                    out[(by * bw + bx) * bpb..][..bpb].copy_from_slice(&pix[src..src + bpb]);
                }
            }
        }
        owned = out;
        pix = &owned;
    }
    let mut rgba = vec![0u32; w * h];
    let n = w * h;
    match a[2].as_str() {
        "bc1" => texture2ddecoder::decode_bc1(pix, w, h, &mut rgba).unwrap(),
        "bc3" => texture2ddecoder::decode_bc3(pix, w, h, &mut rgba).unwrap(),
        "bc4" => texture2ddecoder::decode_bc4(pix, w, h, &mut rgba).unwrap(),
        "bc5" => texture2ddecoder::decode_bc5(pix, w, h, &mut rgba).unwrap(),
        "bc7" => texture2ddecoder::decode_bc7(pix, w, h, &mut rgba).unwrap(),
        "rgba" => {
            for i in 0..n {
                let p = &pix[i * 4..i * 4 + 4];
                rgba[i] = u32::from_le_bytes([p[2], p[1], p[0], p[3]]); // decoder output is BGRA in a u32
            }
        }
        _ => panic!("format"),
    }
    // texture2ddecoder packs each pixel as little-endian BGRA
    let mut bytes = Vec::with_capacity(n * 4);
    for p in &rgba {
        let b = p.to_le_bytes();
        bytes.extend_from_slice(&[b[2], b[1], b[0], b[3]]);
    }
    let f = std::fs::File::create(&a[3]).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&bytes).unwrap();
}
