//! The minimap of FM2's HUD: the track's centre line around the car, heading up, in the rectangle the scene gives the map
//! (`MiniMapQuad`: a 175 x 109 plane whose origin is the car; imported as `imported/fm2/ui/hud.json`).
//!
//! The game draws this with a native renderer (`CustomRendererID MiniMap`, not in the scene), so how it looks is read from the Xenia shots:
//! the line is white, about 4 px wide with a black edge, the car is a green arrow at the map's origin and the start / finish line a
//! yellow bar across the track. The scale (design pixels per metre) is INFERRED from those shots (see `SCALE`); the line itself is
//! the importer's `ai/line.json` centre line (the game's own, 2 m spacing).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde_json::Value;

/// Design-space rectangle of the map relative to its origin (the car): from the scene's `Track_Map_Placeholder` mesh (x -89.91 .. 85.5,
/// up -50.89 .. 57.86).
/// The rectangle is centred on the quad (pivot = bbox centre, so +-87.7 x +-54.4); the car sits 0.9 px left of and 20.1 px below that
/// centre (measured: the green arrow's centroid is (172.7, 162.7) in all twelve Xenia shots, the quad centre (173.6, 142.6)).
const LEFT: f32 = 86.8;
const RIGHT: f32 = 88.6;
const BELOW: f32 = 34.3;
const ABOVE: f32 = 74.5;
/// Where the car sits relative to the quad's centre (design px, y up).
const CAR_OFFSET: Vec2 = Vec2::new(-0.9, -20.1);
/// Supersampling of the map picture.
const SS: f32 = 2.0;
/// Design pixels per metre (fitted: the best overlap of this line with the white pixels of ten Xenia maps, each at its best position on the line, is at 0.20 for ten of twelve tracks; Sunset Infield and the Nurburgring fit badly).
pub const SCALE: f32 = 0.20;
const WHITE_W: f32 = 4.5;
const EDGE_W: f32 = 6.5;

pub struct Minimap {
    /// The centre line in engine space (x, z), closed.
    pts: Vec<Vec2>,
    pub handle: Handle<Image>,
    /// Where the map's origin sits on the 1280 x 720 design space (y up).
    pub origin: Vec2,
}

impl Minimap {
    /// Reads the centre line (`ai/line.json`: x, y, z in the files' left-handed z) and the map's place from `hud.json`.
    pub fn load(line_json: &std::path::Path, hud: &Value, images: &mut Assets<Image>) -> Option<Self> {
        let d: Value = serde_json::from_slice(&std::fs::read(line_json).ok()?).ok()?;
        let pts: Vec<Vec2> = d["centre"].as_array()?.iter().filter_map(|p| Some(Vec2::new(p[0].as_f64()? as f32, -(p[2].as_f64()? as f32)))).collect();
        if pts.len() < 3 {
            return None;
        }
        let q = hud["scenes"]["I_HUD_All_Divided_PartOne"].as_array()?.iter().find(|it| it["name"] == "MiniMapQuad")?;
        let origin = Vec2::new(q["x"].as_f64()? as f32, q["y"].as_f64()? as f32) + CAR_OFFSET;
        let (w, h) = (((LEFT + RIGHT) * SS) as u32, ((BELOW + ABOVE) * SS) as u32);
        let img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, vec![0; (w * h * 4) as usize], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
        Some(Self { pts, handle: images.add(img), origin })
    }

    /// Top-left of the picture in design space (x right, y up -> top edge).
    pub fn rect(&self) -> (Vec2, Vec2) {
        (Vec2::new(self.origin.x - LEFT, self.origin.y + ABOVE), Vec2::new(LEFT + RIGHT, BELOW + ABOVE))
    }

    /// Redraws the picture for a car at `pos` (engine x, z) facing `fwd` (unit, engine x, z).
    pub fn draw(&self, images: &mut Assets<Image>, pos: Vec2, fwd: Vec2) {
        let Some(mut img) = images.get_mut(&self.handle) else { return };
        let (w, h) = (img.width() as usize, img.height() as usize);
        let Some(data) = img.data.as_mut() else { return };
        data.fill(0);
        let right = Vec2::new(-fwd.y, fwd.x);
        // Map metres -> picture pixels (x right, row down from the top edge).
        let to_px = |p: Vec2| {
            let rel = p - pos;
            let (x, ahead) = (rel.dot(right), rel.dot(fwd));
            Vec2::new((LEFT + x * SCALE) * SS, (ABOVE - ahead * SCALE) * SS)
        };
        let px: Vec<Vec2> = self.pts.iter().map(|&p| to_px(p)).collect();
        let n = px.len();
        // Edge first (black), then the white line over it: one pass per width.
        for (width, colour) in [(EDGE_W, [0u8, 0, 0, 255]), (WHITE_W, [255u8, 255, 255, 255])] {
            // Open at the start line: Xenia draws nothing behind it (Sunset Infield and the Nurburgring show a line that ends at the bar).
            for i in 0..n - 1 {
                let (a, b) = (px[i], px[i + 1]);
                if (a.x < -8.0 && b.x < -8.0) || (a.x > w as f32 + 8.0 && b.x > w as f32 + 8.0) || (a.y < -8.0 && b.y < -8.0) || (a.y > h as f32 + 8.0 && b.y > h as f32 + 8.0) {
                    continue;
                }
                stroke(data, w, h, a, b, width * SS / 2.0, colour);
            }
        }
        // Start / finish: a yellow bar across the line at point 0.
        let (s0, s1) = (px[0], px[1 % n]);
        let dir = (s1 - s0).normalize_or(Vec2::Y);
        let across = Vec2::new(-dir.y, dir.x);
        stroke(data, w, h, s0 - across * 8.0 * SS, s0 + across * 8.0 * SS, 2.4 * SS, [240, 200, 20, 255]);
        // The car: a green arrow at the origin, pointing up.
        let o = Vec2::new(LEFT * SS, ABOVE * SS);
        // The Xenia arrow: a 16 x 24 px head with a notch at the back, black edge.
        let pts = [(0.0, -12.0), (8.0, 12.0), (0.0, 6.0), (-8.0, 12.0)];
        let big = |k: f32| -> Vec<Vec2> { pts.iter().map(|&(x, y)| o + Vec2::new(x * k, y * k) * SS).collect() };
        for (k, col) in [(1.18, [0u8, 0, 0, 255]), (1.0, [30u8, 220, 40, 255])] {
            let v = big(k);
            fill_triangle(data, w, h, [v[0], v[1], v[2]], col);
            fill_triangle(data, w, h, [v[0], v[2], v[3]], col);
        }
    }
}

/// A line segment with round ends, `r` pixels each side.
fn stroke(data: &mut [u8], w: usize, h: usize, a: Vec2, b: Vec2, r: f32, c: [u8; 4]) {
    let (x0, x1) = ((a.x.min(b.x) - r - 1.0).floor().max(0.0) as usize, ((a.x.max(b.x) + r + 1.0).ceil() as usize).min(w.saturating_sub(1)));
    let (y0, y1) = ((a.y.min(b.y) - r - 1.0).floor().max(0.0) as usize, ((a.y.max(b.y) + r + 1.0).ceil() as usize).min(h.saturating_sub(1)));
    let ab = b - a;
    let len2 = ab.length_squared().max(1e-6);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
            let d = (p - (a + ab * t)).length();
            // One pixel of antialiasing at the edge.
            let cover = (r + 0.5 - d).clamp(0.0, 1.0);
            if cover > 0.0 {
                blend(data, (y * w + x) * 4, c, cover);
            }
        }
    }
}

fn fill_triangle(data: &mut [u8], w: usize, h: usize, t: [Vec2; 3], c: [u8; 4]) {
    let (min, max) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
    let den = (t[1].y - t[2].y) * (t[0].x - t[2].x) + (t[2].x - t[1].x) * (t[0].y - t[2].y);
    if den.abs() < 1e-6 {
        return;
    }
    for y in (min.y.floor().max(0.0) as usize)..=((max.y.ceil() as usize).min(h.saturating_sub(1))) {
        for x in (min.x.floor().max(0.0) as usize)..=((max.x.ceil() as usize).min(w.saturating_sub(1))) {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let l1 = ((t[1].y - t[2].y) * (px - t[2].x) + (t[2].x - t[1].x) * (py - t[2].y)) / den;
            let l2 = ((t[2].y - t[0].y) * (px - t[2].x) + (t[0].x - t[2].x) * (py - t[2].y)) / den;
            if l1 >= 0.0 && l2 >= 0.0 && l1 + l2 <= 1.0 {
                blend(data, (y * w + x) * 4, c, 1.0);
            }
        }
    }
}

/// Source-over onto straight-alpha bytes.
fn blend(data: &mut [u8], o: usize, c: [u8; 4], a: f32) {
    let da = data[o + 3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    for ch in 0..3 {
        let d = data[o + ch] as f32;
        data[o + ch] = ((c[ch] as f32 * a + d * da * (1.0 - a)) / out_a.max(1e-4)).round().clamp(0.0, 255.0) as u8;
    }
    data[o + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}
