use std::io::Read;

fn main() {
    let path = r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops Cold War\zone\zm_common.ff";
    let mut f = std::fs::File::open(path).unwrap();
    let mut data = vec![0u8; 0x1000];
    f.read_exact(&mut data).unwrap();

    println!("Magic: {:?}", &data[..4]);
    let ver = u32::from_le_bytes(data[4..8].try_into().unwrap());
    println!("Version: {}", ver);
    let build_hash = u32::from_le_bytes(data[8..12].try_into().unwrap());
    println!("Build hash: {:#x}", build_hash);
    let platform = u32::from_le_bytes(data[12..16].try_into().unwrap());
    println!("Platform: {}", platform);

    let tag = &data[0x44..0x44 + 20];
    let end = tag.iter().position(|&b| b == 0).unwrap_or(20);
    println!("Tag: {}", String::from_utf8_lossy(&tag[..end]));

    let total_decomp = u64::from_le_bytes(data[0x68..0x70].try_into().unwrap());
    println!("Total decompressed at 0x68: {} ({:.1} MB)", total_decomp, total_decomp as f64 / (1024.0 * 1024.0));

    // Scan for interesting u32 values around header boundary
    println!("\n--- Key u64 values around 0x178 boundary ---");
    for off in (0x150..0x1C0).step_by(8) {
        let val = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        if val != 0 {
            println!("  0x{:04x}: {:#018x} ({})", off, val, val);
        }
    }

    // Hex dump 0x178 to 0x600
    println!("\n--- Hex dump 0x178 to 0x600 ---");
    for off in (0x178..0x600).step_by(16) {
        let hex: String = (0..16)
            .map(|j| format!("{:02x}", data[off + j]))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = (0..16)
            .map(|j| {
                let b = data[off + j];
                if (32..127).contains(&b) { b as char } else { '.' }
            })
            .collect();
        // Only show non-zero lines
        if data[off..off + 16].iter().any(|&b| b != 0) {
            println!("  0x{:04x}: {}  {}", off, hex, ascii);
        }
    }

    // Look for block-size-like u32 pairs
    println!("\n--- Possible block table entries (comp/decomp u32 pairs) ---");
    for off in (0x178..0xA00).step_by(4) {
        let a = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        let b = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
        if a > 0x1000 && a < 0x1000000 && b > 0x1000 && b < 0x1000000 {
            println!("  0x{:04x}: comp={:#x} decomp={:#x}", off, a, b);
        }
    }

    // Also check for the xfile block descriptors - look at every 8 bytes as u64
    println!("\n--- Non-zero u64 values 0x178-0x500 ---");
    for off in (0x178..0x500).step_by(8) {
        let val = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        if val != 0 && val < 0xFFFFFFFFFFFF {
            println!("  0x{:04x}: {:#x} ({})", off, val, val);
        }
    }
}
