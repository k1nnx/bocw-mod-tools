use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use std::path::Path;

pub const XPAK_MAGIC: &[u8; 4] = b"KAPI";

#[derive(Debug, Clone)]
pub struct XpakHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub flags: u16,
    pub entry_count: u32,
    pub reserved: u32,
    pub sub_count: u32,
    pub sub_flags: u32,
    pub total_size: u64,
    pub hash: u64,
}

#[derive(Debug, Clone)]
pub struct XpakEntry {
    pub hash: u64,
    pub offset: u64,
    pub compressed_size: u64,
    pub decompressed_size: u64,
    pub sub_index: u32,
}

#[derive(Debug, Clone)]
pub struct XpakFile {
    pub path: std::path::PathBuf,
    pub header: XpakHeader,
    pub entries: Vec<XpakEntry>,
}

impl XpakFile {
    pub fn open(path: &Path) -> Result<Self> {
        let data =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;

        let header = parse_header(&data)?;
        let entries = parse_entries(&data, &header)?;

        Ok(XpakFile {
            path: path.to_path_buf(),
            header,
            entries,
        })
    }

    pub fn xsub_paths(&self) -> Vec<std::path::PathBuf> {
        let stem = self.path.file_stem().unwrap().to_string_lossy();
        let parent = self.path.parent().unwrap();

        (0..self.header.sub_count)
            .map(|i| parent.join(format!("{}-{:05}.xsub", stem, i)))
            .collect()
    }
}

fn parse_header(data: &[u8]) -> Result<XpakHeader> {
    if data.len() < 48 {
        bail!("File too small for XPAK header");
    }

    let mut cursor = Cursor::new(data);

    let mut magic = [0u8; 4];
    cursor.read_exact(&mut magic)?;
    if &magic != XPAK_MAGIC {
        bail!("Invalid XPAK magic: {:?}", magic);
    }

    let version = cursor.read_u16::<LittleEndian>()?;
    let flags = cursor.read_u16::<LittleEndian>()?;
    let entry_count = cursor.read_u32::<LittleEndian>()?;
    let reserved = cursor.read_u32::<LittleEndian>()?;
    let sub_count = cursor.read_u32::<LittleEndian>()?;
    let sub_flags = cursor.read_u32::<LittleEndian>()?;
    let total_size = cursor.read_u64::<LittleEndian>()?;
    let hash = cursor.read_u64::<LittleEndian>()?;

    Ok(XpakHeader {
        magic,
        version,
        flags,
        entry_count,
        reserved,
        sub_count,
        sub_flags,
        total_size,
        hash,
    })
}

fn parse_entries(data: &[u8], header: &XpakHeader) -> Result<Vec<XpakEntry>> {
    let mut entries = Vec::with_capacity(header.entry_count as usize);
    let entry_start = 0x80;
    let entry_size = 40;

    for i in 0..header.entry_count {
        let offset = entry_start + (i as usize) * entry_size;
        if offset + entry_size > data.len() {
            break;
        }

        let mut cursor = Cursor::new(&data[offset..]);
        let hash = cursor.read_u64::<LittleEndian>()?;
        let file_offset = cursor.read_u64::<LittleEndian>()?;
        let compressed_size = cursor.read_u64::<LittleEndian>()?;
        let decompressed_size = cursor.read_u64::<LittleEndian>()?;
        let sub_index = cursor.read_u32::<LittleEndian>()?;

        entries.push(XpakEntry {
            hash,
            offset: file_offset,
            compressed_size,
            decompressed_size,
            sub_index,
        });
    }

    Ok(entries)
}
