use anyhow::{bail, Context, Result};
use std::io::Cursor;
use std::path::Path;

use crate::oodle;
use crate::taff::{self, DBStreamHeader, TaffHeader};

#[derive(Debug, Clone)]
pub struct ZoneFile {
    pub path: std::path::PathBuf,
    pub taff_header: TaffHeader,
    pub zone_name: Option<String>,
    pub raw_data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DecompressedZone {
    pub zone_file: ZoneFile,
    pub data: Vec<u8>,
}

impl ZoneFile {
    pub fn open(path: &Path) -> Result<Self> {
        let raw_data =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;

        let taff_header =
            taff::parse_header(&raw_data).context("Failed to parse TAFF header")?;

        let zone_name = taff_header
            .fastfile_info()
            .ok()
            .map(|info| info.name)
            .filter(|n| !n.is_empty());

        Ok(ZoneFile {
            path: path.to_path_buf(),
            taff_header,
            zone_name,
            raw_data,
        })
    }

    pub fn decompress(&self, oodle_dll: &Path) -> Result<DecompressedZone> {
        let platform = self.taff_header.platform_data()?;
        if platform.encrypted {
            bail!("Encrypted zone files are not supported (need AES key)");
        }

        let size_data = self.taff_header.size_data()?;
        let expected_size = size_data.size as usize;

        let mut decompressed = Vec::with_capacity(expected_size);
        let mut offset = self.taff_header.header_end_offset;
        let data = &self.raw_data;

        loop {
            if offset + DBStreamHeader::SIZE > data.len() {
                break;
            }

            let mut cursor = Cursor::new(&data[offset..]);
            let block = DBStreamHeader::read(&mut cursor)?;

            if block.is_end() {
                break;
            }

            if block.offset as usize != offset {
                bail!(
                    "Block offset mismatch: header says {:#x} but we're at {:#x}",
                    block.offset,
                    offset
                );
            }

            if block.is_boundary_skip() {
                let boundary = taff::BLOCK_BOUNDARY;
                offset = ((offset / boundary) + 1) * boundary;
                continue;
            }

            let block_data_start = offset + DBStreamHeader::SIZE;
            let block_data_end = block_data_start + block.aligned_size as usize;

            if block_data_end > data.len() {
                bail!(
                    "Block at {:#x} extends past end of file: {:#x} > {:#x}",
                    offset,
                    block_data_end,
                    data.len()
                );
            }

            let compressed = &data[block_data_start..block_data_start + block.compressed_size as usize];
            let decomp_size = block.uncompressed_size as usize;

            if platform.compression == taff::COMP_UNCOMPRESSED
                || block.compressed_size == block.uncompressed_size
            {
                decompressed.extend_from_slice(compressed);
            } else {
                let block_data = oodle::decompress(oodle_dll, compressed, decomp_size)
                    .with_context(|| {
                        format!(
                            "Failed to decompress block at {:#x} ({} -> {} bytes)",
                            offset, block.compressed_size, block.uncompressed_size
                        )
                    })?;
                decompressed.extend_from_slice(&block_data);
            }

            offset = block_data_end;
        }

        if decompressed.is_empty() {
            bail!("No data blocks found in zone file");
        }

        Ok(DecompressedZone {
            zone_file: self.clone(),
            data: decompressed,
        })
    }

    pub fn file_size(&self) -> usize {
        self.raw_data.len()
    }

    pub fn block_count(&self) -> usize {
        let mut count = 0;
        let mut offset = self.taff_header.header_end_offset;
        let data = &self.raw_data;

        loop {
            if offset + DBStreamHeader::SIZE > data.len() {
                break;
            }
            let mut cursor = Cursor::new(&data[offset..]);
            let block = match DBStreamHeader::read(&mut cursor) {
                Ok(b) => b,
                Err(_) => break,
            };
            if block.is_end() {
                break;
            }
            if block.is_boundary_skip() {
                let boundary = taff::BLOCK_BOUNDARY;
                offset = ((offset / boundary) + 1) * boundary;
                continue;
            }
            count += 1;
            offset += DBStreamHeader::SIZE + block.aligned_size as usize;
        }
        count
    }
}

/// Recompress decompressed zone data back into a valid TAFF .ff file.
/// Preserves the original header chunks but updates sizes.
pub fn recompress_zone(
    original: &ZoneFile,
    decompressed_data: &[u8],
    oodle_dll: &Path,
) -> Result<Vec<u8>> {
    let platform = original.taff_header.platform_data()?;
    if platform.encrypted {
        bail!("Cannot recompress encrypted zone files");
    }

    let chunk_size: usize = 0x3FEE0; // ~260KB, same default as the game

    // Rebuild header with updated size
    let mut new_chunks = original.taff_header.chunks.clone();

    // Update SIZE_DATA chunk with new decompressed size
    if let Some(size_chunk) = new_chunks.iter_mut().find(|c| c.section_type == taff::ST_SIZE_DATA) {
        let mut new_data = size_chunk.data.clone();
        if new_data.len() >= 8 {
            new_data[..8].copy_from_slice(&(decompressed_data.len() as u64).to_le_bytes());
        }
        size_chunk.data = new_data;
    }

    let mut output = TaffHeader::write_header(&new_chunks);

    // Compress data in blocks
    let mut remaining = decompressed_data;
    let boundary = taff::BLOCK_BOUNDARY;

    while !remaining.is_empty() {
        let uncompressed_size = remaining.len().min(chunk_size);
        let chunk_data = &remaining[..uncompressed_size];

        let compressed = oodle::compress(
            oodle_dll,
            chunk_data,
            oodle::OodleCompressor::Kraken,
            oodle::OodleCompressionLevel::Normal,
        )
        .with_context(|| format!("Failed to compress block of {} bytes", uncompressed_size))?;

        let aligned_size = (compressed.len() + 3) & !3; // 4-byte align

        // Check if this block fits before the next boundary
        let block_start = output.len();
        let block_end = block_start + DBStreamHeader::SIZE + aligned_size;
        let next_boundary = ((block_start / boundary) + 1) * boundary;

        if block_end > next_boundary {
            // Write a boundary-skip marker: uncompressed_size=0, aligned_size!=0
            // aligned_size != 0 distinguishes this from the end marker (aligned_size == 0)
            let remaining_to_boundary = next_boundary - block_start - DBStreamHeader::SIZE;
            let header = DBStreamHeader {
                compressed_size: 0,
                uncompressed_size: 0,
                aligned_size: remaining_to_boundary as u32,
                offset: block_start as u32,
            };
            header.write(&mut output)?;
            let pad = next_boundary - output.len();
            output.extend(std::iter::repeat(0u8).take(pad));
            continue;
        }

        let header = DBStreamHeader {
            compressed_size: compressed.len() as u32,
            uncompressed_size: uncompressed_size as u32,
            aligned_size: aligned_size as u32,
            offset: block_start as u32,
        };
        header.write(&mut output)?;

        output.extend_from_slice(&compressed);
        // Pad to aligned size
        let pad = aligned_size - compressed.len();
        output.extend(std::iter::repeat(0u8).take(pad));

        remaining = &remaining[uncompressed_size..];
    }

    // End marker
    let end_offset = output.len();
    let end_header = DBStreamHeader {
        compressed_size: 0,
        uncompressed_size: 0,
        aligned_size: 0,
        offset: end_offset as u32,
    };
    end_header.write(&mut output)?;
    output.extend(std::iter::repeat(0u8).take(0x40)); // trailing padding

    Ok(output)
}

pub fn list_zone_files(zone_dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(zone_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "ff") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn categorize_zone(name: &str) -> &'static str {
    if name.starts_with("zm_") || name.contains("_zm_") {
        "zombies"
    } else if name.starts_with("mp_") || name.contains("_mp_") {
        "multiplayer"
    } else if name.starts_with("cp_") || name.contains("_cp_") {
        "campaign"
    } else if name.starts_with("core_") || name.contains("_core_") {
        "core"
    } else if name.starts_with("techset_") {
        "techset"
    } else if name.starts_with("en_") {
        "localization"
    } else if name.starts_with("ww_") || name.starts_with("1080_") || name.starts_with("4k_") {
        "resolution"
    } else {
        "other"
    }
}
