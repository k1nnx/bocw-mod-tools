//! End-to-end `install` → `apply` → `clean` on a synthetic, uncompressed zone.
//! Uncompressed zones never touch Oodle, so this runs without the game.

use std::path::{Path, PathBuf};
use std::process::Command;

use bocw_mod_tools::backup;
use bocw_mod_tools::patch::find_string;
use bocw_mod_tools::taff::{self, FlexChunk, TaffHeader};
use bocw_mod_tools::zone::{self, BlockCodec, BlockLayout, ZoneFile};

const ZONE: &str = "zm_common";

struct NoCodec;

impl BlockCodec for NoCodec {
    fn decompress(&self, _: &[u8], _: usize) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("uncompressed fixture needs no codec")
    }
    fn compress(&self, _: &[u8], _: u8) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("uncompressed fixture needs no codec")
    }
}

struct Fixture {
    root: PathBuf,
    original_zone: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bocw_mod_tools_apply_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("game/zone")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();

        let mut content = vec![0xEEu8; 300_000];
        content[1000..1022].copy_from_slice(b"weapon_rarity_map_list");
        content[250_000..250_031].copy_from_slice(b"weapon_upgraded_rarity_map_list");
        let original_zone = build_zone(&content);
        std::fs::write(root.join("game/zone/zm_common.ff"), &original_zone).unwrap();

        Self { root, original_zone }
    }

    fn game(&self) -> PathBuf {
        self.root.join("game")
    }

    fn zone_path(&self) -> PathBuf {
        self.game().join("zone/zm_common.ff")
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let output = Command::new(env!("CARGO_BIN_EXE_bocw_mod_tools"))
            .env("USERPROFILE", self.root.join("home"))
            .arg("--game-dir")
            .arg(self.game())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{:?} failed:\n{}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn set_options(&self, rarity: bool, upgraded: bool) {
        let dir = self.root.join("home/bocw_mods/presets");
        std::fs::create_dir_all(&dir).unwrap();
        let store = serde_json::json!({
            "mod_id": "no_weapon_rarities",
            "presets": [{
                "name": "test",
                "is_default": true,
                "values": {"disable_rarity_map": rarity, "disable_upgraded_rarity": upgraded}
            }],
            "active_preset": "test"
        });
        std::fs::write(dir.join("no_weapon_rarities.json"), store.to_string()).unwrap();
    }

    fn zone_content(&self) -> Vec<u8> {
        ZoneFile::open(&self.zone_path())
            .unwrap()
            .decompress_with(&NoCodec, BlockLayout::default())
            .unwrap()
            .data
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn build_zone(content: &[u8]) -> Vec<u8> {
    let mut info = vec![0u8; 0x150];
    info[..ZONE.len()].copy_from_slice(ZONE.as_bytes());
    let chunks = [
        FlexChunk { section_type: taff::ST_VERSION, data: taff::CW_VERSION.to_le_bytes().to_vec() },
        FlexChunk {
            section_type: taff::ST_PLATFORM_DATA,
            data: vec![0, taff::COMP_UNCOMPRESSED, taff::PLATFORM_PC, 0],
        },
        FlexChunk { section_type: taff::ST_SIZE_DATA, data: vec![0; 24] },
        FlexChunk { section_type: taff::ST_FASTFILE_INFO, data: info },
    ];
    let header = ZoneFile::from_bytes(Path::new("fixture.ff"), TaffHeader::write_header(&chunks)).unwrap();
    zone::recompress_zone_with(&header, content, &NoCodec, BlockLayout::default()).unwrap()
}

fn has(data: &[u8], needle: &str) -> bool {
    !find_string(data, needle).is_empty()
}

#[test]
fn apply_follows_options_and_clean_restores() {
    let fx = Fixture::new();
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("mods/no_weapon_rarities");
    fx.run(&["install", sample.to_str().unwrap()]);

    // Defaults: both patches on.
    fx.run(&["apply", "--force-offline"]);
    let patched = std::fs::read(fx.zone_path()).unwrap();
    let data = fx.zone_content();
    assert!(has(&data, "weapon_rarity_map_noop"));
    assert!(has(&data, "weapon_upgraded_rarity_map_noop"));
    assert!(!has(&data, "weapon_rarity_map_list"));
    assert_eq!(
        std::fs::read(backup::backup_path(&fx.game(), "zone/zm_common.ff").unwrap()).unwrap(),
        fx.original_zone
    );
    let state = backup::load_state(&fx.game()).unwrap();
    assert!(state.mods_active);
    assert_eq!(state.modified_files.len(), 1);

    // Re-applying the same settings patches the original again: same bytes.
    fx.run(&["apply", "--force-offline"]);
    assert_eq!(std::fs::read(fx.zone_path()).unwrap(), patched);

    // Turning one option off reverts just that patch.
    fx.set_options(true, false);
    fx.run(&["apply", "--force-offline"]);
    let data = fx.zone_content();
    assert!(has(&data, "weapon_rarity_map_noop"));
    assert!(has(&data, "weapon_upgraded_rarity_map_list"));

    // Turning both off restores the original zone and clears tracking.
    fx.set_options(false, false);
    let output = fx.run(&["apply", "--force-offline"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("no longer patched"));
    assert_eq!(std::fs::read(fx.zone_path()).unwrap(), fx.original_zone);
    let state = backup::load_state(&fx.game()).unwrap();
    assert!(!state.mods_active);
    assert!(state.modified_files.is_empty());

    // And clean restores after a patched apply.
    fx.set_options(true, true);
    fx.run(&["apply", "--force-offline"]);
    assert_ne!(std::fs::read(fx.zone_path()).unwrap(), fx.original_zone);
    fx.run(&["clean"]);
    assert_eq!(std::fs::read(fx.zone_path()).unwrap(), fx.original_zone);
}
