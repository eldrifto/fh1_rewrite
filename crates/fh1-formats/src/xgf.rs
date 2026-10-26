//! Forza Motorsport 2 UI scenes (`*.xgf`, `media/UI.zip/Scenes`): Anark Gameface presentations compiled to a
//! binary XML tree. All words are big-endian u32.
//!
//! Layout (checked on `I_HUD_All_Divided_Common` and `..._PartOne`):
//! - 32-byte header: `[string_end - 32, version?, ?, ?, node_count, attr_count, 0, 0]`.
//! - `node_count` records of 4 words: `[first_child, next_sibling, tag, first_attr]`. `0xFFFF_FFFF` = none;
//!   `tag` is an offset into the string table; `first_child` is always the next record when present.
//! - attribute triples `[name, value, next]`: name and value are string-table offsets, `next` is the 1-based
//!   index of the next triple of the same node (`0xFFFF_FFFF` ends the list). `first_attr` is 1-based too.
//!   The triple area starts one word after the node records.
//! - the string table (NUL separated, starts with `presentation`), then the model data: the same mesh blob as FH1's `.fbf` files
//!   (`fh1_ui::anark::fbf`): header words 1..3 are `[mesh_count, group_table_bytes, data_bytes]`, the blob is `mesh_count` 96-byte
//!   descriptors (`[n_verts, n_groups, ?, ?, bbox min xyz, pad, bbox max xyz, pad, ..., vertex fetch constant at 18..20, first group * 36
//!   at 20]`), the 36-byte group table, then the buffers (32-byte vertices `pos, normal, uv`; u16 BE triangle indices per group). It starts
//!   16 bytes before `32 + header[0]` (the first descriptor's first four words sit in the string table's padding). A `modelbuffer`
//!   node's `modeldata` attribute is the mesh index. Checked: every buffer is accounted for on the HUD scenes.
//!
//! Every attribute value is text (`"0 -79.5 5"`, `"long:1"`, `"string:Scene.ActivePlayer"`).

use std::collections::HashMap;

use crate::Error;

const NONE: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Whitespace separated floats ("0 -79.5 5"); missing or malformed -> `None`.
    pub fn floats(&self, name: &str) -> Option<Vec<f32>> {
        self.attr(name)?.split_whitespace().map(|t| t.parse().ok()).collect()
    }
}

/// One 32-byte vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub vertices: Vec<Vertex>,
    /// Triangle-list indices per index group.
    pub groups: Vec<Vec<u16>>,
}

#[derive(Debug, Clone)]
pub struct Xgf {
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>,
    /// Header node / attribute counts, kept so tests can check the decode is complete.
    pub node_count: usize,
    pub attr_count: usize,
}

fn be(d: &[u8], word: usize) -> Option<u32> {
    let o = word * 4;
    d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Xgf {
    pub fn parse(d: &[u8]) -> Result<Xgf, Error> {
        if d.len() < 32 {
            return Err(Error::Truncated("xgf header"));
        }
        let find = |needle: &[u8]| d.windows(needle.len()).position(|w| w == needle);
        let sb = find(b"presentation\0").ok_or(Error::BadMagic("xgf: no string table"))?;
        let string_end = (be(d, 0).unwrap() as usize + 32).min(d.len());
        let node_count = be(d, 4).unwrap() as usize;
        let attr_count = be(d, 5).unwrap() as usize;
        let table = d.get(sb..string_end).ok_or(Error::Truncated("xgf strings"))?;
        let mut strs: HashMap<u32, &str> = HashMap::new();
        let mut pos = 0usize;
        while pos < table.len() {
            let end = table[pos..].iter().position(|&b| b == 0).map_or(table.len(), |e| pos + e);
            strs.insert(pos as u32, std::str::from_utf8(&table[pos..end]).unwrap_or(""));
            pos = end + 1;
        }
        let words = (sb / 4).saturating_sub(8);
        let w = |i: usize| be(d, 8 + i);
        let attr_base = node_count * 4 + 1;
        if attr_base > words {
            return Err(Error::Truncated("xgf node records"));
        }
        let s = |off: u32| strs.get(&off).copied().unwrap_or("");
        let mut nodes = Vec::with_capacity(node_count);
        let mut links = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let (child, sib, tag, first) = (w(i * 4).ok_or(Error::Truncated("xgf node"))?, w(i * 4 + 1).unwrap(), w(i * 4 + 2).unwrap(), w(i * 4 + 3).unwrap());
            let mut attrs = Vec::new();
            let mut a = first;
            let mut guard = 0;
            while a != NONE && a != 0 && guard < 4096 {
                let p = attr_base + (a as usize - 1) * 3;
                let (Some(n), Some(v), Some(nx)) = (w(p), w(p + 1), w(p + 2)) else { break };
                attrs.push((s(n).to_owned(), s(v).to_owned()));
                a = nx;
                guard += 1;
            }
            links.push((child, sib));
            nodes.push(Node { tag: s(tag).to_owned(), attrs, children: Vec::new(), parent: None });
        }
        // Build children from first_child + next_sibling chains.
        for i in 0..node_count {
            let mut c = links[i].0;
            let mut guard = 0;
            while c != NONE && (c as usize) < node_count && guard < node_count {
                nodes[i].children.push(c as usize);
                nodes[c as usize].parent = Some(i);
                c = links[c as usize].1;
                guard += 1;
            }
        }
        let meshes = parse_meshes(d, string_end).unwrap_or_default();
        Ok(Xgf { nodes, meshes, node_count, attr_count })
    }

    pub fn root(&self) -> usize {
        0
    }

    /// Node index by its `xmlid` attribute (library entries and instances carry one).
    pub fn by_xmlid(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.attr("xmlid") == Some(id))
    }

    /// Depth-first walk from `from`: `f(index, depth)`.
    pub fn walk(&self, from: usize, f: &mut impl FnMut(usize, usize)) {
        fn rec(x: &Xgf, i: usize, d: usize, f: &mut impl FnMut(usize, usize)) {
            f(i, d);
            for &c in &x.nodes[i].children {
                rec(x, c, d + 1, f);
            }
        }
        rec(self, from, 0, f);
    }
}

fn parse_meshes(d: &[u8], string_end: usize) -> Option<Vec<Mesh>> {
    let [n_mesh, gt_bytes, data_bytes] = [be(d, 1)? as usize, be(d, 2)? as usize, be(d, 3)? as usize];
    let start = string_end.checked_sub(16)?;
    let desc_at = start;
    let gt_at = desc_at + n_mesh * 96;
    let blob_at = gt_at + gt_bytes;
    let blob = d.get(blob_at..blob_at + data_bytes)?;
    let w = |o: usize| -> Option<u32> { d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])) };
    let mut meshes = Vec::with_capacity(n_mesh);
    for m in 0..n_mesh {
        let dw = |i: usize| w(desc_at + m * 96 + i * 4);
        let n_verts = dw(0)? as usize;
        let n_groups = dw(1)? as usize;
        let voff = (dw(18)? & !3) as usize;
        let vbytes = ((dw(19)? >> 2) & 0xFF_FFFF) as usize * 4;
        if n_verts == 0 || vbytes != 32 * n_verts {
            return None;
        }
        let vb = blob.get(voff..voff + vbytes)?;
        let vertices = vb
            .chunks_exact(32)
            .map(|c| {
                let g = |i: usize| f32::from_be_bytes([c[4 * i], c[4 * i + 1], c[4 * i + 2], c[4 * i + 3]]);
                Vertex { pos: [g(0), g(1), g(2)], normal: [g(3), g(4), g(5)], uv: [g(6), g(7)] }
            })
            .collect();
        let g0 = dw(20)? as usize / 36;
        let mut groups = Vec::new();
        for gi in g0..g0 + n_groups {
            let e = gt_at + gi * 36;
            let (n_idx, ioff, ibytes) = (w(e)? as usize, w(e + 28)? as usize, w(e + 32)? as usize);
            if ibytes != 2 * n_idx {
                return None;
            }
            let ib = blob.get(ioff..ioff + ibytes)?;
            groups.push(ib.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect());
        }
        // Descriptor words 4..6 / 8..10 (the descriptor's first four words are the ones before `string_end`).
        let lo = [f32::from_bits(dw(4)?), f32::from_bits(dw(5)?), f32::from_bits(dw(6)?)];
        let hi = [f32::from_bits(dw(8)?), f32::from_bits(dw(9)?), f32::from_bits(dw(10)?)];
        meshes.push(Mesh { bbox_min: lo, bbox_max: hi, vertices, groups });
    }
    Some(meshes)
}
