//! Track textures: `_0x########.bix` (header + mip tail) paired with `_0x########_B.bix`
//! (the full-resolution base level, Xenos-tiled). Big-endian.
//!
//! Layout from the Colorado recon (`docs/COLORADO_RECON.md`, ".bix / _B.bix textures"):
//! `"BIX1"` (FM3: `"BIX0"`), u32 width, height, mip_count, format word (fetch-constant dword 1: low 6 bits =
//! GPU format, bits 6-7 = endian), total_bytes, base_bytes`. The `_B` file holds `base_bytes`
//! of tiled 4x4 blocks with the same addressing as `.xds` textures.

use crate::xds::{self, Format, Image};
use crate::Error;

pub const HEADER_LEN: usize = 28;

#[derive(Debug, Clone, Copy)]
pub struct BixHeader {
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub format: Format,
    pub endian: u32,
    pub total_bytes: u32,
    pub base_bytes: u32,
}

pub fn parse_header(bix: &[u8]) -> Result<BixHeader, Error> {
    // `BIX1` (FH1 / FH2 / FM4) or `BIX0` (FM3: the same header fields).
    if !matches!(bix.get(..4), Some(b"BIX1" | b"BIX0")) || bix.len() < HEADER_LEN {
        return Err(Error::BadMagic("bix"));
    }
    let u = |i: usize| u32::from_be_bytes(bix[4 + i * 4..8 + i * 4].try_into().unwrap());
    let word = u(3);
    Ok(BixHeader {
        width: u(0),
        height: u(1),
        mip_count: u(2),
        format: Format::from_raw(word & 0x3F),
        endian: (word >> 6) & 3,
        total_bytes: u(4),
        base_bytes: u(5),
    })
}

/// Decode the base level from the `.bix` header and its `_B.bix` data.
pub fn decode_base(bix: &[u8], base: &[u8]) -> Result<(BixHeader, Image), Error> {
    let h = parse_header(bix)?;
    if base.len() < h.base_bytes as usize {
        return Err(Error::Truncated("_B.bix"));
    }
    let img = xds::untile(base, h.format, h.width, h.height, h.width, true, h.endian)?;
    Ok((h, img))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bix(magic: &[u8; 4]) -> Vec<u8> {
        let mut d = magic.to_vec();
        // width 512, height 256, 10 mips, DXT5 (0x14) big-endian format word, total / base bytes.
        for v in [512u32, 256, 10, 0x1A20_0154, 0x0006_0000, 0x0004_0000] {
            d.extend(v.to_be_bytes());
        }
        d
    }

    #[test]
    fn fm3_bix0_and_fh1_bix1_headers_parse_alike() {
        for magic in [b"BIX0", b"BIX1"] {
            let h = parse_header(&bix(magic)).unwrap();
            assert_eq!((h.width, h.height, h.mip_count, h.base_bytes), (512, 256, 10, 0x4_0000));
        }
        assert!(parse_header(&bix(b"BIX2")).is_err());
    }
}
