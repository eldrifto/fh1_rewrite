//! FM6 (Xbox One) `.modelbin` meshes: a `burG` container (see [`crate::grub`]).
//!
//! Chunks used here (read from the IDx files and checked on all of them, docs/DESIGN.md "FM6 IDx"):
//! - `IndB`, `VerB`: 16-byte header (u32 count, u32 bytes, u16 stride, u16 component count, u32 DXGI format)
//!   then the data. The buffer id is the i32 at metadata +8. Indices are u32 (format 42) or u16.
//!   `VerB` id -1 is the shared position stream: R16G16B16A16_SNORM (format 13), one entry per vertex of the
//!   whole file; the other `VerB`s hold the rest of each vertex for one range of it.
//! - `VLay`: vertex layout with the same id as its `VerB`: u16 name count, names (u32 length + text), u16
//!   element count, 20-byte elements (u16 name, u16 index, u32 stream slot, u32 DXGI format, i32 offset, u32 class).
//! - `Mesh`: u16 material slot (0xFFFF = merged shadow-only copy) @0, u16 LOD bit mask @4, u8 LOD @6;
//!   u32 first index, i32 base vertex, u32 index count, u32 triangle count @25; a binding list
//!   (u32 count, then per stream: i32 buffer id, u32, u32 stride, u32 byte offset); and at the end
//!   five 4-float texture-coordinate rects, the position scale and the position offset (each 4 floats).
//!   The metadata holds the name and the mesh's bounding box (last 24 bytes).
//!
//! Vertex `v` of a mesh is `index + base vertex`; each stream is read at `offset + v * stride`.
//! Position = snorm(xyz) * scale + offset (checked: equals the stored box on every LOD0 mesh of the IDx).
//! Normal = normalize(position.w, normal.x, normal.y) (checked: agrees with the triangle normals, mean cosine
//! 0.98 to 0.99, so the winding is counter-clockwise for `cross(b - a, c - a)` pointing out).
//! Texture coordinates are R16G16_UNORM and returned raw (0..1); how the five rects map onto the four sets
//! is NOT verified.
//!
//! FM5 (Mesh version 0x20701, container version 1) differs only here: positions are three floats (DXGI 6, no scale /
//! offset), normals three half floats (DXGI 10), and the mesh record ends with the five uv rects (185 bytes on the E21).
//!
//! Morphs: `Mrph` lists the morph targets (u16 count, u32-length names: the wheel has `weight[0]`, `weight[1]`).
//! `MBuf` (same 16-byte header, same id as the attribute `VerB`, DXGI 10 = four half floats per component) holds
//! per vertex the position change of each target, then the normal change of each target. A buffer with fewer
//! targets than `Mrph` is taken to hold the last ones (seen once: the wheel's blur meshes carry only the radius
//! target, recognised by its values; NOT a rule read from the game). On the IDx wheel: `weight[1]` moves every
//! vertex out from the axle by 1.3182 x its distance (0.14 m rim at 0, 0.324 m at 1), `weight[0]` moves the
//! barrel's inner edge (x = 1.0) along the axle by -0.9.

use crate::{grub, Error};

#[derive(Debug, Clone)]
pub struct Mesh {
    pub name: String,
    /// Material slot of the part (index into [`material_instances`]); `0xFFFF` = shadow-only copy.
    pub material: u16,
    /// Skeleton bone the positions are relative to (u16 @2; 1 = `root` for parts already in car space).
    pub bone: u16,
    pub lod: u8,
    pub lod_mask: u16,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Raw texture coordinates per set (TEXCOORD index, values), before any rect is applied.
    pub uvs: Vec<(u16, Vec<[f32; 2]>)>,
    pub uv_rects: [[f32; 4]; 5],
    /// Triangles as indices into `positions`.
    pub indices: Vec<u32>,
    /// Stored box: min xyz, max xyz.
    pub bbox: Option<[f32; 6]>,
    /// Position change per morph target (one list per `Mrph` target, empty when the mesh has none for it).
    pub morphs: Vec<Vec<[f32; 3]>>,
}

fn half(d: &[u8], o: usize) -> f32 {
    let h = u16::from_le_bytes([d[o], d[o + 1]]) as u32;
    let (s, e, m) = ((h >> 15) << 31, (h >> 10) & 31, h & 1023);
    let bits = match e {
        0 if m == 0 => s,
        0 => return if s != 0 { -1.0 } else { 1.0 } * (m as f32) * 2f32.powi(-24),
        31 => s | 0x7f80_0000 | (m << 13),
        _ => s | ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

struct Buffer<'a> {
    data: &'a [u8],
    /// DXGI format from the buffer header (the position stream: 13 = FM6 packed, 6 = FM5 float).
    format: u32,
}

struct Element {
    name: String,
    index: u16,
    slot: u32,
    format: u32,
}

fn rd_u16(d: &[u8], o: usize) -> Result<u16, Error> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or(Error::Truncated("modelbin"))
}
fn rd_u32(d: &[u8], o: usize) -> Result<u32, Error> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or(Error::Truncated("modelbin"))
}
fn rd_f32(d: &[u8], o: usize) -> Result<f32, Error> {
    rd_u32(d, o).map(f32::from_bits)
}

fn format_bytes(f: u32) -> Option<usize> {
    match f {
        6 => Some(12),
        13 | 10 => Some(8),
        37 | 35 | 24 => Some(4),
        _ => None,
    }
}

fn parse_layout(b: &[u8]) -> Result<Vec<Element>, Error> {
    let mut o = 2;
    let mut names = Vec::new();
    for _ in 0..rd_u16(b, 0)? {
        let l = rd_u32(b, o)? as usize;
        names.push(String::from_utf8_lossy(b.get(o + 4..o + 4 + l).ok_or(Error::Truncated("VLay name"))?).into_owned());
        o += 4 + l;
    }
    let n = rd_u16(b, o)?;
    o += 2;
    let mut els = Vec::new();
    for _ in 0..n {
        let ni = rd_u16(b, o)? as usize;
        els.push(Element {
            name: names.get(ni).cloned().unwrap_or_default(),
            index: rd_u16(b, o + 2)?,
            slot: rd_u32(b, o + 4)?,
            format: rd_u32(b, o + 8)?,
        });
        o += 20;
    }
    Ok(els)
}

fn snorm16(d: &[u8], o: usize) -> f32 {
    (i16::from_le_bytes([d[o], d[o + 1]]) as f32 / 32767.0).max(-1.0)
}

pub fn parse(file: &[u8]) -> Result<Vec<Mesh>, Error> {
    let g = grub::parse(file)?;
    let id_of = |c: &grub::Chunk| -> Result<i32, Error> { Ok(rd_u32(g.meta_of(c)?, 8)? as i32) };
    let mut verts = std::collections::HashMap::new();
    let mut layouts = std::collections::HashMap::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut morph_bufs = std::collections::HashMap::new(); // id -> (data, components)
    let mut n_targets = 0usize;
    for c in &g.chunks {
        let d = g.data_of(c)?;
        match c.tag.as_str() {
            "MBuf" => {
                let (bytes, comps) = (rd_u32(d, 4)? as usize, rd_u16(d, 10)? as usize);
                morph_bufs.insert(id_of(c)?, (d.get(16..16 + bytes).ok_or(Error::Truncated("MBuf"))?, comps));
            }
            "Mrph" => n_targets = rd_u16(d, 0)? as usize,
            "VerB" | "IndB" => {
                let (count, bytes, stride) = (rd_u32(d, 0)? as usize, rd_u32(d, 4)? as usize, rd_u16(d, 8)? as usize); // stride: index width
                let data = d.get(16..16 + bytes).ok_or(Error::Truncated("modelbin buffer"))?;
                if c.tag == "IndB" {
                    indices = match stride {
                        4 => data.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect(),
                        2 => data.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32).collect(),
                        s => return Err(Error::Unsupported(format!("index stride {s}"))),
                    };
                    if indices.len() != count {
                        return Err(Error::SizeMismatch { expected: count, got: indices.len() });
                    }
                } else {
                    verts.insert(id_of(c)?, Buffer { data, format: rd_u32(d, 12)? });
                }
            }
            "VLay" => {
                layouts.insert(id_of(c)?, parse_layout(d)?);
            }
            _ => {}
        }
    }
    if !g.chunks.iter().any(|c| c.tag == "Mesh") {
        return Ok(Vec::new()); // e.g. the skeleton file
    }
    let pos = verts.get(&-1).ok_or(Error::Unsupported("modelbin without a position stream".into()))?;
    let mut meshes = Vec::new();
    for c in g.chunks.iter().filter(|c| c.tag == "Mesh") {
        let r = g.data_of(c)?;
        let meta = g.meta_of(c)?;
        let material = rd_u16(r, 0)?;
        let bone = rd_u16(r, 2)?;
        let lod_mask = rd_u16(r, 4)?;
        let lod = r[6];
        let (start, base, icount) = (rd_u32(r, 25)? as usize, rd_u32(r, 29)? as i32 as i64, rd_u32(r, 33)? as usize);
        // binding list: the first u32 in 45..70 that is a small count followed by buffer id -1
        let bo = (45..70)
            .find(|&i| matches!(rd_u32(r, i), Ok(1..=4)) && rd_u32(r, i + 4).ok() == Some(u32::MAX))
            .ok_or(Error::Unsupported("mesh binding list not found".into()))?;
        let mut binds = Vec::new();
        for k in 0..rd_u32(r, bo)? as usize {
            let o = bo + 4 + 16 * k;
            binds.push((rd_u32(r, o)? as i32, rd_u32(r, o + 8)? as usize, rd_u32(r, o + 12)? as usize));
        }
        let end = r.len();
        let f4 = |o: usize| -> Result<[f32; 4], Error> { Ok([rd_f32(r, o)?, rd_f32(r, o + 4)?, rd_f32(r, o + 8)?, rd_f32(r, o + 12)?]) };
        // FM6 (packed positions): five uv rects, then scale and offset; FM5 (float positions): the five rects end the record
        let float_pos = pos.format == 6;
        let (offset, scale) = if float_pos { ([0.0; 4], [1.0; 4]) } else { (f4(end - 16)?, f4(end - 32)?) };
        let rects_at = if float_pos { end - 80 } else { end - 112 };
        let mut uv_rects = [[0.0; 4]; 5];
        for (k, rect) in uv_rects.iter_mut().enumerate() {
            *rect = f4(rects_at + 16 * k)?;
        }
        let tri = indices.get(start..start + icount).ok_or(Error::Truncated("mesh index range"))?;
        // compact the used vertices
        let mut local: Vec<usize> = tri.iter().map(|&i| (i as i64 + base) as usize).collect();
        let mut used = local.clone();
        used.sort_unstable();
        used.dedup();
        for v in local.iter_mut() {
            *v = used.binary_search(v).unwrap();
        }
        let (_, pstride, poff) = *binds.iter().find(|b| b.0 == -1).ok_or(Error::Unsupported("mesh without position binding".into()))?;
        let mut positions = Vec::with_capacity(used.len());
        let mut normals = Vec::with_capacity(used.len());
        let mut wcomp = Vec::with_capacity(used.len());
        for &v in &used {
            let o = poff + v * pstride;
            if o + if float_pos { 12 } else { 8 } > pos.data.len() {
                return Err(Error::Truncated("position stream"));
            }
            if float_pos {
                positions.push([0, 1, 2].map(|k| f32::from_le_bytes(pos.data[o + 4 * k..o + 4 * k + 4].try_into().unwrap())));
                wcomp.push(0.0);
            } else {
                positions.push([0, 1, 2].map(|k| snorm16(pos.data, o + 2 * k) * scale[k] + offset[k]));
                wcomp.push(snorm16(pos.data, o + 6));
            }
        }
        let mut uvs: Vec<(u16, Vec<[f32; 2]>)> = Vec::new();
        let mut morphs = vec![Vec::new(); n_targets];
        if let Some(&(bid, stride, off)) = binds.iter().find(|b| b.0 != -1) {
            if let Some(&(mdata, comps)) = morph_bufs.get(&bid) {
                let mstride = comps * 8;
                let in_buf = comps / 2;
                for k in 0..in_buf.min(n_targets) {
                    let target = n_targets - in_buf + k;
                    let row0 = off / stride;
                    morphs[target] = used
                        .iter()
                        .map(|&v| {
                            let o = (row0 + v) * mstride + k * 8;
                            if o + 6 <= mdata.len() { [half(mdata, o), half(mdata, o + 2), half(mdata, o + 4)] } else { [0.0; 3] }
                        })
                        .collect();
                }
            }
            let buf = verts.get(&bid).ok_or(Error::Unsupported(format!("vertex buffer {bid} missing")))?;
            let slot1_stride = |els: &Vec<Element>| els.iter().filter(|e| e.slot == 1).map(|e| format_bytes(e.format).unwrap_or(0)).sum::<usize>();
            let els = layouts
                .get(&bid)
                .filter(|l| slot1_stride(l) == stride)
                .or_else(|| layouts.values().find(|l| slot1_stride(l) == stride))
                .ok_or(Error::Unsupported(format!("no vertex layout with stride {stride}")))?;
            let mut eo = 0;
            for e in els.iter().filter(|e| e.slot == 1) {
                let nb = format_bytes(e.format).ok_or(Error::Unsupported(format!("vertex format {}", e.format)))?;
                let at = |v: usize| off + v * stride + eo;
                match (e.name.as_str(), e.format) {
                    ("NORMAL", 10) => {
                        // FM5: three half floats (and a fourth, unused)
                        for &v in &used {
                            let n = [0, 1, 2].map(|k| half(buf.data, at(v) + 2 * k));
                            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                            normals.push(n.map(|c| c / l));
                        }
                    }
                    ("NORMAL", 37) => {
                        for (k, &v) in used.iter().enumerate() {
                            let (nx, ny) = (snorm16(buf.data, at(v)), snorm16(buf.data, at(v) + 2));
                            let n = [wcomp[k], nx, ny];
                            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                            normals.push(n.map(|c| c / l));
                        }
                    }
                    ("TEXCOORD", 35) => {
                        let set = used
                            .iter()
                            .map(|&v| {
                                let o = at(v);
                                [u16::from_le_bytes([buf.data[o], buf.data[o + 1]]) as f32 / 65535.0, u16::from_le_bytes([buf.data[o + 2], buf.data[o + 3]]) as f32 / 65535.0]
                            })
                            .collect();
                        uvs.push((e.index, set));
                    }
                    _ => {}
                }
                eo += nb;
            }
        }
        let name_end = meta.len().saturating_sub(24);
        let name = meta.get(16..name_end).map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').to_string()).unwrap_or_default();
        let bbox = if meta.len() >= 40 {
            let mut b = [0.0; 6];
            for (k, x) in b.iter_mut().enumerate() {
                *x = rd_f32(meta, meta.len() - 24 + 4 * k)?;
            }
            Some(b)
        } else {
            None
        };
        meshes.push(Mesh { name, material, bone, lod, lod_mask, positions, normals, uvs, uv_rects, indices: local.into_iter().map(|v| v as u32).collect(), bbox, morphs });
    }
    Ok(meshes)
}

/// One skeleton bone. `local` is row-major with the translation in the last row (`v' = v * M`);
/// `world` is `local * parent.world`.
#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub parent: i16,
    pub local: [[f32; 4]; 4],
    pub world: [[f32; 4]; 4],
}

fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

impl Bone {
    /// A point in this bone's space moved into car space.
    pub fn to_car(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.world;
        [0, 1, 2].map(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j])
    }
    /// A direction (normal) moved into car space (rotation only; not renormalised).
    pub fn dir_to_car(&self, d: [f32; 3]) -> [f32; 3] {
        let m = &self.world;
        [0, 1, 2].map(|j| d[0] * m[0][j] + d[1] * m[1][j] + d[2] * m[2][j])
    }
}

/// The skeleton (`Skel` chunk): u16 bone count, then per bone a u32-length name, i16 parent, two i16
/// (child / sibling links, not used here) and a 4x4 float matrix. Every IDx modelbin carries the same 29 bones.
pub fn parse_skeleton(file: &[u8]) -> Result<Vec<Bone>, Error> {
    let g = grub::parse(file)?;
    let c = g.chunks.iter().find(|c| c.tag == "Skel").ok_or(Error::BadMagic("modelbin without Skel"))?;
    let b = g.data_of(c)?;
    let mut o = 2;
    let mut bones: Vec<Bone> = Vec::new();
    for _ in 0..rd_u16(b, 0)? {
        let l = rd_u32(b, o)? as usize;
        let name = String::from_utf8_lossy(b.get(o + 4..o + 4 + l).ok_or(Error::Truncated("Skel name"))?).into_owned();
        o += 4 + l;
        let parent = rd_u16(b, o)? as i16;
        o += 6;
        let mut local = [[0.0; 4]; 4];
        for (i, row) in local.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                *x = rd_f32(b, o + 4 * (4 * i + j))?;
            }
        }
        o += 64;
        let world = match bones.get(parent as usize) {
            Some(p) if parent >= 0 => mat_mul(&local, &p.world),
            _ => local,
        };
        bones.push(Bone { name, parent, local, world });
    }
    Ok(bones)
}

/// The material instances (`MatI` chunks) in file order; a mesh's `material` indexes this list.
/// Each is an embedded container for [`crate::materialbin::parse`].
pub fn material_instances(file: &[u8]) -> Result<Vec<&[u8]>, Error> {
    let g = grub::parse(file)?;
    g.chunks.iter().filter(|c| c.tag == "MatI").map(|c| g.data_of(c)).collect()
}
