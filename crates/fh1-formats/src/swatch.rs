//! FM6 (Xbox One) `.swatchbin` textures: a `burG` container (see [`crate::grub`]) with one `TXCB` chunk.
//!
//! Header (the chunk's metadata block): tag `HCXT`, 4 bytes, a 16-byte GUID, then u16 width, u16 height,
//! u16 depth (1), the same three again, u8 mip count twice, a zero byte and 4 flag bytes.
//!
//! Pixel data: every mip, largest first, no gaps. Elements are 4x4 blocks for BC formats and single pixels for
//! 8888. A mip is padded to whole 8x8-element micro tiles (64 elements). The addressing is AMD GCN tiling, as in
//! Microsoft's `xg_x.dll` (AddrLib), and was checked against the data (each mip, shrunk 2x, must equal the next):
//!
//! - small mips (narrower than 64 or shorter than 32 elements): "1D thin": micro tiles row by row, the 64
//!   elements inside a tile in Morton order `x0 y0 x1 y1 x2 y2` (index bit 0 = x0, bit 1 = y0 ...);
//! - larger mips: "2D thin" macro tiles of 64x32 elements. The element index has, from the low bit up: the first
//!   `8 - log2(bytes)` Morton bits (one 256-byte group); 3 pipe bits `p0 = x3^y3^x4, p1 = x4^y4, p2 = x5^y5`;
//!   2 bank bits `b0 = y4^y5^x6, b1 = y3^x7`; the remaining Morton bits; then the macro tile number
//!   `(y >> 5) * macros_per_row + (x >> 6)`.
//!
//! Checked on BC1, BC4, BC5 and 8888 textures up to 1024x1024 (BC7 only through its size and flags; see
//! STATUS.md). Pipe / bank terms beyond x7 are not needed up to 1024 pixels and are unknown beyond that.
//! Not supported (returns `Unsupported`): the 27 AO maps (flags `6e 02 00 90`), 16-bit float maps (`6e 01 00 a0`),
//! two small tiling maps whose size does not fit, and 2D mips whose sides are not multiples of 64 x 32.

use crate::{grub, Error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc4,
    Bc5,
    Bc7,
    /// Four bytes per pixel, byte order as stored (not checked against the game: normal and radiosity maps).
    Rgba8,
}

impl Format {
    /// Bytes per element (a 4x4 block, or one pixel for `Rgba8`).
    pub fn element_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc4 => 8,
            Format::Bc5 | Format::Bc7 => 16,
            Format::Rgba8 => 4,
        }
    }

    fn block(self) -> usize {
        if self == Format::Rgba8 {
            1
        } else {
            4
        }
    }
}

pub struct Swatch<'a> {
    pub width: usize,
    pub height: usize,
    pub mips: usize,
    pub format: Format,
    pub flags: [u8; 4],
    pub guid: [u8; 16],
    /// Small block-compressed textures (MODLOG 112): mip 0 is stored macro-tiled, padded to whole 64x32-element macro
    /// tiles, even when smaller than one; the smaller mips' padding is not fully known, so only mip 0 can be decoded.
    pub mip0_only: bool,
    data: &'a [u8],
}

fn pad8(v: usize) -> usize {
    v.div_ceil(8) * 8
}

fn morton(ix: usize, iy: usize) -> usize {
    let mut m = 0;
    for i in 0..3 {
        m |= ((ix >> i) & 1) << (2 * i);
        m |= ((iy >> i) & 1) << (2 * i + 1);
    }
    m
}

/// Position of element `(x, y)` inside a mip that is `a` elements wide, in elements from the mip's start.
pub fn element_index(x: usize, y: usize, a: usize, bytes: usize, two_d: bool) -> usize {
    if !two_d {
        return ((y >> 3) * (pad8(a) / 8) + (x >> 3)) * 64 + morton(x & 7, y & 7);
    }
    let bit = |v: usize, i: usize| (v >> i) & 1;
    let t = morton(x & 7, y & 7);
    let low = 8 - bytes.trailing_zeros() as usize;
    let pipe = (bit(x, 3) ^ bit(y, 3) ^ bit(x, 4)) | ((bit(x, 4) ^ bit(y, 4)) << 1) | ((bit(x, 5) ^ bit(y, 5)) << 2);
    let bank = (bit(y, 4) ^ bit(y, 5) ^ bit(x, 6)) | ((bit(y, 3) ^ bit(x, 7)) << 1);
    let macro_tile = (y >> 5) * a.div_ceil(64) + (x >> 6);
    (t & ((1 << low) - 1)) | (pipe << low) | (bank << (low + 3)) | ((t >> low) << (low + 5)) | (macro_tile << (11))
}

pub fn parse(file: &[u8]) -> Result<Swatch<'_>, Error> {
    let g = grub::parse(file)?;
    let c = g.chunks.iter().find(|c| c.tag == "TXCB").ok_or(Error::BadMagic("swatchbin without TXCB"))?;
    let meta = g.meta_of(c)?;
    let data = g.data_of(c)?;
    if meta.len() < 43 {
        return Err(Error::Truncated("swatchbin header"));
    }
    let r = &meta[24..];
    let w = u16::from_le_bytes([r[0], r[1]]) as usize;
    let h = u16::from_le_bytes([r[2], r[3]]) as usize;
    let mips = r[12] as usize;
    let flags = [r[15], r[16], r[17], r[18]];
    let mut guid = [0u8; 16];
    guid.copy_from_slice(&meta[8..24]);
    let format = match (flags[0], flags[1]) {
        (0x0e, 0) => Format::Bc1,
        (0x6e, 0) => Format::Bc4,
        (0xae, 0) => Format::Bc5,
        (0x2e, 1) => Format::Bc7,
        (0xae, 1) | (0xad, 1) => Format::Rgba8,
        _ => return Err(Error::Unsupported(format!("swatchbin flags {flags:02x?}"))),
    };
    let mut s = Swatch { width: w, height: h, mips, format, flags, guid, mip0_only: false, data };
    let total: usize = (0..mips).map(|i| s.slot_bytes(i)).sum();
    if total != data.len() {
        // The small-texture layout: 444 of 1,773 library textures (all block-compressed, mostly 64..128 px or 64 px
        // tall) have mip 0 as whole 64x32-element macro tiles (MODLOG 112).
        let (a, b) = s.mip_elements(0);
        let mip0 = a.div_ceil(64) * 64 * b.div_ceil(32) * 32 * format.element_bytes();
        if format == Format::Rgba8 || data.len() < mip0 {
            return Err(Error::Unsupported(format!("swatchbin size {} does not fit {w}x{h} {mips} mips {format:?} ({total})", data.len())));
        }
        s.mip0_only = true;
    }
    Ok(s)
}

impl<'a> Swatch<'a> {
    /// Size of mip `i` in elements.
    pub fn mip_elements(&self, i: usize) -> (usize, usize) {
        let q = self.format.block();
        ((self.width >> i).max(1).div_ceil(q), (self.height >> i).max(1).div_ceil(q))
    }

    /// Size of mip `i` in pixels.
    pub fn mip_pixels(&self, i: usize) -> (usize, usize) {
        ((self.width >> i).max(1), (self.height >> i).max(1))
    }

    fn slot_bytes(&self, i: usize) -> usize {
        let (a, b) = self.mip_elements(i);
        pad8(a) * pad8(b) * self.format.element_bytes()
    }

    /// Decodes mip `i` to RGBA8 (`width * height * 4` bytes). BC4 fills R only (G = B = 0, A = 255), BC5 fills R and G.
    pub fn decode_mip(&self, i: usize) -> Result<Vec<u8>, Error> {
        let (a, b) = self.mip_elements(i);
        let bytes = self.format.element_bytes();
        if self.mip0_only && i > 0 {
            return Err(Error::Unsupported("small-texture layout: only mip 0 is known".into()));
        }
        let two_d = self.mip0_only || (a >= 64 && b >= 32);
        if two_d && !self.mip0_only && (a % 64 != 0 || b % 32 != 0) {
            return Err(Error::Unsupported(format!("2D mip {a}x{b} elements is not a multiple of 64x32")));
        }
        let (off, len) = if self.mip0_only {
            (0, a.div_ceil(64) * 64 * b.div_ceil(32) * 32 * bytes)
        } else {
            ((0..i).map(|k| self.slot_bytes(k)).sum(), self.slot_bytes(i))
        };
        let slot = &self.data[off..off + len];
        let mut linear = vec![0u8; a * b * bytes];
        for y in 0..b {
            for x in 0..a {
                let src = element_index(x, y, a, bytes, two_d) * bytes;
                linear[(y * a + x) * bytes..][..bytes].copy_from_slice(&slot[src..src + bytes]);
            }
        }
        let (w, h) = self.mip_pixels(i);
        let (pw, ph) = (a * self.format.block(), b * self.format.block());
        let mut px = vec![0u32; pw * ph];
        let bad = |e: &str| Error::Unsupported(format!("bc decode: {e}"));
        match self.format {
            Format::Bc1 => texture2ddecoder::decode_bc1(&linear, pw, ph, &mut px).map_err(bad)?,
            Format::Bc4 => texture2ddecoder::decode_bc4(&linear, pw, ph, &mut px).map_err(bad)?,
            Format::Bc5 => texture2ddecoder::decode_bc5(&linear, pw, ph, &mut px).map_err(bad)?,
            Format::Bc7 => texture2ddecoder::decode_bc7(&linear, pw, ph, &mut px).map_err(bad)?,
            Format::Rgba8 => {
                for (k, p) in px.iter_mut().enumerate() {
                    let c = &linear[k * 4..k * 4 + 4];
                    *p = u32::from_le_bytes([c[2], c[1], c[0], c[3]]);
                }
            }
        }
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let b = px[y * pw + x].to_le_bytes(); // BGRA
                out.extend_from_slice(&[b[2], b[1], b[0], b[3]]);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_permutations() {
        for &(a, b, bytes, two_d) in &[(64usize, 32usize, 8usize, true), (128, 64, 16, true), (128, 128, 4, true), (16, 8, 8, false), (4, 2, 16, false)] {
            let mut seen = std::collections::HashSet::new();
            for y in 0..b {
                for x in 0..a {
                    assert!(seen.insert(element_index(x, y, a, bytes, two_d)), "duplicate at {x},{y} for {a}x{b} {bytes} {two_d}");
                }
            }
            let slots = pad8(a) * pad8(b);
            assert!(seen.iter().all(|&i| i < slots), "index beyond slot for {a}x{b}");
        }
    }

    #[test]
    fn first_micro_tile_is_morton() {
        assert_eq!(element_index(1, 0, 16, 8, false), 1);
        assert_eq!(element_index(0, 1, 16, 8, false), 2);
        assert_eq!(element_index(2, 0, 16, 8, false), 4);
    }
}
