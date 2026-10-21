use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use t9_common::dw_protocol::proxy;

const BACKUP_DIR_NAME: &str = ".bocw_backups";
const STATE_FILE: &str = "mod_state.json";

#[derive(Debug)]
pub struct LocalDll {
    pub name: &'static str,
    pub has_backup: bool,
    pub exists: bool,
}

#[derive(Debug)]
pub struct TrackedFile {
    pub path: String,
    pub backup_exists: bool,
    pub matches_backup: bool,
}

#[derive(Debug)]
pub struct InstallInspection {
    pub state: ModState,
    pub local_dlls: Vec<LocalDll>,
    pub tracked_files: Vec<TrackedFile>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupManifest {
    pub created: String,
    pub game_dir: String,
    pub files: HashMap<String, FileRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileRecord {
    pub original_sha256: String,
    pub size: u64,
    pub backed_up: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModState {
    pub mods_active: bool,
    pub active_mods: Vec<String>,
    pub modified_files: Vec<String>,
    pub last_applied: Option<String>,
    pub offline_verified: bool,
}

impl Default for ModState {
    fn default() -> Self {
        Self {
            mods_active: false,
            active_mods: Vec::new(),
            modified_files: Vec::new(),
            last_applied: None,
            offline_verified: false,
        }
    }
}

pub fn backup_dir(game_dir: &Path) -> PathBuf {
    game_dir.join(BACKUP_DIR_NAME)
}

pub fn state_file_path(game_dir: &Path) -> PathBuf {
    backup_dir(game_dir).join(STATE_FILE)
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let hash = Sha256::digest(&data);
    Ok(hex::encode(hash))
}

pub fn ensure_backup_dir(game_dir: &Path) -> Result<PathBuf> {
    let dir = backup_dir(game_dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(dir.join("zone"))?;
    Ok(dir)
}

fn checked_relative_path(relative_path: &str) -> Result<&Path> {
    let path = Path::new(relative_path);
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        bail!("Expected a game-relative file path: {}", relative_path);
    }
    Ok(path)
}

/// Where the backup of a game-relative file lives (it may not exist yet).
pub fn backup_path(game_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    Ok(backup_dir(game_dir).join(checked_relative_path(relative_path)?))
}

pub fn backup_file(game_dir: &Path, relative_path: &str) -> Result<FileRecord> {
    let relative_path = checked_relative_path(relative_path)?;
    let src = game_dir.join(relative_path);
    if !src.exists() {
        bail!("Source file does not exist: {}", src.display());
    }

    let bak_dir = ensure_backup_dir(game_dir)?;
    let dst = bak_dir.join(relative_path);

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !dst.exists() {
        std::fs::copy(&src, &dst)
            .with_context(|| format!("Failed to backup {} -> {}", src.display(), dst.display()))?;
    }

    // A prior backup is the original, even if the game file has since been patched.
    let hash = sha256_file(&dst)?;
    let meta = std::fs::metadata(&dst)?;
    Ok(FileRecord {
        original_sha256: hash,
        size: meta.len(),
        backed_up: true,
    })
}

pub fn restore_file(game_dir: &Path, relative_path: &str) -> Result<()> {
    let relative_path = checked_relative_path(relative_path)?;
    let bak = backup_dir(game_dir).join(relative_path);
    let dst = game_dir.join(relative_path);

    if !bak.exists() {
        bail!("No backup found for {}", relative_path.display());
    }

    std::fs::copy(&bak, &dst)
        .with_context(|| format!("Failed to restore {} -> {}", bak.display(), dst.display()))?;

    Ok(())
}

pub fn restore_all(game_dir: &Path) -> Result<Vec<String>> {
    let state = load_state(game_dir)?;
    let mut restored = Vec::new();
    let mut failed = Vec::new();

    for file in &state.modified_files {
        match restore_file(game_dir, file) {
            Ok(()) => restored.push(file.clone()),
            Err(e) => failed.push((file.clone(), e)),
        }
    }

    let mut new_state = state;
    new_state.modified_files = failed.iter().map(|(file, _)| file.clone()).collect();
    if failed.is_empty() {
        new_state = ModState::default();
    } else {
        new_state.mods_active = true;
    }
    save_state(game_dir, &new_state)?;

    if !failed.is_empty() {
        bail!(
            "Restored {} file(s), but {} could not be restored; state retained for retry: {}",
            restored.len(),
            failed.len(),
            failed
                .iter()
                .map(|(path, e)| format!("{} ({})", path, e))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(restored)
}

pub fn verify_integrity(game_dir: &Path) -> Result<Vec<(String, bool)>> {
    let manifest_path = backup_dir(game_dir).join("manifest.json");

    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let manifest: BackupManifest = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;

    let mut results = Vec::new();

    for (rel_path, record) in &manifest.files {
        let file_path = game_dir.join(rel_path);
        if !file_path.exists() {
            results.push((rel_path.clone(), false));
            continue;
        }

        let current_hash = sha256_file(&file_path)?;
        let matches = current_hash == record.original_sha256;
        results.push((rel_path.clone(), matches));
    }

    Ok(results)
}

/// Read-only inspection of the project's tracked changes. Absence of findings
/// does not prove that a game install matches Steam's original files.
pub fn inspect_install(game_dir: &Path) -> Result<InstallInspection> {
    let state = load_state(game_dir)?;
    let local_dlls = std::iter::once(&proxy::DLL_NAME)
        .chain(proxy::LEGACY_DLL_NAMES)
        .filter_map(|&name| {
            let exists = game_dir.join(name).exists();
            let has_backup = game_dir.join(format!("{}.bak", name)).exists();
            (exists || has_backup).then_some(LocalDll {
                name,
                exists,
                has_backup,
            })
        })
        .collect();
    let mut tracked_files = Vec::new();
    for path in &state.modified_files {
        let relative = checked_relative_path(path)?;
        let backup = backup_dir(game_dir).join(relative);
        let current = game_dir.join(relative);
        let backup_exists = backup.is_file();
        tracked_files.push(TrackedFile {
            path: path.clone(),
            backup_exists,
            matches_backup: backup_exists
                && current.is_file()
                && sha256_file(&current)? == sha256_file(&backup)?,
        });
    }
    Ok(InstallInspection {
        state,
        local_dlls,
        tracked_files,
    })
}

pub fn load_state(game_dir: &Path) -> Result<ModState> {
    let path = state_file_path(game_dir);
    if !path.exists() {
        return Ok(ModState::default());
    }
    let data = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn save_state(game_dir: &Path, state: &ModState) -> Result<()> {
    ensure_backup_dir(game_dir)?;
    let path = state_file_path(game_dir);
    let data = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, data)?;
    Ok(())
}

pub fn save_manifest(game_dir: &Path, manifest: &BackupManifest) -> Result<()> {
    ensure_backup_dir(game_dir)?;
    let path = backup_dir(game_dir).join("manifest.json");
    let data = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&path, data)?;
    Ok(())
}

pub fn is_steam_offline() -> Result<bool> {
    // Check Steam's loginusers.vdf or config for offline status.
    // Primary method: check if Steam process has network connections to Valve servers.
    // Fallback: check the Steam config file.
    let steam_path = find_steam_path()?;
    let config_path = steam_path.join("config").join("loginusers.vdf");

    if !config_path.exists() {
        // Can't verify - warn user but don't block
        return Ok(false);
    }

    // Also check if the game process is running - refuse if it is
    if is_game_running() {
        bail!(
            "Black Ops Cold War is currently running. \
             Close the game before applying or removing mods."
        );
    }

    // Check for Steam's offline indicator
    let offline_vdf = steam_path.join("config").join("offlinemode.vdf");
    if offline_vdf.exists() {
        let content = std::fs::read_to_string(&offline_vdf).unwrap_or_default();
        if content.contains("\"offlinemode\"\t\t\"1\"")
            || content.contains("\"OfflineMode\"\t\t\"1\"")
        {
            return Ok(true);
        }
    }

    // Check Steam registry key on Windows
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Valve\Steam",
                "/v",
                "AlreadyRetriedOfflineMode",
            ])
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.contains("0x1") {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub fn is_game_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq BlackOpsColdWar.exe", "/NH"])
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            return text.contains("BlackOpsColdWar.exe");
        }
    }
    false
}

fn find_steam_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("reg")
            .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.contains("SteamPath") {
                    if let Some(path) = line.split("REG_SZ").nth(1) {
                        return Ok(PathBuf::from(path.trim()));
                    }
                }
            }
        }
    }

    // Fallback common paths
    let candidates = [
        r"C:\Program Files (x86)\Steam",
        r"C:\Program Files\Steam",
        r"D:\Steam",
    ];

    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Ok(p);
        }
    }

    bail!("Could not find Steam installation")
}

pub fn safety_check(game_dir: &Path) -> Result<()> {
    if is_game_running() {
        bail!(
            "SAFETY: Black Ops Cold War is running. \
             Close the game before modifying any files."
        );
    }

    let state = load_state(game_dir)?;
    if state.mods_active {
        eprintln!(
            "WARNING: Mods are currently active. \
             Run 'bocw_mod_tools restore' before going online."
        );
    }

    Ok(())
}

pub fn pre_apply_check(game_dir: &Path, force_offline: bool) -> Result<()> {
    safety_check(game_dir)?;

    if force_offline {
        return Ok(());
    }

    match is_steam_offline() {
        Ok(true) => Ok(()),
        Ok(false) => {
            bail!(
                "SAFETY: Steam does not appear to be in offline mode.\n\
                 To protect your account:\n\
                 1. Open Steam -> Go Offline\n\
                 2. Then run this command again\n\n\
                 Or use --force-offline to bypass this check \
                 (you accept full responsibility)."
            );
        }
        Err(e) => {
            eprintln!(
                "WARNING: Could not verify Steam offline status: {}\n\
                 Proceeding, but MAKE SURE Steam is offline.",
                e
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestInstall(PathBuf);

    impl TestInstall {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "bocw_mod_tools_backup_test_{}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(path.join("zone")).unwrap();
            Self(path)
        }
    }

    impl Drop for TestInstall {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn existing_backup_remains_original_after_patch() {
        let game = TestInstall::new();
        let zone = game.0.join("zone/test.ff");
        std::fs::write(&zone, b"original").unwrap();
        let first = backup_file(&game.0, "zone/test.ff").unwrap();
        std::fs::write(&zone, b"modified").unwrap();
        let second = backup_file(&game.0, "zone/test.ff").unwrap();
        assert_eq!(second.original_sha256, first.original_sha256);
        assert_eq!(second.size, 8);
        assert_eq!(
            std::fs::read(backup_dir(&game.0).join("zone/test.ff")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn failed_restore_keeps_state_for_retry() {
        let game = TestInstall::new();
        let zone = game.0.join("zone/test.ff");
        std::fs::write(&zone, b"original").unwrap();
        backup_file(&game.0, "zone/test.ff").unwrap();
        std::fs::write(&zone, b"modified").unwrap();
        save_state(
            &game.0,
            &ModState {
                mods_active: true,
                active_mods: vec!["example".into()],
                modified_files: vec!["zone/test.ff".into(), "zone/missing.ff".into()],
                last_applied: None,
                offline_verified: false,
            },
        )
        .unwrap();
        let error = restore_all(&game.0).unwrap_err();
        assert!(error.to_string().contains("zone/missing.ff"));
        assert_eq!(std::fs::read(&zone).unwrap(), b"original");
        let state = load_state(&game.0).unwrap();
        assert!(state.mods_active);
        assert_eq!(state.modified_files, ["zone/missing.ff"]);
    }

    #[test]
    fn inspect_detects_game_local_dll_and_modified_zone() {
        let game = TestInstall::new();
        let zone = game.0.join("zone/test.ff");
        std::fs::write(&zone, b"original").unwrap();
        let record = backup_file(&game.0, "zone/test.ff").unwrap();
        save_manifest(
            &game.0,
            &BackupManifest {
                created: String::new(),
                game_dir: String::new(),
                files: HashMap::from([("zone/test.ff".into(), record)]),
            },
        )
        .unwrap();
        std::fs::write(&zone, b"modified").unwrap();
        std::fs::write(game.0.join("XINPUT9_1_0.dll"), b"custom dll").unwrap();
        save_state(
            &game.0,
            &ModState {
                mods_active: true,
                active_mods: vec!["example".into()],
                modified_files: vec!["zone/test.ff".into()],
                last_applied: None,
                offline_verified: false,
            },
        )
        .unwrap();
        let inspection = inspect_install(&game.0).unwrap();
        assert_eq!(inspection.local_dlls.len(), 1);
        assert_eq!(inspection.local_dlls[0].name, "XINPUT9_1_0.dll");
        assert!(!inspection.local_dlls[0].has_backup);
        assert!(!inspection.tracked_files[0].matches_backup);
        assert!(!verify_integrity(&game.0).unwrap()[0].1);
    }

    #[test]
    fn rejects_paths_outside_game_install() {
        let game = TestInstall::new();
        assert!(backup_file(&game.0, "../outside.ff").is_err());
        assert!(restore_file(&game.0, "../outside.ff").is_err());
        assert!(restore_file(&game.0, r"C:\outside.ff").is_err());
    }
}
