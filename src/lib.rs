pub mod taff;
pub mod oodle;
pub mod zone;
pub mod xpak;
pub mod assets;
pub mod extract;
pub mod patch;
pub mod mod_manager;
pub mod backup;

pub const GAME_DIR_ENV: &str = "BOCW_GAME_DIR";
pub const DEFAULT_GAME_DIR: &str =
    r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops Cold War";

pub fn game_dir() -> std::path::PathBuf {
    std::env::var(GAME_DIR_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(DEFAULT_GAME_DIR))
}

pub fn zone_dir() -> std::path::PathBuf {
    game_dir().join("zone")
}

pub fn oodle_dll_path() -> std::path::PathBuf {
    game_dir().join("oo2core_8_win64.dll")
}
