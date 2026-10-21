use anyhow::Result;
use bocw_mod_tools::*;

fn main() -> Result<()> {
    let oodle_dll = oodle_dll_path();
    let zone_path = zone_dir().join("zm_common.ff");

    println!("Opening {}...", zone_path.display());
    let zf = zone::ZoneFile::open(&zone_path)?;

    println!("Decompressing...");
    let decompressed = zf.decompress(&oodle_dll)?;
    println!("Decompressed: {} bytes", decompressed.data.len());

    let patched = decompressed.data.clone();
    let search = b"weapon_rarity_map_list";

    let mut patch_count = 0;
    for i in 0..patched.len().saturating_sub(search.len()) {
        if &patched[i..i + search.len()] == search.as_slice() {
            // Don't actually replace for this test - just count
            patch_count += 1;
        }
    }
    println!("Found '{}' {} time(s) in decompressed data",
        std::str::from_utf8(search).unwrap(), patch_count);

    println!("\nRecompressing (Oodle Kraken)...");
    let recompressed = zone::recompress_zone(&zf, &decompressed.data, &oodle_dll)?;

    println!("Original:     {} bytes", zf.file_size());
    println!("Recompressed: {} bytes", recompressed.len());
    println!("Ratio:        {:.1}%",
        recompressed.len() as f64 / zf.file_size() as f64 * 100.0);

    // Verify the output is a valid TAFF
    let reparsed = taff::parse_header(&recompressed)?;
    println!("\nVerifying recompressed file:");
    println!("  Magic:   {:#x}", reparsed.magic);
    println!("  Version: {:#x}", reparsed.version()?);
    println!("  Compression: {}", reparsed.compression_name());
    println!("  FF name: {}", reparsed.fastfile_info()?.name);

    // Now decompress the recompressed data and verify it matches
    println!("\nRe-decompressing to verify round-trip...");

    // Write to temp file and re-read
    let temp_path = std::path::PathBuf::from("test_recompressed.ff");
    std::fs::write(&temp_path, &recompressed)?;

    let zf2 = zone::ZoneFile::open(&temp_path)?;
    let decompressed2 = zf2.decompress(&oodle_dll)?;

    std::fs::remove_file(&temp_path)?;

    if decompressed.data == decompressed2.data {
        println!("PASS: Round-trip decompression matches exactly!");
    } else {
        println!("Data length: original={} round-trip={}",
            decompressed.data.len(), decompressed2.data.len());

        // Find first difference
        let min_len = decompressed.data.len().min(decompressed2.data.len());
        for i in 0..min_len {
            if decompressed.data[i] != decompressed2.data[i] {
                println!("First difference at offset {:#x}: orig={:#x} new={:#x}",
                    i, decompressed.data[i], decompressed2.data[i]);
                break;
            }
        }
        if decompressed.data.len() != decompressed2.data.len() {
            println!("FAIL: Size mismatch");
        } else {
            println!("FAIL: Content mismatch");
        }
    }

    println!("\nDone!");
    Ok(())
}
