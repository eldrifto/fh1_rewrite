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
//! two u16 at 8 and 10 = uv (offset binary: `value - 0x8000`; scale 1/1310.72, fitted); bytes 16..60 are not decoded
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

pub const UV_SCALE: f32 = 1310.72;

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
        let mut finite = true;
        for v in h.first..=h.last {
            let b = &d[pool.data + v * 60..pool.data + v * 60 + 60];
            let w = half([b[6], b[7]]);
            let pos = [half([b[0], b[1]]) * w, half([b[2], b[3]]) * w, half([b[4], b[5]]) * w];
            finite &= pos.iter().all(|x| x.is_finite());
            for k in 0..3 {
                mn[k] = mn[k].min(pos[k]);
                mx[k] = mx[k].max(pos[k]);
            }
            let uv = |o: usize| (u16::from_be_bytes([b[o], b[o + 1]]) as i32 - 0x8000) as f32 / UV_SCALE;
            vertices.push(Vertex { position: pos, uv: [uv(8), uv(10)] });
        }
        ext.push(if finite { (mn, mx) } else { ([f32::NAN; 3], [f32::NAN; 3]) });

        let end = heads.get(i + 1).map_or(pool_start, |n| n.o);
        let mut materials: Vec<String> = Vec::new();
        let mut current: Option<usize> = None;
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
                    if tris.iter().all(|&t| (t as usize) < vertices.len()) && current.is_some() {
                        runs.push(Run { material: current.unwrap(), lod, indices: tris });
                    }
                    p += 20 + n * 2;
                    continue;
                }
            }
            // A material record: the first of a part is `1, run_count, 1, 3, len, name`, every later one `1, 3, len, name` (VERIFIED on the
            // Cobra's body: chrome, plastic2, body, frame, black, rubber_trim ... one record before each group of runs).
            let rec = if u32_at(d, p) == Some(1) && u32_at(d, p + 8) == Some(1) && u32_at(d, p + 12) == Some(3) {
                Some(p + 16)
            } else if u32_at(d, p) == Some(1) && u32_at(d, p + 4) == Some(3) {
                Some(p + 8)
            } else {
                None
            };
            if let Some(q) = rec {
                let l = u32_at(d, q).unwrap_or(0) as usize;
                if (1..=40).contains(&l) && q + 4 + l <= end && d[q + 4..q + 4 + l].iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
                    let name = String::from_utf8_lossy(&d[q + 4..q + 4 + l]).into_owned();
                    let at = materials.iter().position(|m| *m == name).unwrap_or_else(|| {
                        materials.push(name);
                        materials.len() - 1
                    });
                    current = Some(at);
                    p = q + 4 + l;
                    continue;
                }
            }
            p += 1;
        }
        parts.push(Part { name: h.name.clone(), centre: None, vertices, materials, runs });
    }

    // Placeholder sections (FLT_MAX sentinel bounds: `winga` on cars without a wing) decode to NaN: leave them empty.
    for (i, (mn, mx)) in ext.iter().enumerate() {
        if !(0..3).all(|k| mn[k].is_finite() && mx[k].is_finite()) {
            parts[i].vertices.clear();
            parts[i].runs.clear();
        }
    }
    let symmetric = |b: &[f32; 9], tol: f32| (0..3).all(|k| (b[3 + k] + b[6 + k]).abs() < tol);
    // Centres: the bounds record 0x55 bytes before each name's length word.
    let mut claimed = Vec::new();
    let mut cerr = vec![0.0f32; parts.len()];
    for (i, h) in heads.iter().enumerate() {
        if parts[i].vertices.is_empty() {
            continue;
        }
        let (mn, mx) = ext[i];
        if let Some((p, b)) = h.o.checked_sub(0x55).and_then(|p| bounds_at(d, p).map(|b| (p, b))) {
            if ext_matches(&b, mn, mx, 0.08) && symmetric(&b, 0.05) {
                parts[i].centre = Some([b[0], b[1], b[2]]);
                claimed.push(p);
            }
        }
    }
    // The rest (glass, lamp covers) keep theirs in a table elsewhere in the header area: take an unclaimed, symmetric record whose size matches
    // and whose side agrees with the name (L = -x, R = +x; centre parts sit at x = 0).
    for (i, h) in heads.iter().enumerate() {
        if parts[i].centre.is_some() || parts[i].vertices.is_empty() {
            continue;
        }
        let (mn, mx) = ext[i];
        let suffix: String = h.name.chars().skip_while(|c| c.is_ascii_lowercase()).collect();
        let side = match suffix.chars().next() {
            Some('L') => -1.0f32,
            Some('R') if suffix.len() > 1 || !h.name.starts_with("glass") => 1.0,
            _ => 0.0,
        };
        let on_side = |b: &[f32; 9]| (side < 0.0 && b[0] < -0.05) || (side > 0.0 && b[0] > 0.05) || (side == 0.0 && b[0].abs() < 0.05);
        // Tightest size tolerance that yields a record on the right side (glass records are a few cm off the decoded extent).
        let mut pick: Option<(usize, [f32; 9])> = None;
        for tol in [0.015f32, 0.04, 0.08] {
            let mut cands: Vec<(usize, [f32; 9])> = Vec::new();
            let mut p = 0x40;
            while p + 36 <= pool_start {
                if let Some(b) = bounds_at(d, p) {
                    if symmetric(&b, 0.02) && ext_matches(&b, mn, mx, tol) && !claimed.contains(&p) && b[1] > -0.2 && b[1] < 1.9 && b[2].abs() < 3.5 && b[0].abs() < 1.5 {
                        cands.push((p, b));
                    }
                }
                p += 1;
            }
            let err = |b: &[f32; 9]| (0..3).map(|k| ((b[6 + k] - b[3 + k]) - (mx[k] - mn[k])).abs()).fold(0.0f32, f32::max);
            cands.sort_by(|x, y| err(&x.1).total_cmp(&err(&y.1)));
            if let Some(c) = cands.iter().find(|(_, b)| on_side(b)) {
                pick = Some(*c);
                break;
            }
        }
        // A part named like a side one that sits on the centre line (the Ford GT's `glassRM`, the engine-bay glass): take a centred record.
        if pick.is_none() && side != 0.0 {
            let mut p = 0x40;
            let mut best: Option<(f32, usize, [f32; 9])> = None;
            while p + 36 <= pool_start {
                if let Some(b) = bounds_at(d, p) {
                    if symmetric(&b, 0.02) && ext_matches(&b, mn, mx, 0.08) && !claimed.contains(&p) && b[0].abs() < 0.05 && b[1] > -0.2 && b[1] < 1.9 && b[2].abs() < 3.5 {
                        let e = (0..3).map(|k| ((b[6 + k] - b[3 + k]) - (mx[k] - mn[k])).abs()).fold(0.0f32, f32::max);
                        if best.is_none_or(|x| e < x.0) {
                            best = Some((e, p, b));
                        }
                    }
                }
                p += 1;
            }
            pick = best.map(|(_, p, b)| (p, b));
        }
        let pick = pick.as_ref();
        if let Some((p, b)) = pick {
            parts[i].centre = Some([b[0], b[1], b[2]]);
            cerr[i] = (0..3).map(|k| ((b[6 + k] - b[3 + k]) - (mx[k] - mn[k])).abs()).fold(0.0f32, f32::max);
            claimed.push(*p);
        } else if side != 0.0 {
            // A mirrored pair that shares one record (the twin took it): the same record on this side.
            let mut p = 0x40;
            while p + 36 <= pool_start {
                if let Some(b) = bounds_at(d, p) {
                    if symmetric(&b, 0.02) && ext_matches(&b, mn, mx, 0.08) && b[1] > -0.2 && b[1] < 1.9 && b[2].abs() < 3.5 && b[0].abs() > 0.05 && b[0].abs() < 1.5 {
                        parts[i].centre = Some([side * b[0].abs(), b[1], b[2]]);
                        break;
                    }
                }
                p += 1;
            }
        }
    }
    // A left / right pair whose centres are not mirror images (two records of equal size matched the wrong sides): keep the better-fitting one and
    // mirror it onto the other.
    let twin_of = |name: &str| -> Option<String> {
        let suffix: String = name.chars().skip_while(|c| c.is_ascii_lowercase()).collect();
        let swapped = match suffix.chars().next()? {
            'L' => 'R',
            'R' => 'L',
            _ => return None,
        };
        Some(format!("{}{}{}", &name[..name.len() - suffix.len()], swapped, &suffix[1..]))
    };
    for i in 0..parts.len() {
        let Some(tw) = twin_of(&parts[i].name) else { continue };
        let Some(j) = parts.iter().position(|p| p.name == tw) else { continue };
        if !parts[i].name[parts[i].name.len() - 1..].is_empty() && parts[i].name.chars().skip_while(|c| c.is_ascii_lowercase()).next() != Some('L') {
            continue; // handle each pair once, from the left part
        }
        let (Some(a), Some(b)) = (parts[i].centre, parts[j].centre) else { continue };
        let bad = (a[0] + b[0]).abs() > 0.06 || (a[1] - b[1]).abs() > 0.06 || (a[2] - b[2]).abs() > 0.1;
        // Only glass-like parts (their record is found by size): mirrors, seats and exhausts are legitimately asymmetric.
        let glassy = parts[i].name.starts_with("glass") || parts[i].name.starts_with("lexan");
        if bad && glassy {
            let (keep, other, c) = if cerr[i] <= cerr[j] { (i, j, a) } else { (j, i, b) };
            let _ = keep;
            parts[other].centre = Some([-c[0], c[1], c[2]]);
        }
    }
    // Still missing: mirror the left / right twin's centre.
    for i in 0..parts.len() {
        if parts[i].centre.is_some() || parts[i].vertices.is_empty() {
            continue;
        }
        let name = parts[i].name.clone();
        let suffix: String = name.chars().skip_while(|c| c.is_ascii_lowercase()).collect();
        let swapped = match suffix.chars().next() {
            Some('L') => 'R',
            Some('R') => 'L',
            _ => continue,
        };
        let twin = format!("{}{}{}", &name[..name.len() - suffix.len()], swapped, &suffix[1..]);
        if let Some(c) = parts.iter().find(|p| p.name == twin).and_then(|p| p.centre) {
            parts[i].centre = Some([-c[0], c[1], c[2]]);
        }
    }
    Ok(Fm2Carbin { parts })
}
