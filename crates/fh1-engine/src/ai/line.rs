//! The game's AI racing lines: `media/aiopenworld.zip` `<track>/Ribbon_00/route_NNN.owt` (docs/AI.md "Racing lines").
//!
//! `OWTM` file, big-endian (VERIFIED on all 46 Colorado routes):
//! - header 32 bytes: "OWTM", u32 version 1, 0, 0, u32 count, u32 closed loop (1 = circuit: the last point leads back to
//!   the first, ~2 m apart; 0 = point-to-point), 0, 0;
//! - count x 48 bytes: centre xyz f32 + pad; lateral xyz f32 + pad = the road's half-width vector (|v| ~6 m, 2.5-8 m on
//!   narrow/wide parts) pointing to the LEFT of the direction of travel; racing-line offset f32 in [-1, 1] (positive towards
//!   the lateral vector; it moves to the inside of every corner at the apex) + 3 pad;
//! - tail 16 bytes: "OWTM", 1, 0, 0.
//! Points are ~2 m apart in the game's left-handed space (same as TrackRoute XML): Z is negated on the way in. The racing
//! line = centre + lateral x offset. No speeds or braking points are stored.

use anyhow::{ensure, Result};
use bevy::math::Vec3;

/// One route's racing line in engine space.
#[derive(Debug, Clone)]
pub struct RacingLine {
    pub closed: bool,
    /// Road centre per point.
    pub centre: Vec<Vec3>,
    /// Half-width vector per point (points left of travel; its length = half the road's width).
    pub lateral: Vec<Vec3>,
    /// Racing-line offset fraction per point (-1..1, + = left).
    pub offset: Vec<f32>,
    /// The racing line itself: centre + lateral x offset.
    pub points: Vec<Vec3>,
    /// Distance along the racing line at each point (m); `length` = full lap (closing segment included when closed).
    pub s: Vec<f32>,
    pub length: f32,
    /// Lowest road-centre height on the line (m), for the "fell out of the world" reset.
    pub min_y: f32,
}

/// Where a position sits relative to the line.
#[derive(Debug, Clone, Copy, Default)]
pub struct Projection {
    /// Segment start index (the position lies between `index` and the next point).
    pub index: usize,
    /// Fraction along that segment.
    pub t: f32,
    /// Distance along the line (m).
    pub s: f32,
    /// Signed lateral distance from the road centre (m, + = left).
    pub lateral: f32,
    /// Half the road's width there (m).
    pub half_width: f32,
    /// Distance from the racing line (m, 3D).
    pub distance: f32,
}

impl RacingLine {
    pub fn parse_owt(bytes: &[u8], mirror_z: bool) -> Result<Self> {
        ensure!(bytes.len() >= 32 && &bytes[0..4] == b"OWTM", "not an OWTM racing line");
        let u = |o: usize| u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap());
        let f = |o: usize| f32::from_bits(u(o));
        let count = u(16) as usize;
        let closed = u(20) == 1;
        ensure!(count >= 2 && bytes.len() >= 32 + count * 48, "OWTM: {count} points don't fit in {} bytes", bytes.len());
        let mz = if mirror_z { -1.0 } else { 1.0 };
        let (mut centre, mut lateral, mut offset) = (Vec::with_capacity(count), Vec::with_capacity(count), Vec::with_capacity(count));
        for i in 0..count {
            let o = 32 + i * 48;
            centre.push(Vec3::new(f(o), f(o + 4), f(o + 8) * mz));
            lateral.push(Vec3::new(f(o + 16), f(o + 20), f(o + 24) * mz));
            offset.push(f(o + 32).clamp(-1.0, 1.0));
        }
        Ok(Self::new(closed, centre, lateral, offset))
    }

    pub fn load(path: &std::path::Path, mirror_z: bool) -> Result<Self> {
        Self::parse_owt(&std::fs::read(path)?, mirror_z)
    }

    /// An imported map's racing line (`imported/<map>/ai/line.json`, written by `fh1setup` for FM2 / FM4): `centre` (the
    /// centreline), `points` (the game's optimal line), `left_wall_m` / `right_wall_m` (distances from the centre to each wall,
    /// both positive), `closed`. Coordinates are the importer's (game) space, so z is flipped like `spawns.json`. The road is
    /// re-centred between the walls; the lateral vector is the centreline's left normal scaled to half the wall-to-wall width,
    /// and the offset is the optimal point's position across it.
    pub fn parse_imported(bytes: &[u8], mirror_z: bool) -> Result<Self> {
        let j: serde_json::Value = serde_json::from_slice(bytes)?;
        let mz = if mirror_z { -1.0 } else { 1.0 };
        let vecs = |k: &str| -> Vec<Vec3> {
            j[k].as_array().map_or_else(Vec::new, |a| {
                a.iter().map(|p| Vec3::new(p[0].as_f64().unwrap_or(0.0) as f32, p[1].as_f64().unwrap_or(0.0) as f32, p[2].as_f64().unwrap_or(0.0) as f32 * mz)).collect()
            })
        };
        let nums = |k: &str| -> Vec<f32> { j[k].as_array().map_or_else(Vec::new, |a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()) };
        let (centre0, opt, left, right) = (vecs("centre"), vecs("points"), nums("left_wall_m"), nums("right_wall_m"));
        let n = centre0.len();
        ensure!(n >= 2 && opt.len() == n && left.len() == n && right.len() == n, "ai/line.json: {n} centre points, {} line points, {} / {} walls", opt.len(), left.len(), right.len());
        let closed = j["closed"].as_bool().unwrap_or(true);
        let (mut centre, mut lateral, mut offset) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let prev = if i == 0 { if closed { n - 1 } else { 0 } } else { i - 1 };
            let next = if i + 1 == n { if closed { 0 } else { n - 1 } } else { i + 1 };
            let fwd = (centre0[next] - centre0[prev]).with_y(0.0).normalize_or(Vec3::NEG_Z);
            // Left of travel (engine space, +Y up).
            let left_dir = Vec3::Y.cross(fwd).normalize_or(Vec3::X);
            let half = ((left[i] + right[i]) * 0.5).max(1.0);
            let c = centre0[i] + left_dir * ((left[i] - right[i]) * 0.5);
            let lat = left_dir * half;
            centre.push(c);
            lateral.push(lat);
            offset.push(((opt[i] - c).dot(left_dir) / half).clamp(-1.0, 1.0));
        }
        Ok(Self::new(closed, centre, lateral, offset))
    }

    /// Re-fit the road around the racing line to measured edges: `left_m` / `right_m` = distance from each racing-line point
    /// to the road's edge on its left / right (imported lines: the tarmac edge from the collision surfaces instead of the wall
    /// distance). The racing line itself is unchanged; the centre, half width and offset follow the new edges. Each side is
    /// the minimum over +-2 points (one stray sample does not widen the road), at least 1.5 m.
    pub fn refit_road(&mut self, left_m: &[f32], right_m: &[f32]) {
        let n = self.points.len();
        let near_min = |v: &[f32], i: usize| (0..5).map(|k| v[(i + n + k - 2) % n]).fold(f32::INFINITY, f32::min);
        for i in 0..n {
            let dir = self.lateral[i].normalize_or_zero();
            let (l, r) = (near_min(left_m, i).max(1.5), near_min(right_m, i).max(1.5));
            let half = 0.5 * (l + r);
            let p = self.points[i];
            let c = p + dir * (0.5 * (l - r));
            self.centre[i] = c;
            self.lateral[i] = dir * half;
            self.offset[i] = ((p - c).dot(dir) / half).clamp(-1.0, 1.0);
        }
        self.min_y = self.centre.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
    }

    pub fn load_imported(path: &std::path::Path, mirror_z: bool) -> Result<Self> {
        Self::parse_imported(&std::fs::read(path)?, mirror_z)
    }

    pub fn new(closed: bool, centre: Vec<Vec3>, lateral: Vec<Vec3>, offset: Vec<f32>) -> Self {
        let points: Vec<Vec3> = (0..centre.len()).map(|i| centre[i] + lateral[i] * offset[i]).collect();
        let mut s = Vec::with_capacity(points.len());
        let mut acc = 0.0;
        for i in 0..points.len() {
            if i > 0 {
                acc += points[i].distance(points[i - 1]);
            }
            s.push(acc);
        }
        let length = if closed { acc + points[points.len() - 1].distance(points[0]) } else { acc };
        let min_y = centre.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
        Self { closed, centre, lateral, offset, points, s, length, min_y }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Index `i` wrapped (closed) or clamped (open).
    pub fn wrap(&self, i: isize) -> usize {
        let n = self.len() as isize;
        if self.closed {
            i.rem_euclid(n) as usize
        } else {
            i.clamp(0, n - 1) as usize
        }
    }

    /// Distance along the line wrapped into 0..length (closed) or clamped (open).
    pub fn wrap_s(&self, s: f32) -> f32 {
        if self.closed {
            s.rem_euclid(self.length)
        } else {
            s.clamp(0.0, self.length)
        }
    }

    /// Segment index containing distance `s` (wrapped).
    pub fn index_at(&self, s: f32) -> usize {
        let s = self.wrap_s(s);
        match self.s.binary_search_by(|x| x.total_cmp(&s)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// Length of segment i (to the next point; the closing segment on circuits).
    fn seg_len(&self, i: usize) -> f32 {
        if i + 1 < self.len() {
            self.s[i + 1] - self.s[i]
        } else if self.closed {
            self.length - self.s[i]
        } else {
            0.0
        }
    }

    /// Interpolate a per-point quantity at distance `s`.
    fn lerp_at<T: Copy + std::ops::Mul<f32, Output = T> + std::ops::Add<Output = T>>(&self, s: f32, v: &[T]) -> T {
        let s = self.wrap_s(s);
        let i = self.index_at(s);
        let j = self.wrap(i as isize + 1);
        let len = self.seg_len(i);
        let t = if len > 1e-4 { ((s - self.s[i]) / len).clamp(0.0, 1.0) } else { 0.0 };
        v[i] * (1.0 - t) + v[j] * t
    }

    /// Racing-line position at distance `s`.
    pub fn point_at(&self, s: f32) -> Vec3 {
        self.lerp_at(s, &self.points)
    }

    /// Road centre, left half-width vector and racing-line offset fraction at distance `s`.
    pub fn road_at(&self, s: f32) -> (Vec3, Vec3, f32) {
        (self.lerp_at(s, &self.centre), self.lerp_at(s, &self.lateral), self.lerp_at(s, &self.offset))
    }

    /// Point at distance `s` with the racing-line offset replaced by `lateral_m` metres left of the centre.
    pub fn point_with_lateral(&self, s: f32, lateral_m: f32) -> Vec3 {
        let (c, l, _) = self.road_at(s);
        c + l.normalize_or_zero() * lateral_m
    }

    /// Unit direction of travel at distance `s`.
    pub fn tangent_at(&self, s: f32) -> Vec3 {
        let a = self.point_at(s - 2.0);
        let b = self.point_at(s + 2.0);
        (b - a).normalize_or(Vec3::NEG_Z)
    }

    /// Yaw (radians about +Y, 0 = facing -Z; `Vehicle::place` convention) of the direction of travel at `s`.
    pub fn yaw_at(&self, s: f32) -> f32 {
        let t = self.tangent_at(s);
        (-t.x).atan2(-t.z)
    }

    /// Project a position onto the line. With `hint` (last frame's index) only a window around it is searched, so a
    /// car on a part of the route that crosses itself keeps its place.
    pub fn project(&self, p: Vec3, hint: Option<usize>) -> Projection {
        let n = self.len();
        let range: Box<dyn Iterator<Item = usize>> = match hint {
            Some(h) => {
                const BACK: isize = 20;
                const AHEAD: isize = 60;
                let segs = if self.closed { n } else { n - 1 };
                Box::new((-BACK..=AHEAD).map(move |k| h as isize + k).filter_map(move |i| {
                    if self.closed {
                        Some(i.rem_euclid(n as isize) as usize)
                    } else {
                        (i >= 0 && (i as usize) < segs).then_some(i as usize)
                    }
                }))
            }
            None => Box::new(0..if self.closed { n } else { n - 1 }),
        };
        let mut best = (f32::MAX, 0usize, 0.0f32);
        for i in range {
            let a = self.points[i];
            let b = self.points[self.wrap(i as isize + 1)];
            let ab = b - a;
            let t = if ab.length_squared() > 1e-6 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
            let d = p.distance_squared(a + ab * t);
            if d < best.0 {
                best = (d, i, t);
            }
        }
        let (d2, i, t) = best;
        let s = self.s[i] + self.seg_len(i) * t;
        let (c, l, _) = self.road_at(s);
        let half_width = l.length();
        let lateral = if half_width > 1e-4 { (p - c).dot(l / half_width) } else { 0.0 };
        Projection { index: i, t, s, lateral, half_width, distance: d2.sqrt() }
    }

    /// The line with its offsets limited like the game's RaceTableReader (AIRacing.xml ShouldLimitChi, default set): the
    /// car's centre keeps `inner` metres from the road edge on the inside of a bend and `outer` on the outside (negative =
    /// may run past the edge onto the kerb), plus `shift` (fraction) added to every offset (ChiVariance, per driver).
    /// Inside / outside from the road centre's curvature. Our reading of the margins (INFERRED).
    pub fn limited(&self, inner: f32, outer: f32, shift: f32) -> Self {
        let centre = Self::new(self.closed, self.centre.clone(), self.lateral.clone(), vec![0.0; self.len()]);
        let offset = (0..self.len())
            .map(|i| {
                let hw = self.lateral[i].length().max(0.5);
                let o = self.offset[i] + shift;
                // + = left; turning left -> left is the inside.
                let left_inside = centre.curvature(i, 4) >= 0.0;
                let (lim_left, lim_right) = if left_inside { (inner, outer) } else { (outer, inner) };
                o.clamp(-(1.0 - lim_right / hw), 1.0 - lim_left / hw)
            })
            .collect();
        Self::new(self.closed, self.centre.clone(), self.lateral.clone(), offset)
    }

    /// Signed curvature (1/m, + = turning left) of the racing line at point i, from the circle through i-k, i, i+k
    /// (k points ~ 2k x 2 m apart; horizontal plane).
    pub fn curvature(&self, i: usize, k: usize) -> f32 {
        let a = self.points[self.wrap(i as isize - k as isize)];
        let b = self.points[i];
        let c = self.points[self.wrap(i as isize + k as isize)];
        let (a, b, c) = (bevy::math::Vec2::new(a.x, a.z), bevy::math::Vec2::new(b.x, b.z), bevy::math::Vec2::new(c.x, c.z));
        let (ab, bc, ca) = (b - a, c - b, a - c);
        let denom = ab.length() * bc.length() * ca.length();
        if denom < 1e-6 {
            return 0.0;
        }
        // 2D cross in the xz plane: engine space +X right, -Z forward; a left turn has ab.x*bc.y - ab.y*bc.x < 0 with y = z.
        let cross = ab.x * bc.y - ab.y * bc.x;
        -2.0 * cross / denom
    }
}
