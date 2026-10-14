//! Forza Motorsport 2 `.carbin` car models (type word 2, big-endian), an older layout than `carbin`.
//!
//! File: header + node tables, then one section per part (`bumperFa`, `glassLF`, ...), then two vertex pools
//! (`2, 1, count, 60`, `0x000DECAF`, `count * 60` bytes, 4-byte trailer). Each section header is
//! `[9-float bounds record][... u16 first, u16 last, u8 pool_flag, u32 1][u32 name_len + name]`, followed by
//! `2, 1, 0, 0x3c`, the materials (`1, N, 1, 3, len + name`) and the index runs
//! (`6, lod_tag, 3, count, 2` + `count` big-endian u16 triangle strips, `0xFFFF` restarts, indices local to the section).
//! `pool_flag` 1 selects pool A, 0 pool B; `first..=last` is the section's vertex range.
//!
//! Vertex (60 bytes): four big-endian half floats `x y z w` at 0 (`(x, y, z)` is a unit direction, position = xyz * w, part-local);
//! two u16 at 8 and 10 = uv (offset binary: `value - 0x8000`; scale about 1/1280, not settled); bytes 16..60 are not decoded
//! (the rest looks like lighting coefficients). The part's world centre is the first three floats of its bounds record
//! (`centre, local min, local max`), 0x55 bytes before the name's length word. Car frame: -z front, +z rear, y up, left = -x.
//! Normals are not decoded; compute them from the triangles.

use crate::Error;

#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    /// Part-local, metres.
    pub position: [f32; 3],
    /// `(u16 - 0x8000) / UV_SCALE`.
    pub uv: [f32; 2],
}

pub const UV_SCALE: f32 = 1280.0;

#[derive(Debug, Clone)]
pub struct Run {
    /// Index into `Part::materials`.
    pub material: usize,
    /// 1 = most detailed .. 5.
    pub lod: u32,
    /// Triangle list, local to the part's `vertices`.
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    /// World position of the part's origin (car frame, metres); `None` when no bounds record matched.
    pub centre: Option<[f32; 3]>,
    pub vertices: Vec<Vertex>,
    pub materials: Vec<String>,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone)]
pub struct Fm2Carbin {
    pub parts: Vec<Part>,
}

fn u32_at(d: &[u8], p: usize) -> Option<u32> {
    d.get(p..p + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn f32_at(d: &[u8], p: usize) -> Option<f32> {
    u32_at(d, p).map(f32::from_bits)
}

fn half(b: [u8; 2]) -> f32 {
    let h = u16::from_be_bytes(b);
    let (s, e, m) = (((h >> 15) & 1) as i32, ((h >> 10) & 31) as i32, (h & 1023) as f32);
    let v = match e {
        0 => m * 2f32.powi(-24),
        31 => if m == 0.0 { f32::INFINITY } else { f32::NAN },
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    };
    if s == 1 { -v } else { v }
}

struct Pool {
    count: usize,
    data: usize,
}

fn find_pools(d: &[u8]) -> Vec<Pool> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 20 <= d.len() {
        if u32_at(d, p) == Some(2) && u32_at(d, p + 4) == Some(1) && u32_at(d, p + 12) == Some(60) && u32_at(d, p + 16) == Some(0x000D_ECAF) {
            let count = u32_at(d, p + 8).unwrap() as usize;
            if count > 0 && p + 20 + count * 60 <= d.len() {
                out.push(Pool { count, data: p + 20 });
                p += 20 + count * 60;
                continue;
            }
        }
        p += 1;
    }
    out
}

struct Header {
    o: usize,
    first: usize,
    last: usize,
    flag: u8,
    name: String,
}

fn find_sections(d: &[u8], end: usize) -> Vec<Header> {
    let mut out = Vec::new();
    let mut p = 16;
    while p + 8 < end {
        let l = u32_at(d, p).unwrap_or(0) as usize;
        if (3..=48).contains(&l) && p + 4 + l + 8 <= end {
            let name = &d[p + 4..p + 4 + l];
            if name[0].is_ascii_alphabetic()
                && name.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b' ')
                && u32_at(d, p + 4 + l) == Some(2)
                && u32_at(d, p + 8 + l) == Some(1)
                && u32_at(d, p - 4) == Some(1)
            {
                let first = u16::from_be_bytes([d[p - 9], d[p - 8]]) as usize;
                let last = u16::from_be_bytes([d[p - 7], d[p - 6]]) as usize;
                if last >= first {
                    out.push(Header { o: p, first, last, flag: d[p - 5], name: String::from_utf8_lossy(name).into_owned() });
                    p += 4 + l + 8;
                    continue;
                }
            }
        }
        p += 1;
    }
    out
}

/// `[centre xyz, local min xyz, local max xyz]` at `p`, if it is a plausible bounds record.
fn bounds_at(d: &[u8], p: usize) -> Option<[f32; 9]> {
    let mut v = [0.0; 9];
    for (k, x) in v.iter_mut().enumerate() {
        *x = f32_at(d, p + 4 * k)?;
        if !x.is_finite() || x.abs() > 20.0 {
            return None;
        }
    }
    (0..3).all(|k| v[6 + k] >= v[3 + k]).then_some(v)
}

fn ext_matches(b: &[f32; 9], mn: [f32; 3], mx: [f32; 3], tol: f32) -> bool {
    (0..3).all(|k| ((b[6 + k] - b[3 + k]) - (mx[k] - mn[k])).abs() < tol)
}

pub fn parse(d: &[u8]) -> Result<Fm2Carbin, Error> {
    if !matches!(u32_at(d, 0), Some(1 | 2)) {
        return Err(Error::BadMagic("fm2 carbin"));
    }
    let pools = find_pools(d);
    if pools.is_empty() {
        return Err(Error::Unsupported("fm2 carbin: vertex pools not found".into()));
    }
    let pool_start = pools[0].data - 20;
    let heads = find_sections(d, pool_start);
    if heads.is_empty() {
        return Err(Error::Unsupported("fm2 carbin: no sections".into()));
    }
    let mut parts = Vec::new();
    let mut ext: Vec<([f32; 3], [f32; 3])> = Vec::new();
    for (i, h) in heads.iter().enumerate() {
        let pool = pools.get(if h.flag == 1 { 0 } else { 1 }).ok_or_else(|| Error::Unsupported(format!("fm2 carbin: {} uses a missing pool", h.name)))?;
        if h.last >= pool.count {
            return Err(Error::Unsupported(format!("fm2 carbin: {} vertex range outside pool", h.name)));
        }
        let mut vertices = Vec::with_capacity(h.last - h.first + 1);
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in h.first..=h.last {
            let b = &d[pool.data + v * 60..pool.data + v * 60 + 60];
            let w = half([b[6], b[7]]);
            let pos = [half([b[0], b[1]]) * w, half([b[2], b[3]]) * w, half([b[4], b[5]]) * w];
            for k in 0..3 {
                mn[k] = mn[k].min(pos[k]);
                mx[k] = mx[k].max(pos[k]);
            }
            let uv = |o: usize| (u16::from_be_bytes([b[o], b[o + 1]]) as i32 - 0x8000) as f32 / UV_SCALE;
            vertices.push(Vertex { position: pos, uv: [uv(8), uv(10)] });
        }
        ext.push((mn, mx));

        let end = heads.get(i + 1).map_or(pool_start, |n| n.o);
        let mut materials = Vec::new();
        let mut runs = Vec::new();
        let mut p = h.o + 4 + h.name.len() + 16;
        while p + 24 <= end {
            if u32_at(d, p) == Some(6) && u32_at(d, p + 8) == Some(3) && u32_at(d, p + 16) == Some(2) {
                let (lod, n) = (u32_at(d, p + 4).unwrap(), u32_at(d, p + 12).unwrap() as usize);
                if (1..=5).contains(&lod) && p + 20 + n * 2 <= end {
                    let mut tris = Vec::new();
                    let mut run: Vec<u32> = Vec::new();
                    let flush = |run: &mut Vec<u32>, tris: &mut Vec<u32>| {
                        for j in 0..run.len().saturating_sub(2) {
                            let (a, b, c) = (run[j], run[j + 1], run[j + 2]);
                            if a != b && b != c && a != c {
                                if j % 2 == 0 { tris.extend([a, b, c]) } else { tris.extend([b, a, c]) }
                            }
                        }
                        run.clear();
                    };
                    for k in 0..n {
                        let v = u16::from_be_bytes([d[p + 20 + 2 * k], d[p + 21 + 2 * k]]);
                        if v == 0xFFFF {
                            flush(&mut run, &mut tris);
                        } else {
                            run.push(v as u32);
                        }
                    }
                    flush(&mut run, &mut tris);
                    if tris.iter().all(|&t| (t as usize) < vertices.len()) && !materials.is_empty() {
                        runs.push(Run { material: materials.len() - 1, lod, indices: tris });
                    }
                    p += 20 + n * 2;
                    continue;
                }
            }
            if u32_at(d, p) == Some(1) && u32_at(d, p + 8) == Some(1) && u32_at(d, p + 12) == Some(3) {
                let l = u32_at(d, p + 16).unwrap() as usize;
                if (1..=40).contains(&l) && p + 20 + l <= end && d[p + 20..p + 20 + l].iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
                    materials.push(String::from_utf8_lossy(&d[p + 20..p + 20 + l]).into_owned());
                    p += 20 + l;
                    continue;
                }
            }
            p += 1;
        }
        parts.push(Part { name: h.name.clone(), centre: None, vertices, materials, runs });
    }

    // Centres: the bounds record 0x55 bytes before each name's length word.
    let mut claimed = Vec::new();
    for (i, h) in heads.iter().enumerate() {
        let (mn, mx) = ext[i];
        if let Some(b) = h.o.checked_sub(0x55).and_then(|p| bounds_at(d, p).map(|b| (p, b))) {
            if ext_matches(&b.1, mn, mx, 0.02) {
                parts[i].centre = Some([b.1[0], b.1[1], b.1[2]]);
                claimed.push(b.0);
            }
        }
    }
    // The remaining parts (glass) keep theirs in the header tables: take matching, still unclaimed records in file order.
    let mut cursor = 0;
    for (i, h) in heads.iter().enumerate() {
        if parts[i].centre.is_some() {
            continue;
        }
        let (mn, mx) = ext[i];
        let _ = h;
        let mut p = cursor;
        while p + 36 <= pool_start {
            if let Some(b) = bounds_at(d, p) {
                let sym = (0..3).all(|k| (b[3 + k] + b[6 + k]).abs() < 0.02);
                if sym && ext_matches(&b, mn, mx, 0.01) && !claimed.contains(&p) && b[1] > -0.2 && b[1] < 1.6 && b[2].abs() < 2.6 && b[0].abs() < 1.3 {
                    parts[i].centre = Some([b[0], b[1], b[2]]);
                    claimed.push(p);
                    cursor = p + 36;
                    break;
                }
            }
            p += 1;
        }
    }
    Ok(Fm2Carbin { parts })
}
