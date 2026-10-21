use anyhow::{bail, Result};
use bocw_mod_tools::*;

fn main() -> Result<()> {
    let game = game_dir();
    let oodle_dll = oodle_dll_path();
    let zone_path = zone_dir();
    let ff_path = zone_path.join("zm_common.ff");

    println!("=== BOCW Mod Tools - Apply No Weapon Rarities ===\n");

    // Safety: check game isn't running
    if backup::is_game_running() {
        bail!("Game is running! Close it first.");
    }

    // Step 1: Backup
    println!("[1/4] Backing up zm_common.ff...");
    let backup_path = backup::backup_dir(&game).join("zone").join("zm_common.ff");
    if backup_path.exists() {
        println!("  Backup already exists at {}", backup_path.display());
    } else {
        let record = backup::backup_file(&game, "zone\\zm_common.ff")?;
        println!("  Backed up ({} bytes, sha256: {})", record.size, &record.original_sha256[..16]);
    }

    // Step 2: Decompress
    println!("[2/4] Decompressing zm_common.ff...");
    let zf = zone::ZoneFile::open(&ff_path)?;
    let decompressed = zf.decompress(&oodle_dll)?;
    let mut data = decompressed.data;
    println!("  Decompressed: {} bytes ({:.1} MB)", data.len(), data.len() as f64 / (1024.0 * 1024.0));

    // Step 3: Apply string patches
    println!("[3/4] Applying No Weapon Rarities patches...");
    let patches = [
        ("weapon_rarity_map_list", "weapon_rarity_map_noop"),
        ("weapon_upgraded_rarity_map_list", "weapon_upgraded_rarity_map_noop"),
    ];

    let mut total = 0;
    for (search, replace) in &patches {
        let search_bytes = search.as_bytes();
        let replace_bytes = replace.as_bytes();
        assert_eq!(search_bytes.len(), replace_bytes.len());

        for i in 0..data.len().saturating_sub(search_bytes.len()) {
            if &data[i..i + search_bytes.len()] == search_bytes {
                data[i..i + search_bytes.len()].copy_from_slice(replace_bytes);
                total += 1;
                println!("  Patched: '{}' -> '{}' at {:#x}", search, replace, i);
            }
        }
    }

    if total == 0 {
        bail!("No patches applied! The file may already be patched or has a different format.");
    }
    println!("  {} patches applied", total);

    // Step 4: Recompress and write
    println!("[4/4] Recompressing and writing...");
    let recompressed = zone::recompress_zone(&zf, &data, &oodle_dll)?;
    println!("  Original:     {} bytes ({:.1} MB)", zf.file_size(), zf.file_size() as f64 / (1024.0 * 1024.0));
    println!("  Recompressed: {} bytes ({:.1} MB)", recompressed.len(), recompressed.len() as f64 / (1024.0 * 1024.0));

    std::fs::write(&ff_path, &recompressed)?;
    println!("  Written to: {}", ff_path.display());

    // Save mod state
    let state = backup::ModState {
        mods_active: true,
        active_mods: vec!["no_weapon_rarities".to_string()],
        modified_files: vec!["zone\\zm_common.ff".to_string()],
        last_applied: Some(chrono::Utc::now().to_rfc3339()),
        offline_verified: false,
    };
    backup::save_state(&game, &state)?;

    println!("\n=== MOD APPLIED SUCCESSFULLY ===");
    println!("To test:");
    println!("  1. Make sure Steam is in OFFLINE mode");
    println!("  2. Launch Black Ops Cold War");
    println!("  3. Start a Zombies match (any map)");
    println!("  4. Hit the Mystery Box - all weapons should be same tier");
    println!("\nTo restore original files:");
    println!("  cargo run --bin bocw_mod_tools -- clean");
    println!("  OR run the restore command before going online");

    Ok(())
}
