//! FM6 (Xbox One) `burG` container: the wrapper of `.modelbin`, `.swatchbin` and `.materialbin`.
//!
//! Layout, read from the IDx files and checked on all of them (docs/DESIGN.md "FM6 IDx"):
//! - header: `"burG"`, u16 0x0101 version, u16 0, u32 data start, u32 file size, u32 chunk count (FM6); or (FM5, and
//!   FM6's generic `media/cars/wheel.modelbin`) u16 version 1, u16 chunk count, u32 data start, u32 file size, with the
//!   chunk table at 0x10 instead of 0x14;
//! - then one 24-byte entry per chunk: 4-byte tag (stored reversed, so `"lekS"` is `Skel`), u32 version,
//!   u32 metadata offset, u32 data offset, u32 size, u32 size again (equal on every file seen);
//! - a metadata block per chunk sits between the table and the data start; a chunk's metadata runs from its
//!   offset to the next chunk's offset (the last one to the data start), so it may be empty;
//! - the data blocks follow, back to back, each padded to 4 bytes; the file ends with the last block, either exactly or after its padding.
//!
//! What is inside each chunk (vertices, pixels, shader names) is not decoded here.

use crate::Error;

pub const MAGIC: &[u8; 4] = b"burG";

#[derive(Debug, Clone)]
pub struct Chunk {
    /// The tag as it reads normally (`Skel`, `Mesh`, `TXCB`...).
    pub tag: String,
    pub version: u32,
    pub meta_offset: u32,
    pub meta_len: u32,
    pub data_offset: u32,
    pub size: u32,
    /// Second size field; equals `size` on every file seen so far.
    pub size2: u32,
}

#[derive(Debug, Clone)]
pub struct Grub<'a> {
    pub version: u16,
    pub data_start: u32,
    pub file_size: u32,
    pub chunks: Vec<Chunk>,
    data: &'a [u8],
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, Error> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or(Error::Truncated("burG header"))
}

pub fn parse(data: &[u8]) -> Result<Grub<'_>, Error> {
    if data.get(..4) != Some(MAGIC) {
        return Err(Error::BadMagic("burG"));
    }
    let version = u16::from_le_bytes([data[4], data[5]]);
    let data_start = u32_at(data, 8)?;
    let file_size = u32_at(data, 12)?;
    // version 1 (FM5): u16 count at 6, table at 0x10; version 0x0101 (FM6): u32 count at 16, table at 0x14
    let (count, table) = if version == 1 { (u16::from_le_bytes([data[6], data[7]]) as usize, 0x10) } else { (u32_at(data, 16)? as usize, 0x14) };
    if table + count * 24 > data.len() {
        return Err(Error::Truncated("burG chunk table"));
    }
    if file_size as usize != data.len() {
        return Err(Error::SizeMismatch { expected: file_size as usize, got: data.len() });
    }
    let mut chunks: Vec<Chunk> = Vec::with_capacity(count);
    for i in 0..count {
        let o = table + i * 24;
        let tag: Vec<u8> = data.get(o..o + 4).ok_or(Error::Truncated("burG chunk table"))?.iter().rev().copied().collect();
        chunks.push(Chunk {
            tag: String::from_utf8_lossy(&tag).into_owned(),
            version: u32_at(data, o + 4)?,
            meta_offset: u32_at(data, o + 8)?,
            meta_len: 0,
            data_offset: u32_at(data, o + 12)?,
            size: u32_at(data, o + 16)?,
            size2: u32_at(data, o + 20)?,
        });
    }
    for i in 0..chunks.len() {
        let end = chunks.get(i + 1).map_or(data_start, |c| c.meta_offset);
        chunks[i].meta_len = end.saturating_sub(chunks[i].meta_offset);
    }
    Ok(Grub { version, data_start, file_size, chunks, data })
}

impl<'a> Grub<'a> {
    pub fn data_of(&self, c: &Chunk) -> Result<&'a [u8], Error> {
        let (o, n) = (c.data_offset as usize, c.size as usize);
        self.data.get(o..o + n).ok_or(Error::Truncated("burG chunk data"))
    }

    pub fn meta_of(&self, c: &Chunk) -> Result<&'a [u8], Error> {
        let (o, n) = (c.meta_offset as usize, c.meta_len as usize);
        self.data.get(o..o + n).ok_or(Error::Truncated("burG chunk metadata"))
    }

    /// Checks the layout rules above; returns a list of what does not hold (empty = all hold).
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        let table_end = if self.version == 1 { 0x10 } else { 0x14 } + self.chunks.len() * 24;
        if self.chunks.first().is_some_and(|c| c.data_offset != self.data_start) {
            p.push(format!("first data offset {:#x} != data start {:#x}", self.chunks[0].data_offset, self.data_start));
        }
        if self.chunks.first().is_some_and(|c| c.meta_offset as usize != table_end) {
            p.push(format!("first metadata offset {:#x} != table end {table_end:#x}", self.chunks[0].meta_offset));
        }
        let mut next = self.data_start;
        let mut end = self.data_start;
        for c in &self.chunks {
            if c.size != c.size2 {
                p.push(format!("{}: size {} != size2 {}", c.tag, c.size, c.size2));
            }
            if c.data_offset != next {
                p.push(format!("{}: data at {:#x}, expected {:#x}", c.tag, c.data_offset, next));
            }
            end = c.data_offset + c.size;
            next = (end + 3) & !3;
        }
        if self.file_size < end || self.file_size > next {
            p.push(format!("data ends at {end:#x}, file is {:#x}", self.file_size));
        }
        p
    }
}
