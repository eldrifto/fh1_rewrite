//! Forza Motorsport 2 (Xbox 360, 2007) track scenery: the `.pvs` (version 0xB) texture and per-model tables and the
//! `<track>out.NNNNN.rmb.bin` models (`CAFF21.11.05.0034`: `.gpu` vertex / index buffers, `.data` "rendergraph", `.stream`
//! recorded draw commands). Reverse-engineered from the disc, not from a spec; what is VERIFIED is marked.
//!
//! PVS (VERIFIED on Silverstone): the texture table is `count` 28-byte records `{file_id, index, f32 x2, u32 x2, flags}`
//! (flags 4 = `.bix` pair, 0 = `.bin` CAFF, 1 = runtime, 8 / 9 / 12 = lightmap atlas tile whose `index` is the `.stx.bin`
//! entry of its page and whose f32 pair is [scale u, scale v] and u32 pair [offset u, offset v] as floats; negative
//! scale u = the tile is stored mirrored); right after it comes `u32 model count` and, per model, `u32 N`, `N` PVS
//! texture indices, 12 words of other data and a marker word (0x64 or 1.0f) before the next record. Model `i` is `<prefix>.<i>.rmb.bin`.
//!
//! Model (VERIFIED on Silverstone model 10): the sections follow the 0x190-byte header as `.gpu`, `.data`, `.stream`. A draw is
//! the `.stream` records between `0x2e` (vertex fetch: word 1 = offset of the vertex buffer record in `.data`), `0x2d` (texture
//! bindings: word 1 = count << 16, then (item, ..) pairs; item = index into the model's PVS texture list, in sampler order) and
//! `0x01` (`[4 = triangle list, index count, key]`; the index buffer record `[key, gpu offset, bytes, 1]` is found in `.data`).
//! Vertex buffer records are `[stride, pointer, gpu offset, bytes]`. Vertices: big-endian f32 x3 position, then 16-bit normalised
//! texture coordinate pairs (which offset is the diffuse set is a density heuristic, see [`Draw::uv_offsets`]). The mask /
//! colour uv is read **signed** ([`Draw::uv_at`], verified on the mask atlas); the lightmap uv is the other pair read as
//! **unorm 0..1 over its atlas tile** ([`Draw::uv_unorm`], verified: it spans 0..1 exactly on terrain draws).

use crate::Error;

type Result<T> = std::result::Result<T, Error>;

fn be(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// A texture table entry.
#[derive(Debug, Clone, Copy)]
pub struct Texture {
    pub file_id: u32,
    pub flags: u32,
    /// The record's `index` field. Ordinary textures: the record's own table index. Lightmap atlas tiles (flags 8 / 9 /
    /// 12): the `.stx.bin` entry of the atlas page the tile is cut from.
    pub object: u32,
    /// Lightmap atlas tiles (flags 8 / 9 / 12): [scale u, scale v, offset u, offset v] of the tile inside its page
    /// (**negative** scale u = the tile is stored mirrored).
    pub tile: [f32; 4],
}

#[derive(Debug, Default)]
pub struct Pvs {
    pub textures: Vec<Texture>,
    /// Per model number: its PVS texture indices.
    pub models: Vec<Vec<u32>>,
}

/// Reads the texture table and the per-model texture lists of an FM2 `.pvs`.
pub fn parse_pvs(d: &[u8]) -> Result<Pvs> {
    if d.get(0..4) != Some(b"FPVS") {
        return Err(Error::BadMagic("FM2 pvs"));
    }
    // The table: records whose index field counts 0, 1, 2, 3. The file's tables are not 4-aligned on every track (LagunaSeca's
    // sits at an odd byte offset), so every byte offset is tried (~0.6 MB).
    let start = (4..d.len().saturating_sub(28 * 4)).find(|&o| (0..4).all(|k| be(d, o + 4 + 28 * k) == Some(k as u32)) && be(d, o + 8) == Some(0x3f800000));
    let start = start.ok_or(Error::Truncated("pvs texture table"))?;
    let count = be(d, start - 4).ok_or(Error::Truncated("pvs texture count"))? as usize;
    let mut textures = Vec::with_capacity(count);
    for i in 0..count {
        let o = start + 28 * i;
        let w = |k: usize| be(d, o + 4 * k).ok_or(Error::Truncated("pvs texture"));
        textures.push(Texture {
            file_id: w(0)?,
            flags: w(6)?,
            object: w(1)?,
            tile: [f32::from_bits(w(2)?), f32::from_bits(w(3)?), f32::from_bits(w(4)?), f32::from_bits(w(5)?)],
        });
    }
    let mut o = start + 28 * count;
    let n = be(d, o).ok_or(Error::Truncated("pvs model count"))? as usize;
    o += 4;
    let mut models = Vec::with_capacity(n.min(1 << 16));
    for _ in 0..n {
        let Some(k) = be(d, o) else { break };
        let k = k as usize;
        if k > 256 {
            break;
        }
        o += 4;
        let mut items = Vec::with_capacity(k);
        for j in 0..k {
            items.push(be(d, o + 4 * j).ok_or(Error::Truncated("pvs model list"))?);
        }
        // 12 words of other data and one marker word (0x64 or 1.0f).
        o += 4 * k + 52;
        models.push(items);
    }
    Ok(Pvs { textures, models })
}

/// One draw: a triangle list over one vertex buffer.
#[derive(Debug, Clone)]
pub struct Draw {
    pub stride: usize,
    pub positions: Vec<[f32; 3]>,
    /// Raw big-endian vertices (`stride` bytes each).
    pub vertices: Vec<u8>,
    pub indices: Vec<u32>,
    /// Index into the model's PVS texture list per sampler.
    pub items: Vec<u32>,
    /// Pixel-shader float constants set for the draw (`0x06` records: word 1 = start register << 16 | count, then `count` vec4s),
    /// by register. A road draw sets c0..c3 (`c_constant0..3`) and c5.
    pub constants: std::collections::BTreeMap<u32, [f32; 4]>,
}

impl Draw {
    /// The 16-bit normalised texture coordinate pair at byte `offset` of every vertex (signed / 32767, so -1..1).
    pub fn uv_at(&self, offset: usize) -> Vec<[f32; 2]> {
        if offset + 4 > self.stride {
            return vec![[0.0; 2]; self.positions.len()];
        }
        (0..self.positions.len())
            .map(|i| {
                let v = &self.vertices[i * self.stride..(i + 1) * self.stride];
                let h = |k: usize| i16::from_be_bytes([v[offset + k], v[offset + k + 1]]) as f32 / 32767.0;
                [h(0), h(2)]
            })
            .collect()
    }

    /// The same pair read as **unorm** `value / 65535` (0..1): FM2 stores the lightmap uv that way (the mask / colour uv
    /// is signed-normalised, see [`Self::uv_at`]; its values read as -1..1 while the lightmap's read as 0..1 only as unorm).
    pub fn uv_unorm(&self, offset: usize) -> Vec<[f32; 2]> {
        if offset + 4 > self.stride {
            return vec![[0.0; 2]; self.positions.len()];
        }
        (0..self.positions.len())
            .map(|i| {
                let v = &self.vertices[i * self.stride..(i + 1) * self.stride];
                let h = |k: usize| u16::from_be_bytes([v[offset + k], v[offset + k + 1]]) as f32 / 65535.0;
                [h(0), h(2)]
            })
            .collect()
    }

    /// Offset of the lightmap uv pair (unorm, see [`Self::uv_unorm`]) of a draw that binds a lightmap tile, `uv0` being the
    /// diffuse / mask pair. Layouts seen: stride 20 = position, uv0, lightmap; 24 = position, normal, uv0, lightmap (and the
    /// terrain's position, uv0, ?, lightmap); both end with it. Longer vertices (28+) have more sets, so the pair (a whole word,
    /// not `uv0`) whose uv distance follows the edge length best is taken. None when nothing fits.
    pub fn lightmap_offset(&self, uv0: usize) -> Option<usize> {
        let last = self.stride.checked_sub(4)?;
        if last < 12 {
            return None;
        }
        if matches!(self.stride, 20 | 24) {
            return (last != uv0).then_some(last);
        }
        let mut best: Option<(f32, usize)> = None;
        for o in (12..=last).step_by(4).filter(|&o| o != uv0) {
            let uv = self.uv_unorm(o);
            let mut r = Vec::new();
            for t in self.indices.chunks_exact(3).take(2000) {
                for (a, b) in [(0, 1), (1, 2)] {
                    let (a, b) = (t[a] as usize, t[b] as usize);
                    let dp = dist3(self.positions[a], self.positions[b]);
                    let du = ((uv[a][0] - uv[b][0]).powi(2) + (uv[a][1] - uv[b][1]).powi(2)).sqrt();
                    if dp > 1e-3 {
                        r.push(du / dp);
                    }
                }
            }
            if r.len() < 8 {
                continue;
            }
            let m = r.iter().sum::<f32>() / r.len() as f32;
            let cv = (r.iter().map(|x| (x - m).powi(2)).sum::<f32>() / r.len() as f32).sqrt() / m.max(1e-9);
            if cv < 0.6 && best.is_none_or(|b| cv < b.0) {
                best = Some((cv, o));
            }
        }
        best.map(|b| b.1)
    }

    /// Offsets of the (diffuse, lightmap) texture coordinate pairs. HEURISTIC: among the pairs whose uv distance follows the
    /// edge length (low spread), the one with the higher texel density is the tiling diffuse set, the other the lightmap.
    pub fn uv_offsets(&self) -> (usize, Option<usize>) {
        let mut scored = Vec::new();
        let mut o = 12;
        while o + 4 <= self.stride {
            let uv = self.uv_at(o);
            let mut r = Vec::new();
            for t in self.indices.chunks_exact(3).take(2000) {
                for (a, b) in [(0, 1), (1, 2)] {
                    let (a, b) = (t[a] as usize, t[b] as usize);
                    let dp = dist3(self.positions[a], self.positions[b]);
                    let du = ((uv[a][0] - uv[b][0]).powi(2) + (uv[a][1] - uv[b][1]).powi(2)).sqrt();
                    if dp > 1e-3 {
                        r.push(du / dp);
                    }
                }
            }
            if r.len() >= 8 {
                let m = r.iter().sum::<f32>() / r.len() as f32;
                let sd = (r.iter().map(|x| (x - m).powi(2)).sum::<f32>() / r.len() as f32).sqrt();
                if m > 0.0 {
                    scored.push((o, sd / m, m));
                }
            }
            // The pairs are whole 32-bit words (a 2-byte shift straddles two pairs and reads garbage).
            o += 4;
        }
        let mut good: Vec<_> = scored.iter().filter(|s| s.1 < 0.5).collect();
        good.sort_by(|a, b| b.2.total_cmp(&a.2));
        // Overlapping candidates (a pair shifted by 2 bytes) are not separate sets.
        let first = good.first().map(|s| s.0).unwrap_or(12);
        let second = good.iter().map(|s| s.0).find(|&o| o.abs_diff(first) >= 4);
        (first, second)
    }
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[derive(Debug, Default)]
pub struct Model {
    pub draws: Vec<Draw>,
    /// Draw records that could not be resolved to buffers (reason per draw).
    pub skipped: Vec<String>,
}

fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}

/// Parses an `.rmb.bin` model into its draws.
pub fn parse_model(d: &[u8]) -> Result<Model> {
    if d.get(0..4) != Some(b"CAFF") {
        return Err(Error::BadMagic("FM2 model"));
    }
    // A model without buffers (an empty placeholder) has no .gpu section.
    let Some(gsz) = section(d, b".gpu") else { return Ok(Model::default()) };
    let dsz = section(d, b".data").ok_or(Error::Truncated("fm2 .data"))?;
    let ssz = section(d, b".stream").ok_or(Error::Truncated("fm2 .stream"))?;
    let (g0, d0) = (0x190, 0x190 + gsz);
    let s0 = d0 + dsz;
    let gpu = d.get(g0..d0).ok_or(Error::Truncated("fm2 gpu"))?;
    let data = d.get(d0..s0).ok_or(Error::Truncated("fm2 data"))?;
    let stream = d.get(s0..s0 + ssz).ok_or(Error::Truncated("fm2 stream"))?;
    let start = be(stream, 0).unwrap_or(0x24) as usize;
    let mut model = Model::default();
    let (mut vb, mut items): (Option<usize>, Vec<u32>) = (None, Vec::new());
    let mut constants: std::collections::BTreeMap<u32, [f32; 4]> = Default::default();
    let mut o = start;
    while o + 4 <= stream.len() {
        let (len, ty) = (stream[o + 1] as usize, stream[o + 2]);
        if len < 4 || o + len > stream.len() {
            break;
        }
        let w = |k: usize| be(stream, o + 4 + 4 * k);
        match ty {
            // Draws after the first of a vertex buffer reuse its fetch constants (no new record).
            0x2e => vb = w(1).map(|v| v as usize),
            0x2d => {
                // A draw can carry several of these; the first one binds its pixel-shader samplers (a road draw: `[12, 13, 0, 1,
                // 2, 3]` = two system textures (the shadow maps, items past the model's list) then lightmap + three colours), a
                // later one re-binds slot 0 (`[0]`): keeping the last one made every road a lightmap-only draw.
                if items.is_empty() {
                    let n = (w(1).unwrap_or(0) >> 16) as usize + (w(1).unwrap_or(0) & 0xffff) as usize;
                    // The pairs (item, slot << 16 | slot) start at word 2.
                    items = (0..n).filter_map(|k| w(2 + 2 * k)).collect();
                }
            }
            0x06 => {
                let (start, count) = (w(1).unwrap_or(0) >> 16, (w(1).unwrap_or(0) & 0xffff) as usize);
                for k in 0..count {
                    let f = |j: usize| w(2 + 4 * k + j).map(f32::from_bits);
                    if let (Some(a), Some(b), Some(c), Some(d)) = (f(0), f(1), f(2), f(3)) {
                        constants.insert(start + k as u32, [a, b, c, d]);
                    }
                }
            }
            0x01 => {
                let (count, key) = (w(1).unwrap_or(0) as usize, w(2).unwrap_or(0));
                if let (Some(vo), true) = (vb, count > 0 && w(0) == Some(4)) {
                    match build_draw(gpu, data, vo, count, key, std::mem::take(&mut items), constants.clone()) {
                        Ok(draw) => model.draws.push(draw),
                        Err(why) => model.skipped.push(why),
                    }
                } else {
                    model.skipped.push(format!("draw without a vertex fetch (prim {:?}, count {count})", w(0)));
                }
            }
            _ => {}
        }
        o += len;
    }
    Ok(model)
}

fn build_draw(gpu: &[u8], data: &[u8], vb: usize, count: usize, key: u32, items: Vec<u32>, constants: std::collections::BTreeMap<u32, [f32; 4]>) -> std::result::Result<Draw, String> {
    let (stride, goff, size) = match (be(data, vb), be(data, vb + 8), be(data, vb + 12)) {
        (Some(a), Some(b), Some(c)) => (a as usize, b as usize, c as usize),
        _ => return Err("vertex record outside .data".into()),
    };
    if stride < 12 || size % stride != 0 {
        return Err(format!("vertex record stride {stride} size {size}"));
    }
    let n = size / stride;
    let vertices = gpu.get(goff..goff + size).ok_or("vertices outside .gpu")?.to_vec();
    // The index buffer record [key, gpu offset, bytes, 1].
    let ib = (0..data.len().saturating_sub(16))
        .step_by(4)
        .find(|&o| be(data, o) == Some(key) && be(data, o + 8) == Some(count as u32 * 2) && be(data, o + 12) == Some(1))
        .ok_or_else(|| format!("no index buffer record for key {key:#x} count {count}"))?;
    let ioff = be(data, ib + 4).unwrap_or(0) as usize;
    let raw = gpu.get(ioff..ioff + count * 2).ok_or("indices outside .gpu")?;
    let indices: Vec<u32> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]]) as u32).collect();
    if indices.iter().any(|&i| i as usize >= n) {
        return Err(format!("indices past the {n} vertices"));
    }
    let positions = (0..n)
        .map(|i| {
            let f = |k: usize| f32::from_be_bytes(vertices[i * stride + 4 * k..i * stride + 4 * k + 4].try_into().unwrap());
            [f(0), f(1), f(2)]
        })
        .collect();
    Ok(Draw { stride, positions, vertices, indices, items, constants })
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_garbage() {
        assert!(super::parse_pvs(b"nope").is_err());
        assert!(super::parse_model(b"nope").is_err());
    }
}
