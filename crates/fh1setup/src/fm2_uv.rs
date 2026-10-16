//! FM2 per-draw texture coordinates from the draw's own shaders (`fh1_shaders::fm2`): the vertex shader turns the 16-bit integer pair of the
//! vertex into the interpolator the pixel shader's `tfetch` reads, either `pair * literal` or through the material's placement matrix
//! (`c_place2d_N_Mtx`, the parameter block in the model's `.data`). This replaces the constant x32 the importer used to apply to every draw.
//! Samplers whose coordinate the pixel shader computes (lightmap atlas transforms) are not resolved here.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use fh1_formats::fm2;
use fh1_shaders::fm2 as sh;

fn be(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn section(d: &[u8], name: &[u8]) -> Option<usize> {
    let pos = d.get(..0x190)?.windows(name.len() + 1).position(|w| &w[..name.len()] == name && w[name.len()] == 0)?;
    Some(be(d, pos + 16)? as usize)
}

/// Why draws did or did not resolve (`FM2_UV_STATS=1` prints it at the end of a track): no shader pairing, no direct sampler, direct but the vertex
/// shader's output is not a plain scale / matrix, matrix without its values, resolved.
pub static STATS: [AtomicUsize; 6] = [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];

/// Per draw of `model`: item position -> the texture coordinate of every vertex (an empty list marks a texture bound to a cube sampler). A draw whose shaders could not be paired is empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    /// An item past the model's texture list (the shadow maps).
    System,
    /// A lightmap atlas tile (PVS flags 8 / 9 / 12).
    Tile,
    Other,
}

/// What `resolve` returns per draw.
#[derive(Default, Clone)]
pub struct DrawUv {
    /// Item position -> the texture coordinate of every vertex (an empty list marks a cube sampler).
    pub slots: BTreeMap<usize, Vec<[f32; 2]>>,
    /// The pixel shader's output alpha is a constant.
    pub alpha_const: bool,
    /// Item position -> the draw constant register holding the atlas sub-rectangle `[su, sv, ou, ov]` the shader maps its coordinate into
    /// (`frac(uv) * c.xy + c.zw`); the coordinate in `slots` is the unit coordinate before that transform.
    pub atlas: BTreeMap<usize, u32>,
}

pub fn resolve(raw_model: &[u8], model: &fm2::Model, kind: &dyn Fn(u32) -> ItemKind) -> Vec<DrawUv> {
    let empty = || vec![DrawUv::default(); model.draws.len()];
    let none = || {
        STATS[0].fetch_add(model.draws.len(), Relaxed);
        empty()
    };
    let (Some(g), Some(dt)) = (section(raw_model, b".gpu"), section(raw_model, b".data")) else { return none() };
    let (Some(gpu), Some(data)) = (raw_model.get(0x190..0x190 + g), raw_model.get(0x190 + g..0x190 + g + dt)) else { return none() };
    let shaders = sh::tail_shaders(gpu, model.tail_start);
    let groups = sh::data_groups(data);
    let pairs = sh::pair_groups(&groups, &shaders);
    // Placement matrix blocks are consumed in group order, then constant table order.
    let mats = sh::matrix_blocks(data);
    let mut next = 0;
    let mut group_mats: Vec<Vec<(u32, [[f32; 4]; 3])>> = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let mut v = Vec::new();
        // A group without a vertex container of its own (its vertex shader lives in the tail) has no constant table: the registers its vertex
        // shader reads matrices from, in ascending order, stand in for the container's `c_place2d_*` list.
        let regs: Vec<u32> = if !g.place2d.is_empty() {
            g.place2d.iter().map(|(_, r)| *r).collect()
        } else if let Some(vs) = pairs[gi].0 {
            let mut r: Vec<u32> = sh::vs_uv_sources(&shaders[vs]).values().filter_map(|u| if let sh::UvMap::Matrix { reg } = u.map { Some(reg) } else { None }).collect();
            r.sort_unstable();
            r.dedup();
            r
        } else {
            Vec::new()
        };
        for reg in &regs {
            if let Some(m) = mats.get(next) {
                v.push((*reg, *m));
            }
            next += 1;
        }
        group_mats.push(v);
    }
    // Identical-size pixel shaders: several groups can share a size while the tail holds fewer or different copies, so a draw can take the copy whose
    // sampler count matches its item list.
    let ps_samplers_all: Vec<Option<Vec<sh::PsSampler>>> = shaders.iter().map(|t| (!t.vertex).then(|| sh::ps_samplers(t))).collect();
    let resolved: Vec<Option<_>> = pairs.iter().map(|(v, p)| Some((sh::vs_uv_sources(&shaders[(*v)?]), sh::ps_samplers(&shaders[(*p)?])))).collect();
    model
        .draws
        .iter()
        .map(|d| {
            let mut out = BTreeMap::new();
            let mut atlas: BTreeMap<usize, u32> = BTreeMap::new();
            let Some(gi) = d.ps_container.and_then(|c| groups.iter().position(|g| g.ps == c)) else {
                STATS[0].fetch_add(1, Relaxed);
                if std::env::var("FM2_UV_STATS").is_ok_and(|v| v == "3") {
                    println!("[fm2] uv: no group for ps container {:x?} (groups at {:x?}), stride {}", d.ps_container, groups.iter().map(|g| g.ps).collect::<Vec<_>>(), d.stride);
                }
                return DrawUv { slots: out, ..Default::default() };
            };
            // A model can set a pixel shader container twice before a draw (the last one is then a depth / shadow variant): a draw that lists
            // more items than its shader samples belongs to the one group whose shader samples exactly that many.
            // The container set just before it is the real shader when it fits the item list.
            let gi = match d.ps_alt.and_then(|c| groups.iter().position(|g| g.ps == c)) {
                Some(alt) if resolved[gi].as_ref().is_some_and(|(_, s)| s.len() < d.items.len()) && resolved[alt].as_ref().is_some_and(|(_, s)| s.len() == d.items.len()) => alt,
                _ => gi,
            };
            let gi = if resolved[gi].as_ref().is_some_and(|(_, s)| s.len() < d.items.len()) {
                let fits: Vec<usize> = (0..resolved.len()).filter(|&g| resolved[g].as_ref().is_some_and(|(_, s)| s.len() == d.items.len())).collect();
                // The first container is the one set just before the depth variant: the group before it.
                if fits.len() == 1 { fits[0] } else if gi > 0 && fits.contains(&(gi - 1)) { gi - 1 } else { gi }
            } else {
                gi
            };
            let alpha_const = pairs[gi].1.is_some_and(|p| sh::ps_alpha_constant(&shaders[p]));
            let Some((uv, samp0)) = &resolved[gi] else {
                STATS[0].fetch_add(1, Relaxed);
                if std::env::var("FM2_UV_STATS").is_ok_and(|v| v == "3") {
                    let g = &groups[gi];
                    println!("[fm2] uv: group {gi} unpaired: ps {} dwords vs {} dwords; tail has {:?}", g.ps_dwords, g.vs_dwords, shaders.iter().map(|t| (t.vertex, t.dwords)).collect::<Vec<_>>());
                }
                return DrawUv { slots: out, ..Default::default() };
            };
            let mut samp = samp0;
            if let Some(p0) = pairs[gi].1 {
                let want = shaders[p0].dwords;
                let gap = |n: usize| (n as isize - d.items.len() as isize).abs();
                if let Some((_, alt)) = (0..shaders.len())
                    .filter(|&i| i != p0 && !shaders[i].vertex && shaders[i].dwords == want)
                    .filter_map(|i| Some((i, ps_samplers_all[i].as_ref()?)))
                    .filter(|(_, a)| gap(a.len()) < gap(samp0.len()))
                    .min_by_key(|(_, a)| gap(a.len()))
                {
                    samp = alt;
                }
            }
            // The draw's items fill the pixel shader's 2D samplers; when it lists fewer items than the shader samples, the leading samplers
            // (lightmap atlas pages bound per instance, shadow maps) are bound elsewhere and the items take the last ones.
            let s2d: Vec<&sh::PsSampler> = samp.iter().filter(|s| !s.cube).collect();
            // Bind items to samplers by role: shadow maps (system items) take the direct samplers with no texture coordinate source, lightmap tiles
            // the samplers whose coordinate the shader computes (atlas transform), everything else the direct samplers fed by a plain uv. Roles
            // that do not line up fall back to position.
            let role = |s: &sh::PsSampler| match s.direct {
                None if s.cube => ItemKind::Other,
                None if s.atlas.is_some() => ItemKind::Other,
                None => ItemKind::Tile,
                Some(r) if uv.contains_key(&r) => ItemKind::Other,
                Some(_) => ItemKind::System,
            };
            let mut by_role: BTreeMap<u8, Vec<&sh::PsSampler>> = BTreeMap::new();
            for s in samp {
                by_role.entry(role(s) as u8).or_default().push(s);
            }
            let item_kinds: Vec<ItemKind> = d.items.iter().map(|&it| kind(it)).collect();
            let count = |k: ItemKind| item_kinds.iter().filter(|x| **x == k).count();
            // Lightmap items beyond the shader's computed samplers are simply not sampled (a shader without a lightmap): they do not break the role match.
            let roles_match = [ItemKind::System, ItemKind::Other].iter().all(|k| count(*k) <= by_role.get(&(*k as u8)).map_or(0, Vec::len));
            let mut taken: BTreeMap<u8, usize> = BTreeMap::new();
            let by_kind: Vec<Option<&sh::PsSampler>> = item_kinds
                .iter()
                .map(|k| {
                    let n = taken.entry(*k as u8).or_insert(0);
                    let s = by_role.get(&(*k as u8)).and_then(|v| v.get(*n)).copied();
                    *n += 1;
                    s
                })
                .collect();
            let sampler_of = |j: usize| -> Option<&sh::PsSampler> {
                if roles_match {
                    by_kind[j]
                } else if d.items.len() == samp.len() {
                    samp.get(j)
                } else if d.items.len() <= s2d.len() {
                    s2d.get(s2d.len() - d.items.len() + j).copied()
                } else {
                    s2d.iter().find(|s| s.index as usize == j).copied()
                }
            };
            if std::env::var_os("FM2_UV_DEBUG").is_some() && d.items.len() == 4 && roles_match && STATS[5].fetch_add(1, Relaxed) < 6 {
                println!("[fm2] uv debug: items {:?} kinds {item_kinds:?} samplers {samp:?} roles_match {roles_match} by_kind {:?}", d.items, by_kind.iter().map(|s| s.map(|s| s.index)).collect::<Vec<_>>());
            }
            if s2d.iter().all(|s| s.direct.is_none()) {
                STATS[1].fetch_add(1, Relaxed);
            }
            for slot in 0..d.items.len() {
                // A texture bound to a cube sampler is an environment map, not a surface colour: an empty entry marks it.
                if sampler_of(slot).is_some_and(|s| s.cube) {
                    out.insert(slot, Vec::new());
                    continue;
                }
                let Some(reg) = sampler_of(slot).and_then(|s| s.direct.or(s.atlas.map(|a| a.0))) else { continue };
                let reg = &reg;
                let Some(u) = uv.get(reg) else {
                    STATS[2].fetch_add(1, Relaxed);
                    continue;
                };
                let m: Option<[[f32; 3]; 2]> = match u.map {
                    sh::UvMap::Scale(k) => Some([[k, 0.0, 0.0], [0.0, k, 0.0]]),
                    sh::UvMap::Raw => Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
                    sh::UvMap::Matrix { reg } => group_mats[gi].iter().find(|(r, _)| *r == reg).map(|(_, m)| [[m[0][0], m[0][1], m[0][3]], [m[1][0], m[1][1], m[1][3]]]),
                };
                let Some(m) = m else {
                    STATS[3].fetch_add(1, Relaxed);
                    continue;
                };
                STATS[4].fetch_add(1, Relaxed);
                let o = u.dword as usize * 4;
                if o + 4 > d.stride {
                    continue;
                }
                let uvs: Vec<[f32; 2]> = (0..d.positions.len())
                    .map(|i| {
                        let v = &d.vertices[i * d.stride..(i + 1) * d.stride];
                        let (h0, h1) = (i16::from_be_bytes([v[o], v[o + 1]]) as f32, i16::from_be_bytes([v[o + 2], v[o + 3]]) as f32);
                        [m[0][0] * h0 + m[0][1] * h1 + m[0][2], m[1][0] * h0 + m[1][1] * h1 + m[1][2]]
                    })
                    .collect();
                // A raw integer pair is a tile count only when it stays small and varies.
                if matches!(u.map, sh::UvMap::Raw) {
                    let big = uvs.iter().any(|p| p[0].abs() > 512.0 || p[1].abs() > 512.0);
                    let flat = uvs.iter().all(|p| *p == uvs[0]);
                    if big || flat {
                        STATS[2].fetch_add(1, Relaxed);
                        continue;
                    }
                }
                if let Some((_, c)) = sampler_of(slot).and_then(|s| s.atlas) {
                    atlas.insert(slot, c);
                }
                out.insert(slot as usize, uvs);
            }
            DrawUv { slots: out, alpha_const, atlas }
        })
        .collect()
}
