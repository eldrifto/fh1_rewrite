//! CAFF compiled track textures (`bin/_0x########.bin`, the textures the PVS flags as non-`.bix`).
//!
//! Layout (verified on all 2,965 unique Colorado CAFF textures): `"CAFF21.11.05.0034"` container with a
//! `.data` and a `.gpu` section. The `.gpu` section is the last `gpu_size` bytes of the file (u32 at 0x88)
//! and holds the Xenos-tiled texels, base level first (same tiling as `.xds` / `.bix`). In `.data`, the
//! asset starts with `"texture\0"` + version string; 0x18 bytes after `"texture"`:
//! `u32 format word` (fetch-constant dword 1: low 6 bits GPU format, bits 6-7 endian),
//! `u32 kind` (0 = 2D, 2 = cube), `u32 0`, `u16 width, u16 height`, `u32 0`, `u32 -1`, `u8 mip count`.
//! Colorado: 2,953 DXT1 + 9 DXT5 2D textures (2,877 are 64x64 with 7 mips) and 3 DXT1 cube maps.

use crate::xds::{self, Format, Image};
use crate::Error;

#[derive(Debug, Clone, Copy)]
pub struct CaffTexture {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub endian: u32,
    pub cube: bool,
    /// The fetch-constant format word as stored (gamma flag in bits 8-13, see fh1setup textures.rs).
    pub raw_format: u32,
    pub mip_count: u32,
    /// Offset and length of the `.gpu` section (tiled texels).
    pub gpu_offset: usize,
    pub gpu_len: usize,
}

pub fn parse(d: &[u8]) -> Result<CaffTexture, Error> {
    if !d.starts_with(b"CAFF") || d.len() < 0x90 {
        return Err(Error::BadMagic("CAFF"));
    }
    let be = |o: usize| -> Result<u32, Error> {
        Ok(u32::from_be_bytes(d.get(o..o + 4).ok_or(Error::Truncated("caff"))?.try_into().unwrap()))
    };
    let gpu_len = be(0x88)? as usize;
    let gpu_offset = d.len().checked_sub(gpu_len).ok_or(Error::Truncated("caff .gpu"))?;
    let tex = d.windows(8).take(0x1000).position(|w| w == b"texture\0").ok_or(Error::BadMagic("caff: not a texture"))?;
    let o = tex + 0x18;
    let word = be(o)?;
    let dims = be(o + 12)?;
    Ok(CaffTexture {
        width: dims >> 16,
        height: dims & 0xFFFF,
        format: Format::from_raw(word & 0x3F),
        endian: (word >> 6) & 3,
        cube: be(o + 4)? == 2,
        raw_format: word,
        mip_count: be(o + 24)? >> 24,
        gpu_offset,
        gpu_len,
    })
}

/// The textures of a Forza Motorsport 3 track's `<track>.stx.bin`: one CAFF container with an array of 128-byte `"texture"`
/// descriptors in `.data` (u32 count at 0x18, first at u32 0x48) and every texture's Xenos-tiled chain back to back in `.gpu`
/// (the last u32-at-0x88 bytes). Entry `i` is the data of PVS texture `i` for the PVS's runtime textures (flags 1: grass and
/// fence diffuse...), the ones no `.bix` / `.bin` file or bundle preview holds (VERIFIED on Silverstone, Nurburgring: the
/// entry sizes sum to the `.gpu` length exactly; PVS runtime textures 889..893 are the 512x512 + 4x 2048x1024 entries 889..893).
/// Each returned [`CaffTexture`]'s `gpu_offset` is absolute in the container and `gpu_len` its own chain size.
pub fn parse_stx(d: &[u8]) -> Result<Vec<CaffTexture>, Error> {
    if !d.starts_with(b"CAFF") || d.len() < 0x90 {
        return Err(Error::BadMagic("CAFF"));
    }
    let be = |o: usize| -> Result<u32, Error> {
        Ok(u32::from_be_bytes(d.get(o..o + 4).ok_or(Error::Truncated("stx"))?.try_into().unwrap()))
    };
    let (count, first, gpu_len) = (be(0x18)? as usize, be(0x48)? as usize, be(0x88)? as usize);
    let mut gpu = d.len().checked_sub(gpu_len).ok_or(Error::Truncated("stx .gpu"))?;
    let gpu_start = gpu;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let p = first + i * 0x80;
        if d.get(p..p + 8) != Some(b"texture\0") {
            return Err(Error::BadMagic("stx: texture descriptor"));
        }
        let o = p + 0x18;
        let (word, dims) = (be(o)?, be(o + 12)?);
        let format = Format::from_raw(word & 0x3F);
        let (cube, mips) = (be(o + 4)? == 2, be(o + 24)? >> 24);
        let (width, height) = (dims >> 16, dims & 0xFFFF);
        let len = chain_size(format, width, height, mips.max(1), if cube { 6 } else { 1 })?;
        out.push(CaffTexture {
            width,
            height,
            format,
            endian: (word >> 6) & 3,
            cube,
            raw_format: word,
            mip_count: mips,
            gpu_offset: gpu,
            gpu_len: len,
        });
        gpu += len;
    }
    if gpu - gpu_start != gpu_len {
        return Err(Error::SizeMismatch { expected: gpu_len, got: gpu - gpu_start });
    }
    Ok(out)
}

/// Bytes of one Xenos texture chain as [`crate::bundle::decode_layers`] lays it out: level 0 at its real size, levels 1.. at the
/// power-of-two size, each padded to 32x32 blocks, ending with the packed tail (the first level whose smaller side is 16
/// texels or less, padded like a level), all of it per layer.
fn chain_size(format: Format, width: u32, height: u32, mips: u32, layers: u32) -> Result<usize, Error> {
    // CTX1 / DXT3A-as-1111 (60, 61) have no decoder but take 8-byte 4x4 blocks: LeMans' stx holds one.
    let block = format.block().or(match format {
        Format::Other(60 | 61) => Some((4, 8)),
        _ => None,
    });
    let (bw, bpb) = block.ok_or_else(|| Error::Unsupported(format!("texture format {format:?}")))?;
    let (sw0, sh0) = (width.next_power_of_two(), height.next_power_of_two());
    let mut total = 0usize;
    for l in 0..mips {
        let (mut sw, mut sh) = ((sw0 >> l).max(1), (sh0 >> l).max(1));
        if l == 0 {
            (sw, sh) = (width, height);
        }
        // The packed tail (the first level whose smaller side is 16 texels or less) is one surface of its own size: a tall
        // 16x256 tail takes two tiles (Sebring's 64x1024 DXT5, 11 levels).
        let (bx, by) = (sw.div_ceil(bw), sh.div_ceil(bw));
        if sw.min(sh) <= 16 {
            total += (bx.next_multiple_of(32) * by.next_multiple_of(32) * bpb * layers) as usize;
            break;
        }
        total += (bx.next_multiple_of(32) * by.next_multiple_of(32) * bpb * layers) as usize;
    }
    Ok(total)
}

/// Base level of each face: one for 2D textures, six for cube maps. Colorado's cube maps (256x256 DXT1,
/// one level) store the six faces back to back, `gpu_len / 6` bytes each.
pub fn decode_faces(d: &[u8]) -> Result<(CaffTexture, Vec<Image>), Error> {
    let t = parse(d)?;
    decode_faces_of(d, t)
}

/// [`decode_faces`] for a texture whose descriptor is already known (an [`parse_stx`] entry, `gpu_offset` absolute in `d`).
pub fn decode_faces_of(d: &[u8], t: CaffTexture) -> Result<(CaffTexture, Vec<Image>), Error> {
    if !t.cube {
        let img = xds::untile(&d[t.gpu_offset..], t.format, t.width, t.height, t.width, true, t.endian)?;
        return Ok((t, vec![img]));
    }
    if t.mip_count > 1 {
        // Level-major: level 0 of the six faces first, each padded to whole tiles (the reflection cubes of FM3's `.stx.bin`).
        let levels = crate::bundle::decode_layers(&d[t.gpu_offset..], t.format, t.endian, t.width, t.height, 0, t.mip_count, 6)?;
        return Ok((t, levels.into_iter().next().unwrap_or_default()));
    }
    let face = t.gpu_len / 6;
    let faces = (0..6)
        .map(|i| xds::untile(&d[t.gpu_offset + i * face..], t.format, t.width, t.height, t.width, true, t.endian))
        .collect::<Result<_, _>>()?;
    Ok((t, faces))
}

/// Untile the base level of a 2D CAFF texture.
pub fn decode_base(d: &[u8]) -> Result<(CaffTexture, Image), Error> {
    let t = parse(d)?;
    if t.cube {
        return Err(Error::Unsupported("CAFF cube map".into()));
    }
    let img = xds::untile(&d[t.gpu_offset..], t.format, t.width, t.height, t.width, true, t.endian)?;
    Ok((t, img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_sizes_pad_to_tiles_and_end_in_one_tail_tile() {
        // 64x64 DXT1, 7 levels: 64x64, 32x32 (each one padded 32x32-block surface of 8-byte blocks) + the packed tail.
        assert_eq!(chain_size(Format::Dxt1, 64, 64, 7, 1).unwrap(), 3 * 8192);
        // 64x1024 DXT5, 11 levels: 64x1024 (32x256 padded blocks), 32x512 (32x128), then the 16x256 tail (32x64: two tiles).
        assert_eq!(chain_size(Format::Dxt5, 64, 1024, 11, 1).unwrap(), (32 * 256 + 32 * 128 + 32 * 64) * 16);
        // A single level at 256x256 is 64x64 blocks padded to 64 (already a multiple of 32).
        assert_eq!(chain_size(Format::Dxt1, 256, 256, 1, 6).unwrap(), 6 * 64 * 64 * 8);
        assert!(chain_size(Format::Other(1), 8, 8, 1, 1).is_err());
    }

    #[test]
    fn stx_needs_a_caff_header() {
        assert!(parse_stx(b"nope").is_err());
    }
}
