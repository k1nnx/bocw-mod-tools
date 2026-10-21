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

/// Compresses and decompresses individual data blocks. [`OodleCodec`] calls the
/// game's Oodle DLL; tests substitute their own codec.
pub trait BlockCodec {
    fn decompress(&self, src: &[u8], dst_len: usize) -> Result<Vec<u8>>;
    /// `compression` is the TAFF compression byte from PLATFORM_DATA.
    fn compress(&self, src: &[u8], compression: u8) -> Result<Vec<u8>>;
}

pub struct OodleCodec<'a> {
    pub dll: &'a Path,
}

impl<'a> OodleCodec<'a> {
    pub fn new(dll: &'a Path) -> Self {
        Self { dll }
    }
}

impl BlockCodec for OodleCodec<'_> {
    fn decompress(&self, src: &[u8], dst_len: usize) -> Result<Vec<u8>> {
        oodle::decompress(self.dll, src, dst_len)
    }

    fn compress(&self, src: &[u8], compression: u8) -> Result<Vec<u8>> {
        let compressor = match compression {
            taff::COMP_OODLE_KRAKEN => oodle::OodleCompressor::Kraken,
            taff::COMP_OODLE_MERMAID => oodle::OodleCompressor::Mermaid,
            taff::COMP_OODLE_SELKIE => oodle::OodleCompressor::Selkie,
            other => bail!("No Oodle compressor for TAFF compression {:#x}", other),
        };
        oodle::compress(self.dll, src, compressor, oodle::OodleCompressionLevel::Normal)
    }
}

/// How data blocks are laid out in the file.
#[derive(Debug, Clone, Copy)]
pub struct BlockLayout {
    /// Uncompressed bytes per block when writing.
    pub block_size: usize,
    /// Block headers never straddle a multiple of this offset.
    pub boundary: usize,
}

impl Default for BlockLayout {
    fn default() -> Self {
        Self {
            block_size: 0x3FEE0, // ~260KB, same default as the game
            boundary: taff::BLOCK_BOUNDARY,
        }
    }
}

fn is_supported_compression(compression: u8) -> bool {
    matches!(
        compression,
        taff::COMP_UNCOMPRESSED
            | taff::COMP_OODLE_KRAKEN
            | taff::COMP_OODLE_MERMAID
            | taff::COMP_OODLE_SELKIE
    )
}

fn next_boundary(offset: usize, boundary: usize) -> usize {
    ((offset / boundary) + 1) * boundary
}

impl ZoneFile {
    pub fn open(path: &Path) -> Result<Self> {
        let raw_data =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        Self::from_bytes(path, raw_data)
    }

    pub fn from_bytes(path: &Path, raw_data: Vec<u8>) -> Result<Self> {
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

    /// Data block headers in file order, with the offset each was read from.
    /// Boundary-skip markers are followed but not returned.
    pub fn blocks(&self, layout: BlockLayout) -> Result<Vec<(usize, DBStreamHeader)>> {
        let data = &self.raw_data;
        let mut offset = self.taff_header.header_end_offset;
        let mut blocks = Vec::new();

        loop {
            // Tolerate files that end without an explicit end marker.
            if offset + DBStreamHeader::SIZE > data.len() {
                return Ok(blocks);
            }

            let block = DBStreamHeader::read(&mut Cursor::new(&data[offset..]))?;
            if block.is_end() {
                return Ok(blocks);
            }

            if block.offset as usize != offset {
                bail!(
                    "Block offset mismatch: header says {:#x} but we're at {:#x}",
                    block.offset,
                    offset
                );
            }

            if block.is_boundary_skip() {
                offset = next_boundary(offset, layout.boundary);
                continue;
            }

            if block.compressed_size > block.aligned_size {
                bail!(
                    "Block at {:#x}: compressed size {:#x} exceeds aligned size {:#x}",
                    offset,
                    block.compressed_size,
                    block.aligned_size
                );
            }

            let block_data_end = offset + DBStreamHeader::SIZE + block.aligned_size as usize;
            if block_data_end > data.len() {
                bail!(
                    "Block at {:#x} extends past end of file: {:#x} > {:#x}",
                    offset,
                    block_data_end,
                    data.len()
                );
            }

            blocks.push((offset, block));
            offset = block_data_end;
        }
    }

    pub fn decompress(&self, oodle_dll: &Path) -> Result<DecompressedZone> {
        self.decompress_with(&OodleCodec::new(oodle_dll), BlockLayout::default())
    }

    pub fn decompress_with(&self, codec: &dyn BlockCodec, layout: BlockLayout) -> Result<DecompressedZone> {
        let platform = self.taff_header.platform_data()?;
        if platform.encrypted {
            bail!("Encrypted zone files are not supported (need AES key)");
        }
        if !is_supported_compression(platform.compression) {
            bail!("Unsupported zone compression: {}", self.taff_header.compression_name());
        }

        let size_data = self.taff_header.size_data()?;
        let mut decompressed = Vec::with_capacity(size_data.size as usize);

        for (offset, block) in self.blocks(layout)? {
            let start = offset + DBStreamHeader::SIZE;
            let compressed = &self.raw_data[start..start + block.compressed_size as usize];
            let decomp_size = block.uncompressed_size as usize;

            // Equal sizes mark a block stored without compression.
            if platform.compression == taff::COMP_UNCOMPRESSED
                || block.compressed_size == block.uncompressed_size
            {
                decompressed.extend_from_slice(compressed);
                continue;
            }

            let block_data = codec.decompress(compressed, decomp_size).with_context(|| {
                format!(
                    "Failed to decompress block at {:#x} ({} -> {} bytes)",
                    offset, block.compressed_size, block.uncompressed_size
                )
            })?;
            if block_data.len() != decomp_size {
                bail!(
                    "Block at {:#x} decompressed to {} bytes, header says {}",
                    offset,
                    block_data.len(),
                    decomp_size
                );
            }
            decompressed.extend_from_slice(&block_data);
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

    /// Number of data blocks; 0 if the block chain is malformed.
    pub fn block_count(&self) -> usize {
        self.blocks(BlockLayout::default()).map_or(0, |b| b.len())
    }
}

/// Recompress decompressed zone data back into a valid TAFF .ff file.
/// Preserves the original header chunks but updates sizes.
pub fn recompress_zone(
    original: &ZoneFile,
    decompressed_data: &[u8],
    oodle_dll: &Path,
) -> Result<Vec<u8>> {
    recompress_zone_with(
        original,
        decompressed_data,
        &OodleCodec::new(oodle_dll),
        BlockLayout::default(),
    )
}

pub fn recompress_zone_with(
    original: &ZoneFile,
    decompressed_data: &[u8],
    codec: &dyn BlockCodec,
    layout: BlockLayout,
) -> Result<Vec<u8>> {
    let platform = original.taff_header.platform_data()?;
    if platform.encrypted {
        bail!("Cannot recompress encrypted zone files");
    }
    if !is_supported_compression(platform.compression) {
        bail!("Cannot recompress {} zone files", original.taff_header.compression_name());
    }

    const HEADER: usize = DBStreamHeader::SIZE;
    // Room every block leaves before the next boundary: enough for an end
    // marker, or for a skip marker whose aligned_size is non-zero (a zero
    // aligned_size would read as the end marker).
    const RESERVE: usize = HEADER + 4;
    let max_aligned = (layout.block_size + 3) & !3;
    if HEADER + max_aligned + RESERVE > layout.boundary {
        bail!(
            "Block size {:#x} does not fit within the {:#x} boundary",
            layout.block_size,
            layout.boundary
        );
    }

    // Rebuild header with updated size
    let mut new_chunks = original.taff_header.chunks.clone();

    // Update SIZE_DATA chunk with new decompressed size
    if let Some(size_chunk) = new_chunks.iter_mut().find(|c| c.section_type == taff::ST_SIZE_DATA) {
        if size_chunk.data.len() >= 8 {
            size_chunk.data[..8].copy_from_slice(&(decompressed_data.len() as u64).to_le_bytes());
        }
    }

    let mut output = TaffHeader::write_header(&new_chunks);
    if output.len() + RESERVE > next_boundary(output.len(), layout.boundary) {
        bail!("TAFF header leaves no room for a block header before the first boundary");
    }

    for chunk_data in decompressed_data.chunks(layout.block_size) {
        let payload = if platform.compression == taff::COMP_UNCOMPRESSED {
            chunk_data.to_vec()
        } else {
            let compressed = codec
                .compress(chunk_data, platform.compression)
                .with_context(|| format!("Failed to compress block of {} bytes", chunk_data.len()))?;
            // The reader treats equal sizes as a stored block, so never keep a
            // codec result that is not smaller than its input.
            if compressed.len() >= chunk_data.len() {
                chunk_data.to_vec()
            } else {
                compressed
            }
        };

        let aligned_size = (payload.len() + 3) & !3; // 4-byte align

        // Keep the reserve free so the next header never straddles the boundary.
        let mut block_start = output.len();
        let boundary = next_boundary(block_start, layout.boundary);
        if block_start + HEADER + aligned_size + RESERVE > boundary {
            // Write a boundary-skip marker: uncompressed_size=0, aligned_size!=0
            // aligned_size != 0 distinguishes this from the end marker (aligned_size == 0)
            DBStreamHeader {
                compressed_size: 0,
                uncompressed_size: 0,
                aligned_size: (boundary - block_start - HEADER) as u32,
                offset: block_start as u32,
            }
            .write(&mut output)?;
            output.resize(boundary, 0);
            block_start = boundary;
        }

        DBStreamHeader {
            compressed_size: payload.len() as u32,
            uncompressed_size: chunk_data.len() as u32,
            aligned_size: aligned_size as u32,
            offset: block_start as u32,
        }
        .write(&mut output)?;
        output.extend_from_slice(&payload);
        // Pad to aligned size
        output.resize(block_start + HEADER + aligned_size, 0);
    }

    // End marker
    let end_offset = output.len();
    DBStreamHeader {
        compressed_size: 0,
        uncompressed_size: 0,
        aligned_size: 0,
        offset: end_offset as u32,
    }
    .write(&mut output)?;
    output.resize(output.len() + 0x40, 0); // trailing padding

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch;
    use crate::taff::FlexChunk;

    /// Toy run-length codec: (count, byte) pairs. Small for runs, larger for noise.
    struct RleCodec;

    impl BlockCodec for RleCodec {
        fn decompress(&self, src: &[u8], dst_len: usize) -> Result<Vec<u8>> {
            let mut out = Vec::with_capacity(dst_len);
            for pair in src.chunks(2) {
                let [count, byte] = pair else { bail!("odd RLE stream") };
                out.extend(std::iter::repeat_n(*byte, *count as usize));
            }
            Ok(out)
        }

        fn compress(&self, src: &[u8], _compression: u8) -> Result<Vec<u8>> {
            let mut out = Vec::new();
            let mut iter = src.iter().peekable();
            while let Some(&byte) = iter.next() {
                let mut count = 1u8;
                while count < u8::MAX && iter.peek() == Some(&&byte) {
                    iter.next();
                    count += 1;
                }
                out.extend_from_slice(&[count, byte]);
            }
            Ok(out)
        }
    }

    const SMALL: BlockLayout = BlockLayout {
        block_size: 64,
        boundary: 512,
    };

    fn header_chunks(compression: u8, size: u64) -> Vec<FlexChunk> {
        let mut info = vec![0u8; 0x150];
        info[..9].copy_from_slice(b"test_zone");
        let mut size_data = size.to_le_bytes().to_vec();
        size_data.resize(24, 0);
        vec![
            FlexChunk { section_type: taff::ST_VERSION, data: taff::CW_VERSION.to_le_bytes().to_vec() },
            FlexChunk { section_type: taff::ST_PLATFORM_DATA, data: vec![0, compression, taff::PLATFORM_PC, 0] },
            FlexChunk { section_type: taff::ST_SIZE_DATA, data: size_data },
            FlexChunk { section_type: taff::ST_FASTFILE_INFO, data: info },
        ]
    }

    fn header_only(compression: u8) -> ZoneFile {
        let raw = TaffHeader::write_header(&header_chunks(compression, 0));
        ZoneFile::from_bytes(Path::new("test_zone.ff"), raw).unwrap()
    }

    /// Runs (compressible) interleaved with noise (stored fallback), plus a patch target.
    fn sample_data() -> Vec<u8> {
        let mut data = Vec::new();
        let mut seed = 0x1234_5678u32;
        for i in 0..40 {
            data.extend(std::iter::repeat_n(b'A' + (i % 26) as u8, 50));
            for _ in 0..30 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                data.push((seed >> 16) as u8);
            }
        }
        data.extend_from_slice(b"..weapon_rarity_map_list..");
        data
    }

    fn build(compression: u8, data: &[u8], layout: BlockLayout) -> ZoneFile {
        let bytes = recompress_zone_with(&header_only(compression), data, &RleCodec, layout).unwrap();
        ZoneFile::from_bytes(Path::new("test_zone.ff"), bytes).unwrap()
    }

    #[test]
    fn header_round_trip() {
        let zone = build(taff::COMP_OODLE_KRAKEN, &sample_data(), SMALL);
        let header = &zone.taff_header;
        assert_eq!(header.version().unwrap(), taff::CW_VERSION);
        assert_eq!(header.compression_name(), "oodle_kraken");
        assert_eq!(header.size_data().unwrap().size, sample_data().len() as u64);
        assert_eq!(zone.zone_name.as_deref(), Some("test_zone"));

        let rewritten = TaffHeader::write_header(&header.chunks);
        assert_eq!(rewritten, zone.raw_data[..header.header_end_offset]);
    }

    #[test]
    fn compressed_round_trip_with_boundaries() {
        let data = sample_data();
        let zone = build(taff::COMP_OODLE_KRAKEN, &data, SMALL);
        assert_eq!(zone.decompress_with(&RleCodec, SMALL).unwrap().data, data);

        let blocks = zone.blocks(SMALL).unwrap();
        assert_eq!(blocks.len(), data.len().div_ceil(SMALL.block_size));
        assert!(blocks.iter().any(|(_, b)| b.compressed_size < b.uncompressed_size));
        assert!(blocks.iter().any(|(_, b)| b.compressed_size == b.uncompressed_size));

        // Data spans several boundaries, and no header straddles one.
        assert!(zone.file_size() > 3 * SMALL.boundary);
        assert_eq!(
            blocks.iter().map(|(_, b)| b.uncompressed_size as usize).sum::<usize>(),
            data.len()
        );
        for (offset, block) in &blocks {
            assert_eq!(block.offset as usize, *offset);
            assert_eq!(offset / SMALL.boundary, (offset + DBStreamHeader::SIZE - 1) / SMALL.boundary);
        }
    }

    #[test]
    fn uncompressed_zone_stays_uncompressed() {
        let data = sample_data();
        let zone = build(taff::COMP_UNCOMPRESSED, &data, SMALL);
        for (_, block) in zone.blocks(SMALL).unwrap() {
            assert_eq!(block.compressed_size, block.uncompressed_size);
        }
        // No codec is involved for an uncompressed zone.
        struct NoCodec;
        impl BlockCodec for NoCodec {
            fn decompress(&self, _: &[u8], _: usize) -> Result<Vec<u8>> { bail!("unexpected") }
            fn compress(&self, _: &[u8], _: u8) -> Result<Vec<u8>> { bail!("unexpected") }
        }
        assert_eq!(zone.decompress_with(&NoCodec, SMALL).unwrap().data, data);
    }

    #[test]
    fn every_alignment_near_a_boundary_round_trips() {
        let layout = BlockLayout { block_size: 60, boundary: 256 };
        for compression in [taff::COMP_UNCOMPRESSED, taff::COMP_OODLE_KRAKEN] {
            for len in 1..700 {
                let data: Vec<u8> = (0..len).map(|i| (i * 7 / 5) as u8).collect();
                let zone = build(compression, &data, layout);
                let out = zone.decompress_with(&RleCodec, layout).unwrap().data;
                assert_eq!(out, data, "compression {compression:#x}, len {len}");
            }
        }
    }

    #[test]
    fn default_layout_round_trip() {
        // Larger than one default block, so the real block size is exercised.
        let data: Vec<u8> = (0..BlockLayout::default().block_size * 2 + 100)
            .map(|i| (i / 300) as u8)
            .collect();
        let zone = build(taff::COMP_OODLE_KRAKEN, &data, BlockLayout::default());
        assert_eq!(zone.block_count(), 3);
        assert_eq!(zone.decompress_with(&RleCodec, BlockLayout::default()).unwrap().data, data);
    }

    #[test]
    fn patched_zone_round_trip() {
        let zone = build(taff::COMP_OODLE_KRAKEN, &sample_data(), SMALL);
        let mut data = zone.decompress_with(&RleCodec, SMALL).unwrap().data;
        let patch = patch::StringPatch {
            search: "weapon_rarity_map_list".into(),
            replace: "weapon_rarity_map_noop".into(),
            description: String::new(),
            max_occurrences: None,
        };
        assert_eq!(patch::apply_string_patch(&mut data, &patch).unwrap(), 1);

        let bytes = recompress_zone_with(&zone, &data, &RleCodec, SMALL).unwrap();
        let patched = ZoneFile::from_bytes(Path::new("patched.ff"), bytes).unwrap();
        let round_trip = patched.decompress_with(&RleCodec, SMALL).unwrap().data;
        assert_eq!(round_trip, data);
        assert!(patch::find_string(&round_trip, "weapon_rarity_map_list").is_empty());
        assert_eq!(patch::find_string(&round_trip, "weapon_rarity_map_noop").len(), 1);
    }

    #[test]
    fn corrupt_block_chains_are_rejected() {
        let zone = build(taff::COMP_OODLE_KRAKEN, &sample_data(), SMALL);
        let (first, _) = zone.blocks(SMALL).unwrap()[0];

        let mut bad_offset = zone.raw_data.clone();
        bad_offset[first + 12..first + 16].copy_from_slice(&0xDEADu32.to_le_bytes());
        let bad = ZoneFile::from_bytes(Path::new("bad.ff"), bad_offset).unwrap();
        assert!(bad.decompress_with(&RleCodec, SMALL).is_err());
        assert_eq!(bad.block_count(), 0);

        let mut oversized = zone.raw_data.clone();
        oversized[first..first + 4].copy_from_slice(&0xFFFFu32.to_le_bytes());
        let bad = ZoneFile::from_bytes(Path::new("bad.ff"), oversized).unwrap();
        assert!(bad.decompress_with(&RleCodec, SMALL).is_err());

        let truncated = zone.raw_data[..first + 40].to_vec();
        let bad = ZoneFile::from_bytes(Path::new("bad.ff"), truncated).unwrap();
        assert!(bad.decompress_with(&RleCodec, SMALL).is_err());
    }

    #[test]
    fn unsupported_zones_are_refused() {
        let zlib = header_only(1);
        assert!(recompress_zone_with(&zlib, b"data", &RleCodec, SMALL).is_err());

        let mut chunks = header_chunks(taff::COMP_OODLE_KRAKEN, 0);
        chunks[1].data[3] = 1; // encrypted
        let encrypted =
            ZoneFile::from_bytes(Path::new("enc.ff"), TaffHeader::write_header(&chunks)).unwrap();
        assert!(recompress_zone_with(&encrypted, b"data", &RleCodec, SMALL).is_err());
        assert!(encrypted.decompress_with(&RleCodec, SMALL).is_err());

        let too_big = BlockLayout { block_size: 512, boundary: 512 };
        assert!(recompress_zone_with(&header_only(0), b"data", &RleCodec, too_big).is_err());
    }
}
