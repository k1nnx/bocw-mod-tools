use anyhow::Result;
use bocw_mod_tools::*;
fn main() -> Result<()> {
    let oodle_dll = oodle_dll_path();
    let zone_path = zone_dir().join("zm_common.ff");

    println!("Opening and decompressing zm_common...");
    let zf = zone::ZoneFile::open(&zone_path)?;
    let decompressed = zf.decompress(&oodle_dll)?;
    let data = &decompressed.data;
    println!("Decompressed: {} bytes\n", data.len());

    // Key strings to find and analyze context around
    let targets = [
        "weapon_rarity_map_list",
        "weapon_upgraded_rarity_map_list",
        "zm_magicbox_weapon_ar_blue_list",
        "zm_magicbox_weapon_lmg_orange_list",
        "zm_magicbox_weapon_misc_green_list",
        "zm_magicbox_weapon_misc_list",
        "zm_magicbox_weapon_pistols_green_list",
        "zm_magicbox_weapon_shotgun_list",
        "zm_magicbox_weapon_shotguns_purple_list",
        "zm_magicbox_weapon_smg_orange_list",
        "zm_magicbox_weapon_smg_purple_list",
        "zm_magicbox_weapon_sniper_list",
        "zm_magicbox_equipment_list",
    ];

    for target in &targets {
        let target_bytes = target.as_bytes();
        let mut found = false;
        for i in 0..data.len().saturating_sub(target_bytes.len()) {
            if &data[i..i + target_bytes.len()] == target_bytes {
                found = true;
                println!("=== {} at offset {:#x} ===", target, i);

                // Show 64 bytes before
                let start = i.saturating_sub(64);
                println!("  BEFORE ({:#x} to {:#x}):", start, i);
                print_hex_and_strings(&data[start..i], start);

                // Show 128 bytes after the string
                let str_end = i + target_bytes.len();
                let end = (str_end + 128).min(data.len());
                println!("  AFTER ({:#x} to {:#x}):", str_end, end);
                print_hex_and_strings(&data[str_end..end], str_end);
                println!();
            }
        }
        if !found {
            println!("NOT FOUND: {}\n", target);
        }
    }

    // Also search for color/tier-related strings near the rarity data
    println!("\n=== Searching for tier/color identifiers ===");
    let color_terms = ["blue", "green", "orange", "purple", "common", "uncommon", "rare", "epic", "legendary"];
    for term in &color_terms {
        let term_bytes = term.as_bytes();
        let mut count = 0;
        for i in 0..data.len().saturating_sub(term_bytes.len()) {
            if &data[i..i + term_bytes.len()] == term_bytes {
                // Check it's a standalone word (null or non-alpha before/after)
                let before_ok = i == 0 || !data[i-1].is_ascii_alphanumeric();
                let after_idx = i + term_bytes.len();
                let after_ok = after_idx >= data.len() || !data[after_idx].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    count += 1;
                    if count <= 5 {
                        // Get surrounding context string
                        let ctx_start = i.saturating_sub(32);
                        let ctx_end = (i + term_bytes.len() + 32).min(data.len());
                        let ctx = extract_printable(&data[ctx_start..ctx_end]);
                        println!("  '{}' at {:#x}: ...{}...", term, i, ctx);
                    }
                }
            }
        }
        if count > 5 {
            println!("  '{}': {} total occurrences (showing first 5)", term, count);
        } else if count > 0 {
            println!("  '{}': {} occurrence(s)", term, count);
        }
    }

    Ok(())
}

fn print_hex_and_strings(data: &[u8], base_offset: usize) {
    for chunk_start in (0..data.len()).step_by(16) {
        let chunk_end = (chunk_start + 16).min(data.len());
        let chunk = &data[chunk_start..chunk_end];

        print!("    {:#010x}: ", base_offset + chunk_start);
        for b in chunk {
            print!("{:02x} ", b);
        }
        // Pad if short
        for _ in chunk.len()..16 {
            print!("   ");
        }
        print!(" | ");
        for b in chunk {
            if b.is_ascii_graphic() || *b == b' ' {
                print!("{}", *b as char);
            } else {
                print!(".");
            }
        }
        println!();
    }
}

fn extract_printable(data: &[u8]) -> String {
    data.iter()
        .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
        .collect()
}
