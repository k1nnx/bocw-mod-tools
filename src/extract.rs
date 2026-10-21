use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;
use std::path::Path;

use crate::assets::{AssetEntry, AssetPool, AssetType, known_asset_names};
use crate::zone::DecompressedZone;

pub fn extract_asset_pool(zone: &DecompressedZone) -> Result<AssetPool> {
    let zone_name = zone
        .zone_file
        .zone_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let mut pool = AssetPool::new(&zone_name);
    let known = known_asset_names();
    let data = &zone.data;

    if data.len() < 16 {
        return Ok(pool);
    }

    // Scan for asset entries in the decompressed data.
    // T9 asset list: array of (u32 type, u32 padding, u64 name_hash, u64 data_ptr)
    let mut offset = 0;

    while offset + 24 <= data.len() {
        let mut cursor = Cursor::new(&data[offset..]);

        let type_id = match cursor.read_u32::<LittleEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };

        if type_id == 0 || type_id > 0xFF {
            offset += 8;
            continue;
        }

        let _padding = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let name_hash = match cursor.read_u64::<LittleEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };

        let asset_type = AssetType::from_u8(type_id as u8);
        if asset_type == AssetType::Unknown {
            offset += 8;
            continue;
        }

        let name = known.get(&name_hash).cloned();

        let entry = AssetEntry {
            asset_type,
            name_hash,
            name,
            offset,
            size: 0,
            data: None,
        };

        pool.add(entry);
        offset += 24;
    }

    Ok(pool)
}

pub fn extract_strings(data: &[u8]) -> Vec<(usize, String)> {
    let mut strings = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if data[i].is_ascii_graphic() || data[i] == b' ' {
            let start = i;
            while i < data.len() && (data[i].is_ascii_graphic() || data[i] == b' ') {
                i += 1;
            }
            let len = i - start;
            if len >= 4 {
                let s = String::from_utf8_lossy(&data[start..i]).to_string();
                strings.push((start, s));
            }
        } else {
            i += 1;
        }
    }

    strings
}

pub fn extract_string_tables(data: &[u8]) -> Vec<(usize, Vec<Vec<String>>)> {
    let mut tables = Vec::new();
    let strings = extract_strings(data);

    let mut current_table: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut last_end = 0;

    for (offset, s) in &strings {
        if *offset > last_end + 2 && !current_row.is_empty() {
            current_table.push(current_row.clone());
            current_row.clear();
        }

        if *offset > last_end + 64 && !current_table.is_empty() {
            let table_start = current_table
                .first()
                .map(|_| *offset)
                .unwrap_or(*offset);
            tables.push((table_start, current_table.clone()));
            current_table.clear();
        }

        if s.contains(',') {
            current_row.extend(s.split(',').map(|p| p.to_string()));
        } else {
            current_row.push(s.clone());
        }

        last_end = offset + s.len();
    }

    if !current_row.is_empty() {
        current_table.push(current_row);
    }
    if !current_table.is_empty() {
        tables.push((0, current_table));
    }

    tables
}

pub fn dump_zone_to_dir(zone: &DecompressedZone, output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;

    let zone_name = zone
        .zone_file
        .zone_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Dump raw decompressed data
    let raw_path = output_dir.join(format!("{}.raw", zone_name));
    std::fs::write(&raw_path, &zone.data)
        .with_context(|| format!("Failed to write {}", raw_path.display()))?;

    // Dump extracted strings
    let strings = extract_strings(&zone.data);
    let strings_path = output_dir.join(format!("{}_strings.txt", zone_name));
    let string_content: String = strings
        .iter()
        .map(|(off, s)| format!("{:#010x}: {}", off, s))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&strings_path, &string_content)?;

    // Dump asset pool info
    let pool = extract_asset_pool(zone)?;
    let pool_path = output_dir.join(format!("{}_assets.json", zone_name));
    let pool_json = serde_json::to_string_pretty(&pool)?;
    std::fs::write(&pool_path, &pool_json)?;

    // Dump zone header info
    let version = zone.zone_file.taff_header.version().unwrap_or(0);
    let compression = zone.zone_file.taff_header.compression_name();
    let block_count = zone.zone_file.block_count();
    let info = serde_json::json!({
        "zone_name": zone_name,
        "ff_version": format!("{:#x}", version),
        "compression": compression,
        "file_size": zone.zone_file.file_size(),
        "decompressed_size": zone.data.len(),
        "block_count": block_count,
        "header_chunks": zone.zone_file.taff_header.chunks.len(),
    });
    let info_path = output_dir.join(format!("{}_info.json", zone_name));
    std::fs::write(&info_path, serde_json::to_string_pretty(&info)?)?;

    Ok(())
}
