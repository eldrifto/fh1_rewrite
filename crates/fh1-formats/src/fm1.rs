//! Forza Motorsport 1 (original Xbox, 2005) containers and car models.
//!
//! **Archives** (`.cab`, and `.ca2` / `.xb2` after [`unchunk`]): `AA AA C0 C0`, a version word, the entry count at 8, the directory count
//! at 12, `directories * 0x102` bytes of directory names, then one 24-byte entry per file `{offset, compressed size, size, c, z, hash}`.
//! `offset` is where the member's bytes sit in the archive (zlib when they start with `0x78`, else stored), `hash` is a 31-bit hash of the
//! file's path (the function is in `default.xbe` and is not recovered: files are matched by content and by archive order, see
//! `fh1setup::fm1`). A `.ca2` is a run of zlib streams, each starting on a 2 KB boundary.
//!
//! **Textures** (`XPR0`): a 2048-byte header whose word at 24 is the Xbox format dword (`format << 8`, mip count `<< 16`, `log2 width << 20`,
//! `log2 height << 24`, `log2 depth << 28`), then the pixels. Only the 2D DXT1 / DXT3 / DXT5 ones are decoded.
//!
//! **Cars** (the blob inside `car_<name>.cab` whose bytes 4..12 are `3, "Car\0"`): chunks `{size u32, version u32, tag\0, ...}`. `Attribs`
//! carries the four wheel hubs (`FL, FR, RR, RL`, three floats each); every `Part` chunk is `{size, 3, "Part\0", flag, hash, name_len, name,
//! ..., "Verts\0", count u32, count * 20 bytes, ..., Subpart chunks}`. A vertex is ten `i16`: position / 6553.6 (x right, y up, z forward: a
//! left-handed frame, the car's origin on the body's underside), a packed normal, one unused word, then `u, v` as `u16 / 32768` at words 8 and
//! 9. A `Subpart` chunk is `{size, ver, "Subpart\0", lod, material, 6, count, 16 bytes, count * u16}`: a triangle strip into the part's
//! vertices (`lod` 1 is the most detailed). Materials are the game's enum ([`MATERIALS`]).

use crate::Error;

pub const MAGIC: [u8; 4] = [0xAA, 0xAA, 0xC0, 0xC0];

/// The game's material enum (`default.xbe`: the table at VA 0x512790, `{name, id}` pairs), indexed by id.
pub const MATERIALS: [&str; 36] = [
    "None", "Body", "Window", "TailLights", "HeadLights", "ReverseLights", "BlackGlass", "Chrome", "Tire", "Black", "Tire2", "Rim", "InnerRim", "BlurredRim",
    "Interior", "Frame", "RedGlass", "RimChrome", "Matte", "CarbonFiber", "Plastic", "RubberTrim", "InnerRimChrome", "BlurredRimChrome", "NamePlate", "Badge",
    "Number", "Misc", "HeadlightGlass", "WhiteTailLights", "TailLightsOn", "Reflectors", "Grille1", "Grille2", "Misc2", "Plastic2",
];

fn u32_at(d: &[u8], p: usize) -> Option<u32> {
    d.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Inflates one zlib stream from the start of `d`: the bytes and how many input bytes it used.
fn zlib(d: &[u8]) -> Option<(Vec<u8>, usize)> {
    use std::io::Read;
    let mut dec = flate2::read::ZlibDecoder::new(d);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    Some((out, dec.total_in() as usize))
}

/// The bytes of a `.ca2` / `.xb2`: zlib streams, each starting on a 2 KB boundary, joined.
pub fn unchunk(d: &[u8]) -> Vec<u8> {
    let (mut out, mut pos) = (Vec::new(), 0);
    while pos < d.len() && d[pos] == 0x78 {
        let Some((chunk, used)) = zlib(&d[pos..]) else { break };
        out.extend_from_slice(&chunk);
        pos += used.div_ceil(2048) * 2048;
    }
    out
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Where the member starts in the archive (the order the authoring tool stored the files in).
    pub offset: u32,
    pub hash: u32,
    pub data: Vec<u8>,
}

/// Every file of an archive (a `.cab` as it is, a `.ca2` after [`unchunk`]), in archive order.
pub fn archive(buf: &[u8]) -> Result<Vec<Entry>, Error> {
    if buf.get(..4) != Some(&MAGIC) {
        return Err(Error::BadMagic("FM1 archive"));
    }
    let n = u32_at(buf, 8).ok_or(Error::Truncated("FM1 archive header"))? as usize;
    let dirs = u32_at(buf, 12).ok_or(Error::Truncated("FM1 archive header"))? as usize;
    let table = 0x30 + dirs * 0x102;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = table + i * 24;
        let (off, csize, hash) = (u32_at(buf, p).ok_or(Error::Truncated("FM1 entry"))?, u32_at(buf, p + 4).unwrap_or(0) as usize, u32_at(buf, p + 20).unwrap_or(0));
        let raw = buf.get(off as usize..off as usize + csize).ok_or(Error::Truncated("FM1 member"))?;
        let data = if raw.first() == Some(&0x78) { zlib(raw).map(|(d, _)| d).unwrap_or_else(|| raw.to_vec()) } else { raw.to_vec() };
        out.push(Entry { offset: off, hash, data });
    }
    out.sort_by_key(|e| e.offset);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------
// textures

#[derive(Debug, Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// `(format, mips, width, height, depth)` of an `XPR0` file.
pub fn xpr_info(d: &[u8]) -> Option<(u8, u32, u32, u32, u32)> {
    if d.get(..4) != Some(b"XPR0") {
        return None;
    }
    let f = u32_at(d, 24)?;
    Some((((f >> 8) & 0xFF) as u8, (f >> 16) & 15, 1 << ((f >> 20) & 15), 1 << ((f >> 24) & 15), 1 << ((f >> 28) & 15)))
}

/// A flat (depth 1) DXT1 / DXT3 / DXT5 `XPR0` texture, base level.
pub fn xpr_decode(d: &[u8]) -> Option<Texture> {
    let (fmt, _, w, h, depth) = xpr_info(d)?;
    if depth != 1 {
        return None;
    }
    let body = d.get(u32_at(d, 8)? as usize..)?;
    let (w_, h_) = (w as usize, h as usize);
    let mut px = vec![0u32; w_ * h_];
    match fmt {
        0x0C => texture2ddecoder::decode_bc1(body, w_, h_, &mut px).ok()?,
        0x0E => texture2ddecoder::decode_bc2(body, w_, h_, &mut px).ok()?,
        0x0F => texture2ddecoder::decode_bc3(body, w_, h_, &mut px).ok()?,
        _ => return None,
    }
    // texture2ddecoder packs BGRA into little-endian u32s.
    let rgba = px.iter().flat_map(|p| {
        let [b, g, r, a] = p.to_le_bytes();
        [r, g, b, a]
    });
    Some(Texture { width: w, height: h, rgba: rgba.collect() })
}

// ---------------------------------------------------------------------------------------------------------------
// cars

#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    /// Metres, the game's frame (x right, y up, z forward).
    pub position: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Subpart {
    pub lod: u32,
    /// Index into [`MATERIALS`].
    pub material: u32,
    /// Triangle list, local to the part's vertices.
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub subparts: Vec<Subpart>,
}

#[derive(Debug, Clone)]
pub struct Car {
    /// Wheel hubs `[front left, front right, rear right, rear left]`, the game's frame; `None` when the blob has no `Attribs` chunk.
    pub hubs: Option<[[f32; 3]; 4]>,
    pub parts: Vec<Part>,
}

fn find(d: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    d.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// True for the car-model blob of a `car_<name>.cab` (or a wheel blob of `Mega.ca2`).
pub fn is_car_blob(d: &[u8]) -> bool {
    d.get(4..12) == Some(b"\x03\x00\x00\x00Car\x00")
}

pub fn parse_car(blob: &[u8]) -> Result<Car, Error> {
    if !is_car_blob(blob) {
        return Err(Error::BadMagic("FM1 car blob"));
    }
    let hubs = find(blob, b"Attribs\0", 0).and_then(|p| {
        let f = |i: usize| blob.get(p + 8 + 8 + i * 4..p + 8 + 8 + i * 4 + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()));
        let mut h = [[0.0f32; 3]; 4];
        for (i, hub) in h.iter_mut().enumerate() {
            for (k, v) in hub.iter_mut().enumerate() {
                *v = f(i * 3 + k)?;
            }
        }
        // Sanity: hubs are a few metres at most.
        h.iter().flatten().all(|v| v.abs() < 10.0).then_some(h)
    });
    let mut parts = Vec::new();
    let mut from = 0;
    while let Some(t) = find(blob, b"Part\0", from) {
        from = t + 5;
        let Some(start) = t.checked_sub(8) else { continue };
        let size = u32_at(blob, start).unwrap_or(0) as usize;
        let Some(c) = blob.get(start..start + size) else { continue };
        // The name: a length-prefixed identifier a few bytes after the tag.
        let mut name = None;
        for k in 12..24 {
            let Some(len) = u32_at(c, k).map(|l| l as usize).filter(|l| (3..=40).contains(l)) else { continue };
            if let Some(s) = c.get(k + 4..k + 4 + len).filter(|s| s.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')) {
                name = Some(String::from_utf8_lossy(s).into_owned());
                break;
            }
        }
        let (Some(name), Some(v)) = (name, find(c, b"Verts\0", 0)) else { continue };
        let count = u32_at(c, v + 6).unwrap_or(0) as usize;
        let Some(vb) = c.get(v + 10..v + 10 + count * 20) else { continue };
        let vertices: Vec<Vertex> = vb
            .chunks_exact(20)
            .map(|b| {
                let s = |i: usize| i16::from_le_bytes([b[i * 2], b[i * 2 + 1]]) as f32;
                let u = |i: usize| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]) as f32;
                Vertex { position: [s(0) / 6553.6, s(1) / 6553.6, s(2) / 6553.6], uv: [u(8) / 32768.0, u(9) / 32768.0] }
            })
            .collect();
        let mut subparts = Vec::new();
        let mut sfrom = v;
        while let Some(s) = find(c, b"Subpart\0", sfrom) {
            sfrom = s + 8;
            let q = s + 8;
            let (Some(lod), Some(material), Some(cnt)) = (u32_at(c, q), u32_at(c, q + 4), u32_at(c, q + 12)) else { continue };
            let Some(ib) = c.get(q + 32..q + 32 + cnt as usize * 2) else { continue };
            let idx: Vec<u32> = ib.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32).collect();
            if idx.iter().any(|i| *i as usize >= vertices.len()) {
                continue;
            }
            subparts.push(Subpart { lod, material, indices: strip_to_list(&idx) });
        }
        parts.push(Part { name, vertices, subparts });
    }
    Ok(Car { hubs, parts })
}

/// A triangle strip as a triangle list (degenerate joins dropped, every second triangle's winding corrected).
pub fn strip_to_list(s: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(s.len() * 3);
    for (j, w) in s.windows(3).enumerate() {
        let (a, b, c) = (w[0], w[1], w[2]);
        if a == b || b == c || a == c {
            continue;
        }
        if j % 2 == 0 {
            out.extend_from_slice(&[a, b, c]);
        } else {
            out.extend_from_slice(&[b, a, c]);
        }
    }
    out
}

/// The `Cars.ca2` per-car groups: the archive's files in storage order, split where a car's first file (the `EngExhB` audio ini) starts,
/// then the engine groups (a `<Engine ... <Data Mass=` file starts one). `Cars.ca2` is stored as one run of per-car folders in the
/// case-insensitive alphabetical order of the folder names (`car_<name>.cab` sorted the same way), then one run of engine folders.
#[derive(Debug, Default)]
pub struct CarFiles {
    /// Per car, in alphabetical order: `(kind, text)` where kind is the root element (`MAXData`, `Weight`, ...).
    pub cars: Vec<Vec<(String, String)>>,
    /// Per engine folder, in alphabetical order.
    pub engines: Vec<Vec<(String, String)>>,
}

fn kind_of(text: &str) -> String {
    let mut t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<?") {
        t = t.find("?>").map(|e| t[e + 2..].trim_start()).unwrap_or("");
    }
    match t.strip_prefix('<') {
        Some(r) => r.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect(),
        None => text.chars().take(8).collect(),
    }
}

pub fn car_files(entries: &[Entry]) -> CarFiles {
    let mut out = CarFiles::default();
    // 0 = before the first group, 1 = in a car, 2 = in an engine.
    let mut mode = 0;
    for e in entries {
        let text = String::from_utf8_lossy(&e.data).into_owned();
        let kind = kind_of(&text);
        if kind == "carlist" {
            continue;
        }
        if text.starts_with("EngExhB ") {
            out.cars.push(Vec::new());
            mode = 1;
        } else if kind == "Engine" && text.contains("<Data Mass=") {
            out.engines.push(Vec::new());
            mode = 2;
        }
        let group = match mode {
            1 => out.cars.last_mut(),
            2 => out.engines.last_mut(),
            _ => None,
        };
        if let Some(g) = group {
            g.push((kind, text));
        }
    }
    out
}
