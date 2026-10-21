use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::collections::HashMap;
use std::path::PathBuf;

use bocw_mod_tools::*;

#[derive(Parser)]
#[command(
    name = "bocw-mod-tools",
    about = "Black Ops Cold War zone modding toolkit",
    version,
    long_about = "Offline-only mod tools for BOCW. Manages zone file backups, \
                  extracts assets, applies mods with an in-game HUD, \
                  and enforces safety checks to protect your account."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Override the game install directory
    #[arg(long, global = true)]
    game_dir: Option<PathBuf>,

    /// Skip the Steam offline check (you accept responsibility)
    #[arg(long, global = true)]
    force_offline: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Show info about the game install and zone files
    Info,

    /// List all zone files with categories
    Zones {
        /// Filter by category (zombies, multiplayer, campaign, core)
        #[arg(short, long)]
        category: Option<String>,
    },

    /// Inspect a specific zone file header
    Inspect {
        /// Zone file name (e.g. zm_common.ff)
        zone: String,
    },

    /// Extract and dump a zone file's contents
    Extract {
        /// Zone file name
        zone: String,
        /// Output directory
        #[arg(short, long, default_value = "extracted")]
        output: PathBuf,
    },

    /// Search for strings across zone files
    Search {
        /// String pattern to search for
        pattern: String,
        /// Only search in these zone categories
        #[arg(short, long)]
        category: Option<String>,
    },

    /// Backup original zone files before modding
    Backup {
        /// Specific zone files to back up (default: all)
        zones: Vec<String>,
    },

    /// Restore original zone files (removes all mods)
    Restore {
        /// Specific zone files to restore (default: all modified)
        zones: Vec<String>,
    },

    /// Verify file integrity against backups
    Verify,

    /// Initialize the mod directory structure
    Init,

    /// Create a new mod from a template
    New {
        /// Mod name
        name: String,
        /// Mod category (zombies, multiplayer, core)
        #[arg(short, long, default_value = "zombies")]
        category: String,
    },

    /// Install a mod from a directory
    Install {
        /// Path to the mod directory
        path: PathBuf,
    },

    /// Uninstall a mod
    Uninstall {
        /// Mod ID
        mod_id: String,
    },

    /// List installed mods
    List,

    /// Build and apply active mods (generates HUD, patches zones)
    Apply {
        /// Profile to apply (default: "default")
        #[arg(short, long, default_value = "default")]
        profile: String,
    },

    /// Remove all mods from game files (safe for going online)
    Clean,

    /// Check current mod state
    Status,

    /// Read-only checks for modified zones and game-local XInput DLLs
    Doctor,

    /// Manage mod presets
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },

    /// Generate the in-game mod menu GSC script
    GenHud {
        /// Output file path
        #[arg(short, long, default_value = "mod_menu.gsc")]
        output: PathBuf,
    },

    /// DemonWare emulator server management
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },

    /// Proxy DLL management (install/remove XInput1_0.dll redirect)
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },
}

#[derive(Subcommand)]
enum ServerAction {
    /// Start the local DemonWare emulator server
    Start,
    /// Show server status
    Status,
}

#[derive(Subcommand)]
enum ProxyAction {
    /// Install the proxy DLL (XInput1_0.dll) to the game directory
    Install,
    /// Remove the proxy DLL and restore the original
    Remove,
    /// Check proxy DLL status
    Status,
}

#[derive(Subcommand)]
enum PresetAction {
    /// List presets for a mod
    List { mod_id: String },
    /// Save current settings as a preset
    Save {
        mod_id: String,
        preset_name: String,
    },
    /// Load a preset
    Load {
        mod_id: String,
        preset_name: String,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {:#}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let game = cli
        .game_dir
        .clone()
        .unwrap_or_else(|| game_dir());
    let zone_path = game.join("zone");
    let oodle = game.join("oo2core_8_win64.dll");

    match cli.command {
        Commands::Info => cmd_info(&game, &zone_path, &oodle),
        Commands::Zones { category } => cmd_zones(&zone_path, category.as_deref()),
        Commands::Inspect { zone } => cmd_inspect(&zone_path, &zone),
        Commands::Extract { zone, output } => cmd_extract(&zone_path, &oodle, &zone, &output),
        Commands::Search { pattern, category } => {
            cmd_search(&zone_path, &pattern, category.as_deref())
        }
        Commands::Backup { zones } => cmd_backup(&game, &zone_path, &zones),
        Commands::Restore { zones } => cmd_restore(&game, &zones),
        Commands::Verify => cmd_verify(&game),
        Commands::Init => cmd_init(),
        Commands::New { name, category } => cmd_new_mod(&name, &category),
        Commands::Install { path } => cmd_install(&path),
        Commands::Uninstall { mod_id } => cmd_uninstall(&mod_id),
        Commands::List => cmd_list(),
        Commands::Apply { profile } => cmd_apply(&game, &zone_path, &oodle, &profile, cli.force_offline),
        Commands::Clean => cmd_clean(&game),
        Commands::Status => cmd_status(&game),
        Commands::Doctor => cmd_doctor(&game),
        Commands::Preset { action } => cmd_preset(action),
        Commands::GenHud { output } => cmd_gen_hud(&output),
        Commands::Server { action } => cmd_server(action),
        Commands::Proxy { action } => cmd_proxy(&game, action),
    }
}

// ─── Command implementations ────────────────────────────────────────────────

fn cmd_info(game: &PathBuf, zone_path: &PathBuf, oodle: &PathBuf) -> Result<()> {
    println!("{}", "BOCW Mod Tools".cyan().bold());
    println!("{}", "═".repeat(50));

    println!(
        "  Game dir:  {}",
        if game.exists() {
            game.display().to_string().green()
        } else {
            format!("{} (NOT FOUND)", game.display()).red()
        }
    );

    println!(
        "  Zone dir:  {}",
        if zone_path.exists() {
            zone_path.display().to_string().green()
        } else {
            "NOT FOUND".to_string().red()
        }
    );

    println!(
        "  Oodle DLL: {}",
        if oodle.exists() {
            "Found".green()
        } else {
            "MISSING".red()
        }
    );

    if zone_path.exists() {
        let files = zone::list_zone_files(zone_path)?;
        println!("  Zone files: {}", files.len().to_string().yellow());

        let mut categories: HashMap<&str, usize> = HashMap::new();
        for f in &files {
            let name = f.file_stem().unwrap().to_string_lossy();
            let cat = zone::categorize_zone(&name);
            *categories.entry(cat).or_insert(0) += 1;
        }
        for (cat, count) in &categories {
            println!("    {}: {}", cat, count);
        }
    }

    let state = backup::load_state(game)?;
    println!(
        "\n  Mods active: {}",
        if state.mods_active {
            format!("YES ({} mods)", state.active_mods.len()).yellow()
        } else {
            "No".green().to_string().into()
        }
    );

    if state.mods_active {
        println!(
            "  {}",
            "WARNING: Remove modifications, then verify files before official play!".red().bold()
        );
    }

    if !backup::inspect_install(game)?.local_dlls.is_empty() {
        println!("  {} Game-local XInput DLL detected; run 'doctor'.", "WARNING:".red().bold());
    }

    Ok(())
}

fn cmd_zones(zone_path: &PathBuf, category: Option<&str>) -> Result<()> {
    let files = zone::list_zone_files(zone_path)?;

    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy();
        let cat = zone::categorize_zone(&name);

        if let Some(filter) = category {
            if cat != filter {
                continue;
            }
        }

        let size = std::fs::metadata(f)?.len();
        let size_str = if size > 1_000_000 {
            format!("{:.1}MB", size as f64 / 1_000_000.0)
        } else {
            format!("{:.1}KB", size as f64 / 1000.0)
        };

        let cat_color = match cat {
            "zombies" => cat.green(),
            "multiplayer" => cat.blue(),
            "campaign" => cat.yellow(),
            "core" => cat.cyan(),
            _ => cat.white(),
        };

        println!("  {:40} {:>10}  [{}]", name, size_str, cat_color);
    }

    Ok(())
}

fn cmd_inspect(zone_path: &PathBuf, zone_name: &str) -> Result<()> {
    let ff_path = zone_path.join(if zone_name.ends_with(".ff") {
        zone_name.to_string()
    } else {
        format!("{}.ff", zone_name)
    });

    let zf = zone::ZoneFile::open(&ff_path)?;

    let version = zf.taff_header.version().unwrap_or(0);
    let platform = zf.taff_header.platform_data().ok();
    let size_data = zf.taff_header.size_data().ok();
    let build = zf.taff_header.build_data().ok();
    let block_count = zf.block_count();

    println!("{}", "Zone File Info".cyan().bold());
    println!("{}", "─".repeat(50));
    println!("  File:        {}", ff_path.display());
    println!("  Size:        {} bytes", zf.file_size());
    println!("  FF Version:  {:#x}", version);
    println!("  Compression: {}", zf.taff_header.compression_name());
    if let Some(ref p) = platform {
        println!("  Platform:    {} (encrypted: {})", p.platform, p.encrypted);
    }
    println!(
        "  Zone name:   {}",
        zf.zone_name.as_deref().unwrap_or("unknown")
    );
    if let Some(ref sd) = size_data {
        println!("  Decompressed: {} bytes ({:.1} MB)", sd.size, sd.size as f64 / (1024.0 * 1024.0));
    }
    println!("  Blocks:      {}", block_count);
    println!("  Header chunks: {}", zf.taff_header.chunks.len());
    for chunk in &zf.taff_header.chunks {
        println!("    {:#010x}  {} bytes", chunk.section_type, chunk.data.len());
    }
    if let Some(ref b) = build {
        println!("  Builder:     {}", b.builder_name);
    }

    if size_data.is_some() {
        let xblock_sizes = zf.taff_header.block_sizes().unwrap_or_default();
        if !xblock_sizes.is_empty() {
            println!("\n  XFile block sizes:");
            let names = ["temp", "temp_preload", "rt_virtual", "rt_physical", "virtual",
                         "cpu_gpu", "physical", "streamer", "streamer_cpu", "memmapped",
                         "load_time", "tmp_preload_ptr", "mesh"];
            for (i, &sz) in xblock_sizes.iter().enumerate() {
                if sz > 0 {
                    let name = names.get(i).unwrap_or(&"unknown");
                    println!("    [{:2}] {:18} {:#x} ({:.1} KB)", i, name, sz, sz as f64 / 1024.0);
                }
            }
        }
    }

    Ok(())
}

fn cmd_extract(
    zone_path: &PathBuf,
    oodle: &PathBuf,
    zone_name: &str,
    output: &PathBuf,
) -> Result<()> {
    let ff_path = zone_path.join(if zone_name.ends_with(".ff") {
        zone_name.to_string()
    } else {
        format!("{}.ff", zone_name)
    });

    println!("Opening {}...", ff_path.display());
    let zf = zone::ZoneFile::open(&ff_path)?;

    println!("Decompressing ({} blocks)...", zf.block_count());
    let decompressed = zf.decompress(oodle)?;

    println!(
        "Decompressed: {} bytes -> {} bytes",
        zf.file_size(),
        decompressed.data.len()
    );

    println!("Extracting to {}...", output.display());
    extract::dump_zone_to_dir(&decompressed, output)?;

    println!("{}", "Done!".green().bold());
    Ok(())
}

fn cmd_search(zone_path: &PathBuf, pattern: &str, category: Option<&str>) -> Result<()> {
    let files = zone::list_zone_files(zone_path)?;
    let pattern_bytes = pattern.as_bytes();

    println!("Searching for '{}' across zone files...\n", pattern.yellow());

    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy();

        if let Some(filter) = category {
            if zone::categorize_zone(&name) != filter {
                continue;
            }
        }

        let data = std::fs::read(f)?;
        let hits = patch::find_pattern(&data, pattern_bytes);
        if !hits.is_empty() {
            println!(
                "  {} - {} hit(s)",
                name.green(),
                hits.len().to_string().yellow()
            );
            for offset in hits.iter().take(5) {
                let context_start = offset.saturating_sub(16);
                let context_end = (offset + pattern.len() + 16).min(data.len());
                let context = String::from_utf8_lossy(&data[context_start..context_end]);
                println!("    {:#010x}: ...{}...", offset, context);
            }
            if hits.len() > 5 {
                println!("    ... and {} more", hits.len() - 5);
            }
        }
    }

    Ok(())
}

fn cmd_backup(game: &PathBuf, zone_path: &PathBuf, zones: &[String]) -> Result<()> {
    backup::safety_check(game)?;

    let files = if zones.is_empty() {
        zone::list_zone_files(zone_path)?
    } else {
        zones
            .iter()
            .map(|z| {
                zone_path.join(if z.ends_with(".ff") {
                    z.clone()
                } else {
                    format!("{}.ff", z)
                })
            })
            .collect()
    };

    println!(
        "Backing up {} zone file(s)...",
        files.len().to_string().yellow()
    );

    let mut manifest = backup::BackupManifest {
        created: chrono::Utc::now().to_rfc3339(),
        game_dir: game.display().to_string(),
        files: HashMap::new(),
    };

    for f in &files {
        if !f.exists() {
            eprintln!("  Skipping {} (not found)", f.display());
            continue;
        }

        let rel = f
            .strip_prefix(game)
            .unwrap_or(f.as_path())
            .to_string_lossy()
            .to_string();

        match backup::backup_file(game, &rel) {
            Ok(record) => {
                println!("  {} {}", "✓".green(), rel);
                manifest.files.insert(rel, record);
            }
            Err(e) => eprintln!("  {} {}: {}", "✗".red(), rel, e),
        }
    }

    backup::save_manifest(game, &manifest)?;
    println!(
        "\n{} {} files backed up.",
        "Done!".green().bold(),
        manifest.files.len()
    );

    Ok(())
}

fn cmd_restore(game: &PathBuf, zones: &[String]) -> Result<()> {
    backup::safety_check(game)?;

    if zones.is_empty() {
        let restored = backup::restore_all(game)?;
        println!(
            "{} Restored {} tracked zone file(s) from backups (not a full game integrity check).",
            "Done!".green().bold(),
            restored.len()
        );
    } else {
        let mut failed = Vec::new();
        for z in zones {
            let rel = format!("zone\\{}", if z.ends_with(".ff") { z.clone() } else { format!("{}.ff", z) });
            match backup::restore_file(game, &rel) {
                Ok(()) => println!("  {} {}", "✓".green(), z),
                Err(e) => {
                    eprintln!("  {} {}: {}", "✗".red(), z, e);
                    failed.push(z.as_str());
                }
            }
        }
        if !failed.is_empty() {
            anyhow::bail!("Could not restore: {}. Check backups before launching the game.", failed.join(", "));
        }
    }

    Ok(())
}

fn cmd_verify(game: &PathBuf) -> Result<()> {
    let results = backup::verify_integrity(game)?;

    if results.is_empty() {
        println!("No backup manifest found. Run 'doctor' to inspect tracked modifications.");
        return Ok(());
    }

    let mut all_ok = true;
    for (file, ok) in &results {
        if *ok {
            println!("  {} {}", "✓".green(), file);
        } else {
            println!("  {} {} (MODIFIED)", "✗".red(), file);
            all_ok = false;
        }
    }

    if all_ok {
        println!("\n{}", "Tracked files match the backup manifest (not Steam-verified).".green().bold());
    } else {
        println!(
            "\n{}",
            "Some files have been modified. Review and restore them; verify with Steam before official play."
                .yellow()
                .bold()
        );
    }

    Ok(())
}

fn cmd_init() -> Result<()> {
    mod_manager::init_mod_dirs()?;
    println!("{} Mod directories created at:", "Done!".green().bold());
    println!("  Mods:    {}", mod_manager::installed_mods_dir().display());
    println!("  Profiles: {}", mod_manager::profiles_dir().display());
    Ok(())
}

fn cmd_new_mod(name: &str, category: &str) -> Result<()> {
    let id = name
        .to_lowercase()
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>();

    let mod_dir = mod_manager::installed_mods_dir().join(&id);
    std::fs::create_dir_all(&mod_dir)?;

    let template = format!(
        r#"[mod]
name = "{name}"
id = "{id}"
author = "You"
version = "1.0.0"
description = "A custom {category} mod"
category = "{category}"
target_zones = ["{zone}"]

# Configurable options shown in the in-game HUD menu.
# Supported types: toggle, slider, choice, number.

[[options]]
id = "enabled"
label = "Enable Mod"
type = "toggle"
default = true
description = "Master switch for this mod"

[[options]]
id = "intensity"
label = "Effect Intensity"
type = "slider"
default = 1.0
min = 0.0
max = 5.0
step = 0.5
description = "How strong the mod effect is"
group = "Gameplay"

# Patches applied to zone file data (binary or string replacements).
# [[patches]]
# file = "my_patch.json"

# GSC scripts injected into zone files.
# [[scripts]]
# file = "scripts/my_mod.gsc"
# target_zone = "{zone}"
"#,
        name = name,
        id = id,
        category = category,
        zone = match category {
            "zombies" => "zm_common",
            "multiplayer" => "mp_common",
            _ => "core_common",
        }
    );

    std::fs::write(mod_dir.join("mod.toml"), template)?;

    let script_dir = mod_dir.join("scripts");
    std::fs::create_dir_all(&script_dir)?;

    let gsc_template = format!(
        r#"#namespace {id};

// {name} - Main script
// Options are available via level.mod_menu at runtime.
// Listen for changes: level waittill("mod_option_changed", mod_name, opt_id, value);

function autoexec main() {{
    level waittill("connected", player);
    player thread on_spawned();
}}

function on_spawned() {{
    self endon("disconnect");

    for (;;) {{
        self waittill("spawned_player");
        // Your mod logic here
    }}
}}
"#,
        id = id,
        name = name,
    );

    std::fs::write(script_dir.join(format!("{}.gsc", id)), gsc_template)?;

    println!("{} Created mod '{}' at:", "Done!".green().bold(), name);
    println!("  {}", mod_dir.display());
    println!("\nEdit {} to configure.", "mod.toml".yellow());

    Ok(())
}

fn cmd_install(path: &PathBuf) -> Result<()> {
    let mod_id = mod_manager::install_mod(path)?;
    println!("{} Installed mod '{}'", "Done!".green().bold(), mod_id);
    Ok(())
}

fn cmd_uninstall(mod_id: &str) -> Result<()> {
    mod_manager::uninstall_mod(mod_id)?;
    println!("{} Uninstalled '{}'", "Done!".green().bold(), mod_id);
    Ok(())
}

fn cmd_list() -> Result<()> {
    let mods = mod_manager::list_installed_mods()?;

    if mods.is_empty() {
        println!("No mods installed. Use 'init' then 'new' to create one.");
        return Ok(());
    }

    println!("{}", "Installed Mods".cyan().bold());
    println!("{}", "─".repeat(60));
    for (_path, manifest) in &mods {
        println!(
            "  {} {} v{} by {}",
            "●".green(),
            manifest.info.name.bold(),
            manifest.info.version,
            manifest.info.author
        );
        if !manifest.info.description.is_empty() {
            println!("    {}", manifest.info.description.dimmed());
        }
        println!(
            "    ID: {}  Category: {}  Options: {}",
            manifest.info.id.yellow(),
            manifest.info.category,
            manifest.options.len()
        );
    }

    Ok(())
}

fn cmd_apply(
    game: &PathBuf,
    zone_path: &PathBuf,
    oodle: &PathBuf,
    profile_name: &str,
    force_offline: bool,
) -> Result<()> {
    backup::pre_apply_check(game, force_offline)?;

    let mods = mod_manager::list_installed_mods()?;
    if mods.is_empty() {
        println!("No mods installed.");
        return Ok(());
    }

    let _profile = mod_manager::load_profile(profile_name)?;

    let mut active: Vec<(mod_manager::ModManifest, HashMap<String, mod_manager::OptionValue>)> =
        Vec::new();

    for (_path, manifest) in &mods {
        let preset_store = mod_manager::PresetStore::load(&manifest.info.id)?;
        let values = preset_store.get_active_values(manifest);
        active.push((manifest.clone(), values));
    }

    let conflicts = mod_manager::detect_conflicts(&active);
    if !conflicts.is_empty() {
        println!("{}", "Mod Conflicts Detected:".yellow().bold());
        for c in &conflicts {
            println!(
                "  {} {} <-> {}: {}",
                "⚠".yellow(),
                c.mod_a,
                c.mod_b,
                c.detail
            );
        }
        println!();
    }

    let hud_gsc = mod_manager::generate_mod_menu_gsc(&active);
    let hud_path = mod_manager::mods_dir().join("_generated_mod_menu.gsc");
    std::fs::create_dir_all(hud_path.parent().unwrap())?;
    std::fs::write(&hud_path, &hud_gsc)?;
    println!("  {} Generated in-game mod HUD", "✓".green());

    // Group patches by target zone so we decompress each zone once
    let mut zone_patches: HashMap<String, Vec<(String, patch::PatchSet)>> = HashMap::new();

    for (manifest, _values) in &active {
        println!(
            "\n  Collecting patches: {} v{}",
            manifest.info.name.bold(),
            manifest.info.version
        );

        for patch_ref in &manifest.patches {
            let patch_path = mod_manager::installed_mods_dir()
                .join(&manifest.info.id)
                .join(&patch_ref.file);

            if !patch_path.exists() {
                eprintln!("    {} Patch file not found: {}", "✗".red(), patch_ref.file);
                continue;
            }

            let patch_set = patch::load_patch_set(&patch_path)?;
            let target = patch_set.target_zone.clone();
            zone_patches
                .entry(target)
                .or_default()
                .push((manifest.info.id.clone(), patch_set));
        }
    }

    // Apply patches per zone: decompress -> patch -> recompress
    let mut modified_files = Vec::new();

    for (zone_name, patches) in &zone_patches {
        let zone_file = format!("zone\\{}.ff", zone_name);
        let ff_path = zone_path.join(format!("{}.ff", zone_name));

        // Never modify a zone when its backup could not be created.
        backup::backup_file(game, &zone_file)?;

        println!("\n  {} Decompressing {}...", "→".cyan(), zone_name);
        let zf = zone::ZoneFile::open(&ff_path)?;
        let decompressed = zf.decompress(oodle)?;
        let mut data = decompressed.data;
        println!("    Decompressed: {} bytes", data.len());

        let mut total_applied = 0;
        for (mod_id, patch_set) in patches {
            let mut applied = 0;
            for bp in &patch_set.binary_patches {
                if patch::apply_binary_patch(&mut data, bp)? {
                    applied += 1;
                }
            }
            for sp in &patch_set.string_patches {
                applied += patch::apply_string_patch(&mut data, sp)?;
            }
            println!("    {} [{}] Applied {} patches", "✓".green(), mod_id, applied);
            total_applied += applied;
        }

        if total_applied > 0 {
            println!("  {} Recompressing {}...", "→".cyan(), zone_name);
            let recompressed = zone::recompress_zone(&zf, &data, oodle)?;
            println!(
                "    {} bytes -> {} bytes ({:.1}%)",
                zf.file_size(),
                recompressed.len(),
                recompressed.len() as f64 / zf.file_size() as f64 * 100.0
            );
            std::fs::write(&ff_path, &recompressed)?;
            modified_files.push(zone_file);
            println!("    {} Written to {}", "✓".green(), ff_path.display());
        }
    }

    let state = backup::ModState {
        mods_active: true,
        active_mods: active.iter().map(|(m, _)| m.info.id.clone()).collect(),
        modified_files,
        last_applied: Some(chrono::Utc::now().to_rfc3339()),
        offline_verified: !force_offline,
    };
    backup::save_state(game, &state)?;

    println!(
        "\n{} {} mod(s) applied.",
        "Done!".green().bold(),
        active.len(),
    );
    println!(
        "{}",
        "Remember: remove all modifications and verify files before official play!".yellow()
    );

    Ok(())
}

fn cmd_clean(game: &PathBuf) -> Result<()> {
    backup::safety_check(game)?;

    let restored = backup::restore_all(game)?;

    if restored.is_empty() {
        println!("No tracked zone files to restore.");
    } else {
        println!(
            "{} Restored {} tracked zone file(s) from backups.",
            "Done!".green().bold(),
            restored.len()
        );
    }

    let inspection = backup::inspect_install(game)?;
    if !inspection.local_dlls.is_empty() {
        anyhow::bail!(
            "Game-local XInput file(s) remain: {}. 'clean' does not remove DLLs; inspect them before launching the game. Steam's Verify integrity of game files can repair game content.",
            inspection.local_dlls.iter().map(|dll| dll.name).collect::<Vec<_>>().join(", ")
        );
    }
    println!("This is not a full integrity check. Verify game files through Steam before going online.");
    Ok(())
}

fn cmd_status(game: &PathBuf) -> Result<()> {
    let inspection = backup::inspect_install(game)?;
    let state = &inspection.state;

    println!("{}", "Mod Status".cyan().bold());
    println!("{}", "─".repeat(40));

    if state.mods_active {
        println!(
            "  Status: {}",
            "MODS ACTIVE".yellow().bold()
        );
        println!("  Active mods:");
        for m in &state.active_mods {
            println!("    - {}", m);
        }
        println!("  Modified files:");
        for f in &state.modified_files {
            println!("    - {}", f);
        }
        if let Some(ref ts) = state.last_applied {
            println!("  Applied at: {}", ts);
        }
        println!(
            "\n  {}",
            "Remove mods and proxy, then verify game files before official play.".red().bold()
        );
    } else {
        println!("  Status: {}", "NO TRACKED ZONE MODS (not verified stock)".yellow());
    }

    for dll in &inspection.local_dlls {
        println!(
            "  {} Game-local {}{}: review before launch",
            "WARNING:".red().bold(),
            dll.name,
            if dll.has_backup { " (.bak exists)" } else { " (no .bak)" }
        );
    }

    for file in &inspection.tracked_files {
        if !file.backup_exists {
            println!("  {} Missing backup for {}", "WARNING:".red().bold(), file.path);
        } else if !file.matches_backup {
            println!("  {} Differs from backup: {}", "MODIFIED:".yellow(), file.path);
        }
    }

    let game_running = backup::is_game_running();
    println!(
        "  Game running: {}",
        if game_running { "Yes".yellow() } else { "No".green() }
    );

    Ok(())
}

fn cmd_doctor(game: &PathBuf) -> Result<()> {
    println!("{}", "Read-only install diagnostics".cyan().bold());
    println!("  Game directory: {}", game.display());
    if !game.is_dir() {
        anyhow::bail!("Game directory not found. Use --game-dir to select the correct installation.");
    }
    cmd_status(game)?;

    let inspection = backup::inspect_install(game)?;
    if inspection.local_dlls.is_empty() {
        println!("  No game-local XInput DLLs detected (not a complete mod scan).");
    }
    if game.join("t9-proxy.log").exists() {
        println!("  t9-proxy.log exists: a proxy ran here at some point (not proof it is running now).");
    }
    if inspection.state.mods_active && inspection.tracked_files.is_empty() {
        println!("  WARNING: mods_active is set, but no modified files are tracked.");
    }
    println!("  To check the original game files, use Steam > Properties > Installed Files > Verify integrity.");
    Ok(())
}

fn cmd_preset(action: PresetAction) -> Result<()> {
    match action {
        PresetAction::List { mod_id } => {
            let store = mod_manager::PresetStore::load(&mod_id)?;
            if store.presets.is_empty() {
                println!("No presets saved for '{}'.", mod_id);
            } else {
                println!("Presets for '{}':", mod_id);
                for p in &store.presets {
                    let marker = if store.active_preset.as_deref() == Some(&p.name) {
                        "●".green()
                    } else {
                        "○".dimmed()
                    };
                    let default = if p.is_default { " (default)" } else { "" };
                    println!("  {} {}{}", marker, p.name, default.dimmed());
                }
            }
        }
        PresetAction::Save {
            mod_id,
            preset_name,
        } => {
            let mod_dir = mod_manager::installed_mods_dir().join(&mod_id);
            let manifest = mod_manager::load_mod(&mod_dir)?;
            let mut store = mod_manager::PresetStore::load(&mod_id)?;

            let values: HashMap<String, mod_manager::OptionValue> = manifest
                .options
                .iter()
                .map(|o| (o.id.clone(), o.default.clone()))
                .collect();

            store.add_preset(&preset_name, values);
            store.save()?;
            println!(
                "{} Saved preset '{}' for '{}'",
                "Done!".green().bold(),
                preset_name,
                mod_id
            );
        }
        PresetAction::Load {
            mod_id,
            preset_name,
        } => {
            let mut store = mod_manager::PresetStore::load(&mod_id)?;
            if !store.presets.iter().any(|p| p.name == preset_name) {
                anyhow::bail!("Preset '{}' not found for '{}'", preset_name, mod_id);
            }
            store.active_preset = Some(preset_name.clone());
            store.save()?;
            println!(
                "{} Loaded preset '{}' for '{}'",
                "Done!".green().bold(),
                preset_name,
                mod_id
            );
        }
    }
    Ok(())
}

fn cmd_gen_hud(output: &PathBuf) -> Result<()> {
    let mods = mod_manager::list_installed_mods()?;

    let mut active: Vec<(mod_manager::ModManifest, HashMap<String, mod_manager::OptionValue>)> =
        Vec::new();

    for (_path, manifest) in &mods {
        let store = mod_manager::PresetStore::load(&manifest.info.id)?;
        let values = store.get_active_values(manifest);
        active.push((manifest.clone(), values));
    }

    let gsc = mod_manager::generate_mod_menu_gsc(&active);
    std::fs::write(output, &gsc)?;
    println!(
        "{} Generated HUD script: {}",
        "Done!".green().bold(),
        output.display()
    );
    println!("  {} mod(s) registered, press F8 in-game to open.", active.len());

    Ok(())
}

fn cmd_server(action: ServerAction) -> Result<()> {
    match action {
        ServerAction::Start => {
            println!("{}", "T9 DemonWare Emulator".cyan().bold());
            println!("{}", "─".repeat(40));

            let server_bin = std::env::current_exe()?
                .parent()
                .unwrap()
                .join("t9-server.exe");

            if !server_bin.exists() {
                // Try the cargo target directory
                let cargo_bin = PathBuf::from("target/debug/t9-server.exe");
                if !cargo_bin.exists() {
                    let release_bin = PathBuf::from("target/release/t9-server.exe");
                    if !release_bin.exists() {
                        anyhow::bail!(
                            "t9-server binary not found. Build it first:\n  \
                             cargo build -p t9-server"
                        );
                    }
                    return start_server(&release_bin);
                }
                return start_server(&cargo_bin);
            }
            start_server(&server_bin)
        }
        ServerAction::Status => {
            println!("{}", "Server Status".cyan().bold());
            println!("{}", "─".repeat(40));

            let ports = [
                ("Auth (login service)", 3100u16),
                ("Lobby", 3200),
                ("STUN (UDP)", 3074),
                ("Umbrella", 3300),
            ];

            for (name, port) in &ports {
                if *port == 3074 {
                    println!("  {} {} (UDP port {}; not checked via TCP)", "?".yellow(), name, port);
                    continue;
                }
                let addr = format!("127.0.0.1:{}", port);
                let reachable = std::net::TcpStream::connect_timeout(
                    &addr.parse().unwrap(),
                    std::time::Duration::from_millis(500),
                )
                .is_ok();

                if reachable {
                    println!("  {} {} (port {})", "●".green(), name, port);
                } else {
                    println!("  {} {} (port {})", "○".red(), name, port);
                }
            }

            Ok(())
        }
    }
}

fn start_server(path: &PathBuf) -> Result<()> {
    println!("  Starting: {}", path.display());
    println!("  Ports: auth=3100, lobby=3200, stun=3074, umbrella=3300");
    println!();

    let status = std::process::Command::new(path)
        .spawn();

    match status {
        Ok(mut child) => {
            println!(
                "  {} Server started (PID {})",
                "✓".green(),
                child.id()
            );
            println!(
                "  {}",
                "Server is running in the background. Press Ctrl+C to stop."
                    .dimmed()
            );
            // Wait for it so the user can see output / Ctrl+C it
            let _ = child.wait();
            Ok(())
        }
        Err(e) => {
            anyhow::bail!("Failed to start server: {}", e);
        }
    }
}

fn cmd_proxy(game: &PathBuf, action: ProxyAction) -> Result<()> {
    let proxy_dll_name = "XINPUT9_1_0.dll";
    let game_dll = game.join(proxy_dll_name);
    let backup_dll = game.join("XINPUT9_1_0.dll.bak");

    // Where cargo builds the cdylib
    let built_dll = {
        let debug = PathBuf::from("target/debug/t9_proxy.dll");
        let release = PathBuf::from("target/release/t9_proxy.dll");
        if release.exists() {
            release
        } else {
            debug
        }
    };

    match action {
        ProxyAction::Install => {
            println!("{}", "Proxy DLL Install".cyan().bold());
            println!("{}", "─".repeat(40));

            if !built_dll.exists() {
                anyhow::bail!(
                    "Proxy DLL not found at {}. Build it first:\n  \
                     cargo build -p t9-proxy",
                    built_dll.display()
                );
            }

            if !game.exists() {
                anyhow::bail!("Game directory not found: {}", game.display());
            }

            if backup::is_game_running() {
                anyhow::bail!("Close the game before changing its DLLs.");
            }

            if game_dll.exists() && !backup_dll.exists()
                && backup::sha256_file(&game_dll)? == backup::sha256_file(&built_dll)?
            {
                anyhow::bail!(
                    "{} already matches the built proxy but has no original backup. Refusing to back up a proxy as though it were the original DLL.",
                    game_dll.display()
                );
            }

            if game_dll.exists() && backup_dll.exists()
                && backup::sha256_file(&game_dll)? != backup::sha256_file(&built_dll)?
            {
                anyhow::bail!("{} differs from the current proxy build; refusing to overwrite an unknown file.", game_dll.display());
            }

            // Back up the original XInput DLL if it exists and we haven't already
            if game_dll.exists() && !backup_dll.exists() {
                std::fs::copy(&game_dll, &backup_dll)?;
                println!(
                    "  {} Backed up original {} -> {}",
                    "✓".green(),
                    proxy_dll_name,
                    "XINPUT9_1_0.dll.bak"
                );
            }

            // Copy our proxy DLL into the game directory
            std::fs::copy(&built_dll, &game_dll)?;
            println!(
                "  {} Installed proxy DLL as {}",
                "✓".green(),
                game_dll.display()
            );
            println!(
                "\n  {}",
                "Start the emulator server before launching the game!"
                    .yellow()
                    .bold()
            );

            Ok(())
        }
        ProxyAction::Remove => {
            println!("{}", "Proxy DLL Remove".cyan().bold());
            println!("{}", "─".repeat(40));

            if backup::is_game_running() {
                anyhow::bail!("Close the game before changing its DLLs.");
            }

            if backup_dll.exists() {
                if game_dll.exists() {
                    if !built_dll.exists()
                        || backup::sha256_file(&game_dll)? != backup::sha256_file(&built_dll)?
                    {
                        anyhow::bail!(
                            "{} does not match the current proxy build; refusing to overwrite an unknown DLL. Review it manually.",
                            game_dll.display()
                        );
                    }
                    if backup::sha256_file(&backup_dll)? == backup::sha256_file(&game_dll)? {
                        anyhow::bail!("Backup {} also matches the proxy; refusing to restore it as original.", backup_dll.display());
                    }
                }
                std::fs::copy(&backup_dll, &game_dll)?;
                std::fs::remove_file(&backup_dll)?;
                println!(
                    "  {} Restored original {}",
                    "✓".green(),
                    proxy_dll_name
                );
            } else if game_dll.exists() {
                anyhow::bail!(
                    "No original backup exists for {}. Refusing to delete an unbacked DLL. Inspect it before removing it manually, then verify game files through Steam.",
                    game_dll.display()
                );
            } else {
                println!("  Proxy DLL not installed.");
            }

            Ok(())
        }
        ProxyAction::Status => {
            println!("{}", "Proxy DLL Status".cyan().bold());
            println!("{}", "─".repeat(40));

            if !game.exists() {
                println!("  Game directory: {} (NOT FOUND)", game.display());
                return Ok(());
            }

            if game_dll.exists() {
                let meta = std::fs::metadata(&game_dll)?;
                let size = meta.len();
                println!("  {} exists ({} bytes)", proxy_dll_name, size);
            } else {
                println!("  {} not found in game directory", proxy_dll_name);
            }

            if game_dll.exists() && backup_dll.exists() {
                println!(
                    "  Status: {} (backup exists; contents not authenticated)",
                    "GAME-LOCAL DLL".yellow().bold()
                );
            } else if game_dll.exists() {
                println!("  Status: {} (no original backup)", "GAME-LOCAL DLL".yellow().bold());
            } else if backup_dll.exists() {
                println!(
                    "  Status: {} (backup exists but proxy not active)",
                    "INCONSISTENT".red().bold()
                );
            } else {
                println!("  Status: {} (not proof of stock files)", "NO GAME-LOCAL DLL".green());
            }

            Ok(())
        }
    }
}
