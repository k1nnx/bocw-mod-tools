use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read};

pub const TAFF_MAGIC: u32 = 0x46464154; // "TAFF"
pub const CW_VERSION: u32 = 0x64;

// Section type identifiers (chunk tags in the flexible header)
pub const ST_VERSION: u32 = 0x39CB44F1;
pub const ST_PLATFORM_DATA: u32 = 0x2C2381CF;
pub const ST_BUILD_DATA: u32 = 0x37F9D612;
pub const ST_BLOCK_SIZES: u32 = 0x66C65056;
pub const ST_SIZE_DATA: u32 = 0x0F992DFE;
pub const ST_FASTFILE_INFO: u32 = 0xC7708CDA;
pub const ST_LINK_RESULT: u32 = 0x8578C004;

// Compression types
pub const COMP_UNCOMPRESSED: u8 = 0x0;
pub const COMP_OODLE_KRAKEN: u8 = 0x8;
pub const COMP_OODLE_MERMAID: u8 = 0x9;
pub const COMP_OODLE_SELKIE: u8 = 0xA;

// Platform types
pub const PLATFORM_PC: u8 = 0x0;

// Alignment boundary for compressed data blocks
pub const BLOCK_BOUNDARY: usize = 0x800000;

#[derive(Debug, Clone)]
pub struct FlexChunk {
    pub section_type: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TaffHeader {
    pub magic: u32,
    pub chunks: Vec<FlexChunk>,
    pub header_end_offset: usize,
}

#[derive(Debug, Clone)]
pub struct PlatformData {
    pub server: bool,
    pub compression: u8,
    pub platform: u8,
    pub encrypted: bool,
}

#[derive(Debug, Clone)]
pub struct SizeData {
    pub size: u64,
    pub external_size: u64,
    pub mem_mapped_offset: u64,
}

#[derive(Debug, Clone)]
pub struct FastFileInfo {
    pub name: String,
    pub signature: [u8; 0x100],
    pub aes_iv: [u8; 0x10],
}

#[derive(Debug, Clone)]
pub struct BuildData {
    pub timestamp: u64,
    pub changelist: u32,
    pub archive_checksum: [u32; 4],
    pub builder_name: String,
}

#[derive(Debug, Clone, Copy)]
pub struct DBStreamHeader {
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub aligned_size: u32,
    pub offset: u32,
}

impl DBStreamHeader {
    pub const SIZE: usize = 16;

    pub fn read(cursor: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            compressed_size: cursor.read_u32::<LittleEndian>()?,
            uncompressed_size: cursor.read_u32::<LittleEndian>()?,
            aligned_size: cursor.read_u32::<LittleEndian>()?,
            offset: cursor.read_u32::<LittleEndian>()?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) -> Result<()> {
        out.write_u32::<LittleEndian>(self.compressed_size)?;
        out.write_u32::<LittleEndian>(self.uncompressed_size)?;
        out.write_u32::<LittleEndian>(self.aligned_size)?;
        out.write_u32::<LittleEndian>(self.offset)?;
        Ok(())
    }

    pub fn is_end(&self) -> bool {
        self.aligned_size == 0
    }

    pub fn is_boundary_skip(&self) -> bool {
        self.uncompressed_size == 0 && self.aligned_size != 0
    }
}

pub fn parse_header(data: &[u8]) -> Result<TaffHeader> {
    if data.len() < 8 {
        bail!("File too small for TAFF header");
    }

    let mut cursor = Cursor::new(data);

    let magic = cursor.read_u32::<LittleEndian>()?;
    if magic != TAFF_MAGIC {
        bail!("Invalid TAFF magic: {:#x} (expected {:#x})", magic, TAFF_MAGIC);
    }

    let chunk_count = cursor.read_u32::<LittleEndian>()? as usize;
    if chunk_count > 16 {
        bail!("Too many TAFF chunks: {} (max 16)", chunk_count);
    }

    let mut chunks = Vec::with_capacity(chunk_count);

    for i in 0..chunk_count {
        let section_type = cursor
            .read_u32::<LittleEndian>()
            .with_context(|| format!("Failed to read chunk {} type", i))?;
        let size = cursor
            .read_u32::<LittleEndian>()
            .with_context(|| format!("Failed to read chunk {} size", i))? as usize;

        let mut chunk_data = vec![0u8; size];
        cursor
            .read_exact(&mut chunk_data)
            .with_context(|| format!("Failed to read chunk {} data ({} bytes)", i, size))?;

        chunks.push(FlexChunk {
            section_type,
            data: chunk_data,
        });
    }

    let header_end_offset = cursor.position() as usize;

    Ok(TaffHeader {
        magic,
        chunks,
        header_end_offset,
    })
}

impl TaffHeader {
    pub fn find_chunk(&self, section_type: u32) -> Option<&FlexChunk> {
        self.chunks.iter().find(|c| c.section_type == section_type)
    }

    pub fn version(&self) -> Result<u32> {
        let chunk = self
            .find_chunk(ST_VERSION)
            .context("Missing VERSION chunk")?;
        if chunk.data.len() < 4 {
            bail!("VERSION chunk too small");
        }
        Ok(u32::from_le_bytes(chunk.data[..4].try_into().unwrap()))
    }

    pub fn platform_data(&self) -> Result<PlatformData> {
        let chunk = self
            .find_chunk(ST_PLATFORM_DATA)
            .context("Missing PLATFORM_DATA chunk")?;
        if chunk.data.len() < 4 {
            bail!("PLATFORM_DATA chunk too small");
        }
        Ok(PlatformData {
            server: chunk.data[0] != 0,
            compression: chunk.data[1],
            platform: chunk.data[2],
            encrypted: chunk.data[3] != 0,
        })
    }

    pub fn size_data(&self) -> Result<SizeData> {
        let chunk = self
            .find_chunk(ST_SIZE_DATA)
            .context("Missing SIZE_DATA chunk")?;
        if chunk.data.len() < 24 {
            bail!("SIZE_DATA chunk too small");
        }
        let mut c = Cursor::new(chunk.data.as_slice());
        Ok(SizeData {
            size: c.read_u64::<LittleEndian>()?,
            external_size: c.read_u64::<LittleEndian>()?,
            mem_mapped_offset: c.read_u64::<LittleEndian>()?,
        })
    }

    pub fn block_sizes(&self) -> Result<Vec<u64>> {
        let chunk = self
            .find_chunk(ST_BLOCK_SIZES)
            .context("Missing BLOCK_SIZES chunk")?;
        let count = chunk.data.len() / 8;
        let mut sizes = Vec::with_capacity(count);
        let mut c = Cursor::new(chunk.data.as_slice());
        for _ in 0..count {
            sizes.push(c.read_u64::<LittleEndian>()?);
        }
        Ok(sizes)
    }

    pub fn fastfile_info(&self) -> Result<FastFileInfo> {
        let chunk = self
            .find_chunk(ST_FASTFILE_INFO)
            .context("Missing FASTFILE_INFO chunk")?;
        if chunk.data.len() < 0x150 {
            bail!("FASTFILE_INFO chunk too small: {} bytes", chunk.data.len());
        }

        let name_end = chunk.data[..0x40]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(0x40);
        let name = String::from_utf8_lossy(&chunk.data[..name_end]).to_string();

        let mut signature = [0u8; 0x100];
        signature.copy_from_slice(&chunk.data[0x40..0x140]);

        let mut aes_iv = [0u8; 0x10];
        aes_iv.copy_from_slice(&chunk.data[0x140..0x150]);

        Ok(FastFileInfo {
            name,
            signature,
            aes_iv,
        })
    }

    pub fn build_data(&self) -> Result<BuildData> {
        let chunk = self
            .find_chunk(ST_BUILD_DATA)
            .context("Missing BUILD_DATA chunk")?;
        if chunk.data.len() < 0x40 {
            bail!("BUILD_DATA chunk too small");
        }
        let mut c = Cursor::new(chunk.data.as_slice());
        let timestamp = c.read_u64::<LittleEndian>()?;
        let changelist = c.read_u32::<LittleEndian>()?;
        let mut archive_checksum = [0u32; 4];
        for cs in &mut archive_checksum {
            *cs = c.read_u32::<LittleEndian>()?;
        }
        let mut builder_buf = [0u8; 32];
        c.read_exact(&mut builder_buf)?;
        let end = builder_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(32);
        let builder_name = String::from_utf8_lossy(&builder_buf[..end]).to_string();

        Ok(BuildData {
            timestamp,
            changelist,
            archive_checksum,
            builder_name,
        })
    }

    pub fn compression_name(&self) -> &'static str {
        match self.platform_data().map(|p| p.compression) {
            Ok(COMP_UNCOMPRESSED) => "none",
            Ok(COMP_OODLE_KRAKEN) => "oodle_kraken",
            Ok(COMP_OODLE_MERMAID) => "oodle_mermaid",
            Ok(COMP_OODLE_SELKIE) => "oodle_selkie",
            Ok(c) => match c {
                1 | 2 => "zlib",
                3 | 4 => "lz4",
                _ => "unknown",
            },
            Err(_) => "unknown",
        }
    }

    pub fn write_header(chunks: &[FlexChunk]) -> Vec<u8> {
        let mut out = Vec::new();
        out.write_u32::<LittleEndian>(TAFF_MAGIC).unwrap();
        out.write_u32::<LittleEndian>(chunks.len() as u32).unwrap();

        for chunk in chunks {
            out.write_u32::<LittleEndian>(chunk.section_type).unwrap();
            out.write_u32::<LittleEndian>(chunk.data.len() as u32)
                .unwrap();
            out.extend_from_slice(&chunk.data);
        }

        out
    }
}
