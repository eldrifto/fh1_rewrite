//! FM6 (Xbox One) materials: `.materialbin` files and the material instances (`MatI` chunks) inside
//! `.modelbin` files. Both are `burG` containers (see [`crate::grub`]).
//!
//! - `MATL` chunk: the shader (a `Game:\...\x.shaderbin` path, one length byte before it).
//! - `MATI` chunk (instances only): the parent material (a `Game:\...\x.materialbin` path).
//! - `MTPR` chunk: parameters. u8 count, then per entry: u16 version (3 = FM6, 2 = FM5), u32 name hash, u8 type,
//!   a 16-byte GUID (version 3 only), value:
//!   type 0 = 4 floats (a colour), 1 = 4 floats, 2 = float, 3 and 4 = u32, 6 = texture (7-bit variable-length
//!   length + path, then 4 bytes), 11 = 2 floats. 3,145 of the library's 3,151 files parse to the end with these; type 7 (6 files,
//!   all in `exterior_doNotUse`) is not understood, and parsing stops there keeping what was read.
//!   An instance's parameters replace its parent's with the same hash.
//!
//! The hashes are not names: they did not match CRC-32 or FNV-1a of any string in the shaders. What a few of
//! them mean was read from their values across all IDx materials (docs/DESIGN.md "FM6 IDx", step 4), not from
//! the game's code.

use crate::{grub, Error};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Color([f32; 4]),
    Vector([f32; 4]),
    Float(f32),
    Int(u32),
    Vector2([f32; 2]),
    Texture(String),
}

#[derive(Debug, Clone, Default)]
pub struct Material {
    /// Shader file name, e.g. `carpaint_standard.shaderbin` (`None` for an instance that only names a parent).
    pub shader: Option<String>,
    /// Parent material path (instances only), as written in the file (`Game:\Media\...`).
    pub parent: Option<String>,
    pub params: Vec<(u32, Value)>,
}

fn game_path(b: &[u8]) -> Option<String> {
    let start = b.windows(5).position(|w| w.eq_ignore_ascii_case(b"Game:"))?;
    let end = b[start..].iter().position(|&c| !(0x20..0x7f).contains(&c)).map_or(b.len(), |e| start + e);
    Some(String::from_utf8_lossy(&b[start..end]).into_owned())
}

fn f4(b: &[u8], o: usize) -> Option<[f32; 4]> {
    let mut v = [0.0; 4];
    for (k, x) in v.iter_mut().enumerate() {
        *x = f32::from_le_bytes(b.get(o + 4 * k..o + 4 * k + 4)?.try_into().ok()?);
    }
    Some(v)
}

fn parse_params(b: &[u8]) -> Result<Vec<(u32, Value)>, Error> {
    let bad = || Error::Truncated("MTPR");
    let n = *b.first().ok_or_else(bad)? as usize;
    let mut o = 1;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let ver = u16::from_le_bytes(b.get(o..o + 2).ok_or_else(bad)?.try_into().unwrap());
        let h = u32::from_le_bytes(b.get(o + 2..o + 6).ok_or_else(bad)?.try_into().unwrap());
        let t = *b.get(o + 6).ok_or_else(bad)?;
        // entry version 3 (FM6) carries a 16-byte GUID before the value; version 2 (FM5) does not
        o += 7 + if ver >= 3 { 16 } else { 0 };
        let word = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).ok_or_else(bad);
        let v = match t {
            0 => Value::Color(f4(b, o).ok_or_else(bad)?),
            1 => Value::Vector(f4(b, o).ok_or_else(bad)?),
            2 => Value::Float(f32::from_bits(word(o)?)),
            3 | 4 => Value::Int(word(o)?),
            6 => {
                // the length is a 7-bit variable-length number (.NET string style): paths of 128+ characters use two
                // bytes (MODLOG 109; reading one byte cut the car's own long texture paths short)
                let (mut l, mut shift) = (0usize, 0);
                loop {
                    let c = *b.get(o).ok_or_else(bad)?;
                    o += 1;
                    l |= ((c & 0x7f) as usize) << shift;
                    shift += 7;
                    if c & 0x80 == 0 || shift > 21 {
                        break;
                    }
                }
                o += l; // the path; 4 more bytes (not decoded) follow
                Value::Texture(String::from_utf8_lossy(b.get(o - l..o).ok_or_else(bad)?).into_owned())
            }
            11 => Value::Vector2([f32::from_bits(word(o)?), f32::from_bits(word(o + 4)?)]),
            _ => break, // not understood: keep what was read
        };
        o += match t {
            0 | 1 => 16,
            11 => 8,
            _ => 4,
        };
        out.push((h, v));
    }
    Ok(out)
}

/// Reads one `.materialbin` file or one `MatI` chunk's data (an embedded container). Empty data gives an empty material.
pub fn parse(file: &[u8]) -> Result<Material, Error> {
    let mut m = Material::default();
    if file.len() < 20 {
        return Ok(m);
    }
    let g = grub::parse(file)?;
    for c in &g.chunks {
        let d = g.data_of(c)?;
        match c.tag.as_str() {
            "MATL" => m.shader = game_path(d).map(|p| p.rsplit('\\').next().unwrap_or("").to_string()),
            "MATI" => m.parent = game_path(d),
            "MTPR" => m.params = parse_params(d)?,
            _ => {}
        }
    }
    Ok(m)
}

impl Material {
    /// Parameters of `self` laid over `parent`'s (same hash: `self` wins).
    pub fn over(&self, parent: &Material) -> Material {
        let mut params = parent.params.clone();
        for (h, v) in &self.params {
            match params.iter_mut().find(|(ph, _)| ph == h) {
                Some(p) => p.1 = v.clone(),
                None => params.push((*h, v.clone())),
            }
        }
        Material { shader: self.shader.clone().or(parent.shader.clone()), parent: parent.parent.clone(), params }
    }

    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.params.iter().filter_map(|(_, v)| if let Value::Texture(s) = v { Some(s.as_str()) } else { None })
    }

    pub fn get(&self, hash: u32) -> Option<&Value> {
        self.params.iter().find(|(h, _)| *h == hash).map(|(_, v)| v)
    }
}
