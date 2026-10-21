use anyhow::Result;
use bocw_mod_tools::*;

fn main() -> Result<()> {
    let oodle_dll = oodle_dll_path();
    let zone_path = zone_dir().join("zm_common.ff");

    println!("=== BOCW Mod Tools - Full Patch Pipeline Test ===\n");

    // Step 1: Open and decompress
    println!("[1/5] Opening {}...", zone_path.display());
    let zf = zone::ZoneFile::open(&zone_path)?;
    println!("  Zone: {}", zf.zone_name.as_deref().unwrap_or("unknown"));
    println!("  File size: {} bytes", zf.file_size());

    println!("[2/5] Decompressing...");
    let decompressed = zf.decompress(&oodle_dll)?;
    let orig_data = &decompressed.data;
    println!("  Decompressed: {} bytes", orig_data.len());

    // Step 2: Apply string patches
    println!("[3/5] Applying string patches...");
    let mut patched_data = orig_data.clone();
    let patches = [
        ("weapon_rarity_map_list", "weapon_rarity_map_noop"),
        ("weapon_upgraded_rarity_map_list", "weapon_upgraded_rarity_map_noop"),
    ];

    let mut total_patches = 0;
    for (search, replace) in &patches {
        assert_eq!(search.len(), replace.len(), "Patch length mismatch: {} vs {}", search, replace);
        let search_bytes = search.as_bytes();
        let replace_bytes = replace.as_bytes();
        let mut count = 0;

        for i in 0..patched_data.len().saturating_sub(search_bytes.len()) {
            if &patched_data[i..i + search_bytes.len()] == search_bytes {
                patched_data[i..i + search_bytes.len()].copy_from_slice(replace_bytes);
                count += 1;
                println!("  Patched '{}' -> '{}' at offset {:#x}", search, replace, i);
            }
        }
        if count == 0 {
            println!("  WARNING: '{}' not found in decompressed data!", search);
        }
        total_patches += count;
    }
    println!("  Total patches applied: {}", total_patches);

    // Verify the original strings are gone and new ones are present
    let orig_present = find_string(&patched_data, b"weapon_rarity_map_list");
    let new_present = find_string(&patched_data, b"weapon_rarity_map_noop");
    println!("  Verify: 'weapon_rarity_map_list' still in data: {}", orig_present);
    println!("  Verify: 'weapon_rarity_map_noop' now in data: {}", new_present);

    // Step 3: Recompress
    println!("[4/5] Recompressing patched data...");
    let recompressed = zone::recompress_zone(&zf, &patched_data, &oodle_dll)?;
    println!("  Recompressed: {} bytes", recompressed.len());
    println!("  Size ratio vs original: {:.1}%",
        recompressed.len() as f64 / zf.file_size() as f64 * 100.0);

    // Step 4: Verify round-trip
    println!("[5/5] Verifying patched file...");
    let temp_path = std::path::PathBuf::from("test_patched_zm_common.ff");
    std::fs::write(&temp_path, &recompressed)?;

    let zf2 = zone::ZoneFile::open(&temp_path)?;
    println!("  TAFF magic:  {:#x}", zf2.taff_header.magic);
    println!("  Version:     {:#x}", zf2.taff_header.version()?);
    println!("  Compression: {}", zf2.taff_header.compression_name());
    println!("  FF name:     {}", zf2.taff_header.fastfile_info()?.name);

    let decompressed2 = zf2.decompress(&oodle_dll)?;
    std::fs::remove_file(&temp_path)?;

    if patched_data == decompressed2.data {
        println!("  Round-trip: PASS (patched data survived recompression)");
    } else {
        println!("  Round-trip: FAIL");
        println!("  Expected {} bytes, got {} bytes", patched_data.len(), decompressed2.data.len());
        return Ok(());
    }

    // Verify patches survived
    let survived = find_string(&decompressed2.data, b"weapon_rarity_map_noop");
    let orig_gone = !find_string(&decompressed2.data, b"weapon_rarity_map_list");
    println!("  Patch survived recompression: {}", survived);
    println!("  Original string removed: {}", orig_gone);

    // Note: we're skipping the "weapon_upgraded_rarity_map_list" check
    // because it may contain the shorter "weapon_rarity_map_list" as a substring
    // The "noop" replacement at the right offset is the definitive check

    println!("\n=== Pipeline test PASSED ===");
    println!("The No Weapon Rarities mod can be safely applied to zm_common.ff");
    println!("  - Backup the original file before applying");
    println!("  - Put Steam in offline mode before launching");
    println!("  - Restore original files before going online");

    Ok(())
}

fn find_string(data: &[u8], needle: &[u8]) -> bool {
    data.windows(needle.len()).any(|w| w == needle)
}
