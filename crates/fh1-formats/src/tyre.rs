//! A tyre built from its size, for FM6 cars. INFERRED GEOMETRY: no tyre model exists in the FM6 files
//! (MODLOG 107); the game appears to build the tyre in code from the sizes in its database and texture it
//! with `genericAssetMaterials/tireTread_<model>` and `tireSidewall_<model>`. The shape here is our own
//! (a rounded section), not the game's.
//!
//! Coordinates match the FM6 wheel model: the axle is x, the rim face at x = 0 and the rim's inner edge at
//! x = `width`; the tyre is centred between them. Triangles wind counter-clockwise seen from outside.

/// One part of the tyre: the tread (the running surface) or the two sidewalls.
#[derive(Debug, Clone)]
pub struct TyrePart {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// u: around the tyre (0..1 per turn); v: across the tread, or from bead to shoulder on a sidewall.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct TyreSize {
    /// Rim (bead) radius in metres: half the rim diameter.
    pub rim_radius: f32,
    /// Section width in metres (the "225" of 225/40 R19, in metres).
    pub width: f32,
    /// Sidewall height as a fraction of the width (the "40" of 225/40 R19, divided by 100).
    pub aspect: f32,
}

impl TyreSize {
    /// From the usual notation: width in mm, aspect in percent, rim diameter in inches.
    pub fn from_code(width_mm: f32, aspect_pct: f32, rim_in: f32) -> Self {
        TyreSize { rim_radius: rim_in * 0.0254 / 2.0, width: width_mm / 1000.0, aspect: aspect_pct / 100.0 }
    }

    pub fn outer_radius(&self) -> f32 {
        self.rim_radius + self.width * self.aspect
    }
}

/// The tyre's cross-section, from the face-side bead round to the inner bead, as (axle offset from the tyre's
/// centre plane, radius) with a flag for tread points. Our own shape: straight-ish sidewalls bulging slightly
/// past the section width at mid height, rounded shoulders, a flat tread 80 % of the width.
fn section(s: &TyreSize) -> Vec<(f32, f32, bool)> {
    let (r0, r1, hw) = (s.rim_radius, s.outer_radius(), s.width / 2.0);
    let h = r1 - r0;
    let mut pts = Vec::new();
    // sidewall: bead (inside the rim flange) -> bulge -> shoulder
    let side = |t: f32| -> (f32, f32) {
        let r = r0 - 0.004 + (h * 0.86 + 0.004) * t;
        let x = hw * (0.90 + 0.12 * (std::f32::consts::PI * t).sin() - 0.02 * t);
        (x, r)
    };
    let n_side = 8;
    let n_shoulder = 6;
    let n_tread = 8;
    for i in 0..=n_side {
        let (x, r) = side(i as f32 / n_side as f32);
        pts.push((-x, r, false));
    }
    // shoulder: quarter circle from the top of the sidewall to the tread edge
    let (sx, sr) = side(1.0);
    let tread_hw = hw * 0.80;
    for i in 1..=n_shoulder {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / n_shoulder as f32;
        let x = tread_hw + (sx - tread_hw) * a.cos();
        let r = sr + (r1 - sr) * a.sin();
        pts.push((-x, r, i == n_shoulder));
    }
    for i in 1..n_tread {
        let x = -tread_hw + 2.0 * tread_hw * i as f32 / n_tread as f32;
        pts.push((x, r1, true));
    }
    // mirror the face side for the inner side
    let face: Vec<_> = pts.iter().take(n_side + 1 + n_shoulder).cloned().collect();
    for &(x, r, tread) in face.iter().rev() {
        pts.push((-x, r, tread));
    }
    pts
}

/// Builds the tread and the sidewalls with `segments` steps around the tyre.
pub fn build(s: &TyreSize, segments: usize) -> (TyrePart, TyrePart) {
    let sec = section(s);
    let centre = s.width / 2.0;
    let mut tread = TyrePart { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), indices: Vec::new() };
    let mut sides = tread.clone();
    // the section's outward normal at each point, from its neighbours (2D, in (axle, radius))
    let n2: Vec<(f32, f32)> = (0..sec.len())
        .map(|i| {
            let a = sec[i.saturating_sub(1)];
            let b = sec[(i + 1).min(sec.len() - 1)];
            let (dx, dr) = (b.0 - a.0, b.1 - a.1);
            let l = (dx * dx + dr * dr).sqrt().max(1e-9);
            (-dr / l, dx / l) // turning the direction of travel (face bead -> inner bead) to its outside
        })
        .collect();
    // split the section into: face sidewall, tread, inner sidewall (shared points duplicated so UVs can differ)
    let first_tread = sec.iter().position(|p| p.2).unwrap();
    let last_tread = sec.len() - 1 - sec.iter().rev().position(|p| p.2).unwrap();
    let ranges = [(0, first_tread, false, false), (first_tread, last_tread, true, false), (last_tread, sec.len() - 1, false, true)];
    for &(a, b, is_tread, inner) in &ranges {
        let part = if is_tread { &mut tread } else { &mut sides };
        let base = part.positions.len() as u32;
        let cols = b - a + 1;
        for k in 0..=segments {
            let ang = std::f32::consts::TAU * k as f32 / segments as f32;
            let (sa, ca) = ang.sin_cos();
            for (j, i) in (a..=b).enumerate() {
                let (x, r, _) = sec[i];
                let (nx, nr) = n2[i];
                part.positions.push([centre + x, r * ca, r * sa]);
                part.normals.push([nx, nr * ca, nr * sa]);
                let v = if is_tread {
                    (x - sec[a].0) / (sec[b].0 - sec[a].0)
                } else {
                    let t = (sec[i].1 - s.rim_radius) / (s.outer_radius() - s.rim_radius);
                    if inner { 1.0 - t.clamp(0.0, 1.0) } else { t.clamp(0.0, 1.0) }
                };
                let _ = j;
                part.uvs.push([k as f32 / segments as f32, v]);
            }
        }
        for k in 0..segments as u32 {
            for j in 0..(cols - 1) as u32 {
                let p = base + k * cols as u32 + j;
                let q = p + cols as u32;
                // with the section running face -> inner and the angle increasing, this order faces outward
                part.indices.extend_from_slice(&[p, q, p + 1, q, q + 1, p + 1]);
            }
        }
    }
    (tread, sides)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(part: &TyrePart) -> f32 {
        // mean cosine between each triangle's winding normal and its vertices' normals
        let p = &part.positions;
        let mut sum = 0.0;
        let mut n = 0;
        for t in part.indices.chunks_exact(3) {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let f = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let fl = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            if fl < 1e-12 {
                continue;
            }
            let vn = part.normals[t[0] as usize];
            sum += (f[0] * vn[0] + f[1] * vn[1] + f[2] * vn[2]) / fl;
            n += 1;
        }
        sum / n as f32
    }

    #[test]
    fn idx_tyre_sizes_and_winding() {
        let s = TyreSize::from_code(225.0, 40.0, 19.0);
        assert!((s.outer_radius() - 0.3313).abs() < 1e-3);
        let (tread, sides) = build(&s, 64);
        let r = |p: &[f32; 3]| (p[1] * p[1] + p[2] * p[2]).sqrt();
        let rmax = tread.positions.iter().map(r).fold(0.0, f32::max);
        let rmin = sides.positions.iter().map(r).fold(1.0, f32::min);
        assert!((rmax - s.outer_radius()).abs() < 1e-4, "outer radius {rmax}");
        assert!(rmin < s.rim_radius && rmin > s.rim_radius - 0.01, "bead radius {rmin}");
        let xs: Vec<f32> = tread.positions.iter().chain(&sides.positions).map(|p| p[0]).collect();
        let (x0, x1) = (xs.iter().cloned().fold(1.0, f32::min), xs.iter().cloned().fold(-1.0, f32::max));
        assert!(x0 > -0.02 && x1 < s.width + 0.02, "tyre spans {x0}..{x1} along the axle");
        assert!(check(&tread) > 0.9, "tread winding {}", check(&tread));
        assert!(check(&sides) > 0.9, "sidewall winding {}", check(&sides));
    }
}
