//! The third-person gauges of FM2's HUD: tick marks, numbers, red zone and needle of the tach and of the small boost gauge, laid out
//! the way the game's own script does (`I_HUD_TachHatches.lua`, read from its compiled bytecode) from the scene's meshes
//! (`imported/fm2/ui/hud.json` "meshes") and the script's settings (`kind: "tach"` items).
//!
//! From the script (`applyData`): the dial's top is the maximum value in units, rounded up (`bucket`); the numbers step by the smallest
//! whole divisor that gives fewer than `mMaxMajorHatches` intervals; an odd bucket leaves its last number off (when the dial says so)
//! and ends it half an interval later; the sweep runs from `mMinAngle` to `mMaxAngle` degrees, clockwise from straight down. Major ticks
//! sit at every number, mid ticks (half opacity) half-way, minor ticks at the quarters. The needle's own animation runs the group's z
//! rotation from `pi - minAngle` down by the sweep over the dial's range. The tach's red zone is the arc mesh seen through a texture
//! that slides along it: the part past `redline / axis max` stays (the animation moves `pivotv` by 0.964 over the range).
//!
//! Not from the files: the sign conventions of the scene's rotations (checked against the Xenia shots: 0 at the bottom, clockwise), the
//! needle's linear sweep (the animation's easing keys are not applied), and the lighting (colours are the materials' diffuse values).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde_json::Value;

/// Design-space size (px) of the pictures drawn here, and their supersampling.
const SIZE: u32 = 240;
const SS: u32 = 3;
/// How far the red-zone texture slides over the animation's range (`pivotv` 0 -> -0.964).
const RED_SLIDE: f32 = 0.964;

#[derive(Clone)]
pub struct MeshData {
    pub verts: Vec<[f32; 5]>,
    pub groups: Vec<Vec<u16>>,
}

/// One drawable mesh instance: which mesh, its pivot, its scale, colour and opacity, and where its pivot sits relative to the dial centre.
#[derive(Clone)]
pub struct Part {
    mesh: MeshData,
    pivot: [f32; 3],
    scale: f32,
    /// Extra scale along the mesh's z (length) axis.
    length_scale: f32,
    colour: [f32; 3],
    opacity: f32,
    offset: Vec2,
}

/// The tach script's settings for one dial (`TachHatches` attributes).
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub unit: f32,
    pub max_major: f32,
    pub min_angle: f32,
    pub max_angle: f32,
    pub stupid: f32,
    pub hide_odd: bool,
}

#[derive(Clone)]
pub struct DialSpec {
    /// Needle pivot (the dial centre) in scene units.
    pub centre: Vec2,
    /// The number group's origin (the script's `number_pivot`).
    pub number_centre: Vec2,
    pub params: Params,
    needle: Part,
    major: Part,
    mid: Part,
    minor: Option<Part>,
    red: Option<Part>,
    pub number_font: String,
    pub number_size: f32,
    pub number_colour: [f32; 3],
    /// The number text's offset from the group origin (scene units); the group turns it round the dial.
    pub number_offset: Vec2,
    pub z: ZIndexes,
}

#[derive(Clone, Copy)]
pub struct ZIndexes {
    pub needle: i32,
    pub ticks: i32,
    pub numbers: i32,
}

/// The dial for one car: where the ticks and numbers go and what the needle's range is.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The needle's range (the data-bound time's min and max).
    pub range: (f32, f32),
    /// Angles are clockwise from straight down, radians.
    pub numbers: Vec<(String, f32)>,
    pub major: Vec<f32>,
    pub mid: Vec<f32>,
    pub minor: Vec<f32>,
}

/// `applyData` of the tach script for a dial showing `min_value ..= max_value` (rpm for the tach, atmospheres for the boost gauge).
pub fn layout(p: &Params, min_value: f32, max_value: f32) -> Layout {
    let bucket = (max_value / p.unit).ceil();
    let floor = (min_value / p.unit).floor();
    let range = bucket - floor;
    // Lua's math.mod keeps the dividend's sign; "odd" is any remainder.
    let odd = p.hide_odd && (bucket % 2.0) != 0.0;
    let mut div = 1.0f32;
    let mut r5 = range / div + if odd { 0.5 } else { 0.0 };
    for _ in 0..64 {
        if p.max_major > r5 {
            break;
        }
        div += 1.0;
        r5 = (range / div + if odd { 0.5 } else { 0.0 }).ceil();
    }
    let r5 = r5.floor().max(1.0);
    let (r5, r6, r7, r8, r10, top) = if odd {
        let n = r5 - 1.0;
        (n, n, n, 2.0 * n, n + 0.5, (n + 0.5) * div * p.unit)
    } else {
        (r5, r5, r5 - 1.0, 2.0 * r5 - 1.0, r5, div * r5 * p.unit)
    };
    let start = p.min_angle.to_radians();
    let step = ((p.max_angle - p.min_angle) / r10.max(0.5)).to_radians();
    let ticks = |first: f32, step: f32, count: f32| (0..(count as i32 + 1).max(0)).map(|k| first + k as f32 * step).collect::<Vec<f32>>();
    let low = floor * p.unit;
    Layout {
        range: (low, low + top),
        numbers: (0..=(r5 as i32)).map(|k| (fmt_num(min_value + k as f32 * div), start + k as f32 * step)).collect(),
        major: ticks(start, step, r6),
        mid: ticks(start + step / 2.0, step, r7),
        minor: ticks(start + step / 4.0, step / 2.0, r8),
    }
}

/// `tostring` of a Lua number: whole numbers without a decimal point.
fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-4 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}

/// Needle rotation (radians, counter-clockwise positive on screen) for a fraction of the dial's range: the animation runs the group's z
/// rotation from `pi - minAngle` down by the sweep (pi to -1.22173 on the tach, 4.15388 to -0.191986 on the boost gauge).
pub fn needle_angle(p: &Params, t: f32) -> f32 {
    std::f32::consts::PI - p.min_angle.to_radians() - t.clamp(0.0, 1.0) * (p.max_angle - p.min_angle).to_radians()
}

/// Where the red zone starts, as a fraction along the arc (0 = the dial's start): the texture has slid `0.964 * redline / range`.
pub fn red_start(redline: f32, range: (f32, f32)) -> f32 {
    RED_SLIDE * ((redline - range.0) / (range.1 - range.0).max(1.0)).clamp(0.0, 1.0)
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

/// `long:250` / `float:0` / `bool:true` / `string:...` -> the value part.
fn param(it: &Value, key: &str) -> Option<String> {
    let v = it["params"][key].as_str()?;
    Some(v.split_once(':').map_or(v, |(_, rest)| rest).to_owned())
}

fn part(doc: &Value, it: &Value) -> Option<Part> {
    let key = it["mesh"].as_str()?;
    let m = &doc["meshes"][key];
    let verts = m["vertices"].as_array()?.iter().map(|v| [f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]), f(&v[4])]).collect();
    let groups = m["groups"].as_array()?.iter().map(|g| g.as_array().into_iter().flatten().map(|i| i.as_u64().unwrap_or(0) as u16).collect()).collect();
    let pivot = [f(&it["pivot"][0]), f(&it["pivot"][1]), f(&it["pivot"][2])];
    let own = f(&it["sx"]);
    let inherited = it["container_scale"][0].as_f64().map_or(1.0, |s| s as f32);
    // The container's scale only matters when the model's own is 1 (the red arc): the tick meshes carry their 72 themselves and sit in
    // unscaled groups.
    let scale = if (own - 1.0).abs() < 1e-3 { inherited } else { own };
    // Needles are scaled along their length only (the boost needle's z is 0.557 of its x); the ticks scale evenly (z / x = 1).
    let length_scale = it["sz"].as_f64().map_or(1.0, |z| z as f32 / own.max(1e-6));
    let d = &it["diffuse"];
    let colour = [f(&d[0]) / 255.0, f(&d[1]) / 255.0, f(&d[2]) / 255.0];
    let opacity = it["mat_opacity"].as_f64().map_or(1.0, |o| o as f32 / 100.0);
    Some(Part { mesh: MeshData { verts, groups }, pivot, scale, length_scale, colour, opacity, offset: Vec2::ZERO })
}

fn in_path(it: &Value, g: &str) -> bool {
    it["path"].as_array().is_some_and(|p| p.iter().any(|n| n == g))
}

impl DialSpec {
    /// The tach (`boost == false`) or the boost gauge of the third-person cluster, from `hud.json`. `None` when the file lacks the pieces.
    pub fn parse(doc: &Value, boost: bool) -> Option<Self> {
        let items = doc["scenes"]["I_HUD_All_Divided_PartTwo"].as_array()?;
        // (needle group, major tick model + its group, mid tick model + its group, the script's group name, cluster the pieces sit in)
        let (needle_g, major_n, major_g, mid_n, mid_g, group, cluster) = if boost {
            ("I_HUD_turbo_needle", "hatch_1", "hatch_pivot1", "hatch_2", "hatch_pivot2", "Turbo", "Turbo")
        } else {
            ("I_HUD_rpm_needle", "RPM_hatch_1", "hatch_pivot_1", "RPM_hatch_1", "hatch_pivot_1", "Hatches", "RPM and MPH")
        };
        let find = |name: &str, first_path: &str| items.iter().find(|it| it["mesh"].is_string() && it["name"] == name && it["path"][0] == first_path && in_path(it, cluster));
        let needle_it = find("Needle_Poly_Reduce", needle_g)?;
        let major_it = find(major_n, major_g)?;
        let mid_it = find(mid_n, mid_g)?;
        let minor_it = if boost { None } else { find("rpm_hatch_2", "hatch_pivot_2") };
        let red_it = if boost { None } else { items.iter().find(|it| it["mesh"].is_string() && it["name"] == "3P_Tach_Redline" && it["path"][0] == "Redline") };
        // The script's settings: the `TachHatches` behaviour whose major-tick path ends in this dial's tick group.
        // (The 1P and digital gauges have behaviours with the same tick group names; the 3P one sits on this needle's centre.)
        let centre0 = Vec2::new(f(&needle_it["x"]), f(&needle_it["y"]));
        let tach = items.iter().find(|it| it["kind"] == "tach" && param(it, "mMajorHatchPath").is_some_and(|p| p.ends_with(major_g)) && (Vec2::new(f(&it["x"]), f(&it["y"])) - centre0).length() < 10.0)?;
        let tach_at = Vec2::new(f(&tach["x"]), f(&tach["y"]));
        let number_it = items.iter().find(|it| it["kind"] == "text" && it["path"][0] == "number_pivot" && in_path(it, group) && (Vec2::new(f(&it["x"]), f(&it["y"])) - tach_at).length() < 120.0)?;
        let centre = Vec2::new(f(&needle_it["x"]), f(&needle_it["y"]));
        let off = |it: &Value| Vec2::new(f(&it["x"]), f(&it["y"])) - centre;
        let with_offset = |it: &Value| {
            let mut p = part(doc, it)?;
            p.offset = off(it);
            Some(p)
        };
        let number_centre = Vec2::new(f(&tach["x"]), f(&tach["y"]));
        let colour = number_it["textcolor"].as_str().map(|s| s.split_whitespace().filter_map(|t| t.parse::<f32>().ok()).collect::<Vec<_>>()).unwrap_or_default();
        let p = |k: &str, d: f32| param(tach, k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        Some(DialSpec {
            centre,
            number_centre,
            params: Params {
                unit: p("mUnitScale", 1000.0),
                max_major: p("mMaxMajorHatches", 8.0),
                min_angle: p("mMinAngle", 0.0),
                max_angle: p("mMaxAngle", 250.0),
                stupid: p("mStupidParameter", -90.0),
                hide_odd: param(tach, "mHideLastNumberIfOdd").is_some_and(|v| v == "true"),
            },
            needle: part(doc, needle_it)?,
            major: with_offset(major_it)?,
            mid: with_offset(mid_it)?,
            minor: minor_it.and_then(with_offset),
            red: red_it.and_then(with_offset),
            number_font: number_it["font"].as_str().unwrap_or("handelgothicef-medium").to_owned(),
            number_size: number_it["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(13.0),
            number_colour: if colour.len() >= 3 { [colour[0] / 255.0, colour[1] / 255.0, colour[2] / 255.0] } else { [0.57; 3] },
            number_offset: Vec2::new(f(&number_it["x"]) - number_centre.x, f(&number_it["y"]) - number_centre.y),
            z: ZIndexes { needle: 4, ticks: 2, numbers: 2 },
        })
    }

    pub fn needle_image(&self) -> Image {
        let mut canvas = Canvas::new();
        canvas.mesh(&self.needle, 0.0, |_| true);
        canvas.finish()
    }

    /// The static dial picture for a layout: the red zone (tach only, from `redline`), then the ticks.
    pub fn dial_image(&self, layout: &Layout, redline: f32) -> Image {
        let mut canvas = Canvas::new();
        if let Some(red) = &self.red {
            let start = red_start(redline, layout.range);
            canvas.mesh(red, 0.0, |v| v >= start);
        }
        let mut mid = self.mid.clone();
        mid.opacity *= 0.5;
        let mut sets: Vec<(&Vec<f32>, &Part)> = vec![(&layout.mid, &mid)];
        if let Some(m) = &self.minor {
            sets.push((&layout.minor, m));
        }
        sets.push((&layout.major, &self.major));
        for (set, part) in sets {
            for &a in set {
                // The group turns clockwise by `a` from the bottom (negative z rotation).
                canvas.mesh(part, -a, |_| true);
            }
        }
        canvas.finish()
    }
}

/// A SIZE x SIZE canvas of premultiplied float RGBA samples at SS x supersampling; the dial centre is its middle.
struct Canvas {
    w: usize,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    fn new() -> Self {
        let w = (SIZE * SS) as usize;
        Self { w, px: vec![[0.0; 4]; w * w] }
    }

    /// Draws a mesh rotated by `angle` (radians, counter-clockwise) about the dial centre. `keep` decides per sample by the mesh's v.
    fn mesh(&mut self, p: &Part, angle: f32, keep: impl Fn(f32) -> bool) {
        let (s, c) = angle.sin_cos();
        let k = SS as f32;
        let half = self.w as f32 / 2.0;
        // Screen of a vertex: the model's x, z (y up) relative to the pivot, scaled, turned about the centre, plus the pivot's offset.
        let to_px = |v: &[f32; 5]| {
            let (lx, ly) = ((v[0] - p.pivot[0]) * p.scale + p.offset.x, (v[2] - p.pivot[2]) * p.scale * p.length_scale + p.offset.y);
            let (rx, ry) = (lx * c - ly * s, lx * s + ly * c);
            Vec2::new(half + rx * k, half - ry * k)
        };
        for g in &p.mesh.groups {
            for t in g.chunks_exact(3) {
                let v = [p.mesh.verts[t[0] as usize], p.mesh.verts[t[1] as usize], p.mesh.verts[t[2] as usize]];
                let (a, b, cc) = (to_px(&v[0]), to_px(&v[1]), to_px(&v[2]));
                let den = (b.y - cc.y) * (a.x - cc.x) + (cc.x - b.x) * (a.y - cc.y);
                if den.abs() < 1e-9 {
                    continue;
                }
                let (x0, x1) = (a.x.min(b.x).min(cc.x).floor().max(0.0) as usize, (a.x.max(b.x).max(cc.x).ceil() as usize).min(self.w - 1));
                let (y0, y1) = (a.y.min(b.y).min(cc.y).floor().max(0.0) as usize, (a.y.max(b.y).max(cc.y).ceil() as usize).min(self.w - 1));
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let (qx, qy) = (x as f32 + 0.5, y as f32 + 0.5);
                        let l1 = ((b.y - cc.y) * (qx - cc.x) + (cc.x - b.x) * (qy - cc.y)) / den;
                        let l2 = ((cc.y - a.y) * (qx - cc.x) + (a.x - cc.x) * (qy - cc.y)) / den;
                        let l3 = 1.0 - l1 - l2;
                        if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                            continue;
                        }
                        let vv = l1 * v[0][4] + l2 * v[1][4] + l3 * v[2][4];
                        if !keep(vv) {
                            continue;
                        }
                        // Source-over, premultiplied.
                        let dst = &mut self.px[y * self.w + x];
                        let a_src = p.opacity;
                        for ch in 0..3 {
                            dst[ch] = p.colour[ch] * a_src + dst[ch] * (1.0 - a_src);
                        }
                        dst[3] = a_src + dst[3] * (1.0 - a_src);
                    }
                }
            }
        }
    }

    fn finish(self) -> Image {
        let n = SIZE as usize;
        let ss = SS as usize;
        let mut data = Vec::with_capacity(n * n * 4);
        for y in 0..n {
            for x in 0..n {
                let mut acc = [0.0f32; 4];
                for dy in 0..ss {
                    for dx in 0..ss {
                        let q = self.px[(y * ss + dy) * self.w + x * ss + dx];
                        for ch in 0..4 {
                            acc[ch] += q[ch];
                        }
                    }
                }
                let a = acc[3] / (ss * ss) as f32;
                let un = |c: f32| if acc[3] > 0.0 { (c / acc[3]).clamp(0.0, 1.0) } else { 0.0 };
                // Straight alpha in sRGB bytes (the diffuse values are sRGB colours as the scene authors them).
                data.extend_from_slice(&[(un(acc[0]) * 255.0).round() as u8, (un(acc[1]) * 255.0).round() as u8, (un(acc[2]) * 255.0).round() as u8, (a * 255.0).round() as u8]);
            }
        }
        Image::new(Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const TACH: Params = Params { unit: 1000.0, max_major: 8.0, min_angle: 0.0, max_angle: 250.0, stupid: -90.0, hide_odd: true };
    const BOOST: Params = Params { unit: 1.0, max_major: 5.0, min_angle: -58.0, max_angle: 191.0, stupid: 90.0, hide_odd: false };

    /// Skyline-like car, top 10000 rpm: numbers 0..10 by 2, six major, five mid, ten minor ticks, 50 degree steps.
    #[test]
    fn even_dial() {
        let l = layout(&TACH, 0.0, 10_000.0);
        assert_eq!(l.range, (0.0, 10_000.0));
        assert_eq!(l.numbers.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["0", "2", "4", "6", "8", "10"]);
        assert_eq!((l.major.len(), l.mid.len(), l.minor.len()), (6, 5, 10));
        assert!((l.major[1] - 50f32.to_radians()).abs() < 1e-5);
        assert!((l.mid[0] - 25f32.to_radians()).abs() < 1e-5 && (l.minor[0] - 12.5f32.to_radians()).abs() < 1e-5);
    }

    /// An odd top (9000): the last number is left off and the dial ends a half interval later (9000), 55.6 degree steps.
    #[test]
    fn odd_dial() {
        let l = layout(&TACH, 0.0, 9_000.0);
        assert_eq!(l.range.1, 9_000.0);
        assert_eq!(l.numbers.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["0", "2", "4", "6", "8"]);
        assert!((l.major[1] - (250.0f32 / 4.5).to_radians()).abs() < 1e-5);
    }

    /// The boost gauge of a turbo car (-1 .. 1 atm) reads -1, 0, 1: -1 at the lower right (-58 degrees), 1 at the top; the Xenia shot shows
    /// exactly these three labels.
    #[test]
    fn boost_dial() {
        let l = layout(&BOOST, -1.0, 1.0);
        assert_eq!(l.range, (-1.0, 1.0));
        assert_eq!(l.numbers.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["-1", "0", "1"]);
        assert!((l.numbers[0].1 - (-58f32).to_radians()).abs() < 1e-5);
        assert!((l.numbers[2].1 - 191f32.to_radians()).abs() < 1e-4);
    }

    #[test]
    fn needle() {
        assert!((needle_angle(&TACH, 0.0) - std::f32::consts::PI).abs() < 1e-6);
        assert!((needle_angle(&TACH, 1.0) - (-1.22173)).abs() < 1e-4);
        assert!((needle_angle(&BOOST, 0.0) - 4.15388).abs() < 1e-4);
        assert!((needle_angle(&BOOST, 1.0) - (-0.191986)).abs() < 1e-4);
    }
}
