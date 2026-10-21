use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};


const MODS_DIR_NAME: &str = "bocw_mods";

// ─── Mod manifest ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModManifest {
    #[serde(rename = "mod")]
    pub info: ModInfo,
    #[serde(default)]
    pub options: Vec<ModOption>,
    #[serde(default)]
    pub patches: Vec<ModPatchRef>,
    #[serde(default)]
    pub scripts: Vec<ModScript>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    pub name: String,
    pub id: String,
    pub author: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub target_zones: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModOption {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub opt_type: ModOptionType,
    pub default: OptionValue,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModOptionType {
    Toggle,
    Slider,
    Choice,
    Number,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    Bool(bool),
    Float(f64),
    Int(i64),
    Str(String),
}

impl OptionValue {
    pub fn to_gsc_literal(&self) -> String {
        match self {
            OptionValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            OptionValue::Float(f) => format!("{}", f),
            OptionValue::Int(i) => format!("{}", i),
            OptionValue::Str(s) => gsc_string(s),
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            OptionValue::Bool(b) => *b,
            OptionValue::Float(f) => *f != 0.0,
            OptionValue::Int(i) => *i != 0,
            OptionValue::Str(s) => !s.is_empty(),
        }
    }
}

/// Quote `s` as a GSC string literal.
pub fn gsc_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModPatchRef {
    pub file: String,
    /// Option id that must be truthy for the patch to apply; `!id` inverts it.
    #[serde(default)]
    pub condition: Option<String>,
}

impl ModPatchRef {
    pub fn is_enabled(&self, values: &HashMap<String, OptionValue>) -> Result<bool> {
        let Some(condition) = self.condition.as_deref().map(str::trim).filter(|c| !c.is_empty())
        else {
            return Ok(true);
        };
        let (negate, id) = match condition.strip_prefix('!') {
            Some(id) => (true, id.trim()),
            None => (false, condition),
        };
        let value = values.get(id).with_context(|| {
            format!("Patch '{}': condition refers to unknown option '{}'", self.file, id)
        })?;
        Ok(value.is_truthy() != negate)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModScript {
    pub file: String,
    pub target_zone: String,
    #[serde(default)]
    pub inject_point: String,
}

// ─── Mod state ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModConfig {
    pub mod_id: String,
    pub enabled: bool,
    pub options: HashMap<String, OptionValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModProfile {
    pub name: String,
    pub mods: Vec<ModConfig>,
}

// ─── Manager ────────────────────────────────────────────────────────────────

pub fn mods_dir() -> PathBuf {
    let base = dirs_fallback();
    base.join(MODS_DIR_NAME)
}

pub fn installed_mods_dir() -> PathBuf {
    mods_dir().join("installed")
}

pub fn profiles_dir() -> PathBuf {
    mods_dir().join("profiles")
}

fn dirs_fallback() -> PathBuf {
    std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn init_mod_dirs() -> Result<()> {
    std::fs::create_dir_all(installed_mods_dir())?;
    std::fs::create_dir_all(profiles_dir())?;
    Ok(())
}

/// Resolve a file named in a manifest, refusing paths that leave the mod directory.
pub fn mod_file_path(mod_dir: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        bail!("Expected a path inside the mod directory: {}", relative);
    }
    Ok(mod_dir.join(path))
}

pub fn load_mod(mod_dir: &Path) -> Result<ModManifest> {
    let manifest_path = mod_dir.join("mod.toml");
    if !manifest_path.exists() {
        bail!("No mod.toml found in {}", mod_dir.display());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: ModManifest = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

    Ok(manifest)
}

pub fn list_installed_mods() -> Result<Vec<(PathBuf, ModManifest)>> {
    let dir = installed_mods_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut mods = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Ok(manifest) = load_mod(&path) {
                mods.push((path, manifest));
            }
        }
    }

    mods.sort_by(|a, b| a.1.info.name.cmp(&b.1.info.name));
    Ok(mods)
}

pub fn install_mod(source: &Path) -> Result<String> {
    let manifest = load_mod(source)?;
    let dest = installed_mods_dir().join(&manifest.info.id);

    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }

    copy_dir_recursive(source, &dest)?;

    Ok(manifest.info.id)
}

pub fn uninstall_mod(mod_id: &str) -> Result<()> {
    let dir = installed_mods_dir().join(mod_id);
    if !dir.exists() {
        bail!("Mod '{}' is not installed", mod_id);
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

pub fn load_profile(name: &str) -> Result<ModProfile> {
    let path = profiles_dir().join(format!("{}.json", name));
    if !path.exists() {
        return Ok(ModProfile {
            name: name.to_string(),
            mods: Vec::new(),
        });
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn save_profile(profile: &ModProfile) -> Result<()> {
    std::fs::create_dir_all(profiles_dir())?;
    let path = profiles_dir().join(format!("{}.json", profile.name));
    let content = serde_json::to_string_pretty(profile)?;
    std::fs::write(&path, content)?;
    Ok(())
}

// ─── In-game HUD GSC generation ─────────────────────────────────────────────

pub fn generate_mod_menu_gsc(active_mods: &[(ModManifest, HashMap<String, OptionValue>)]) -> String {
    let mut gsc = String::new();

    gsc.push_str("// AUTO-GENERATED by bocw_mod_tools - DO NOT EDIT\n");
    gsc.push_str("// In-game mod menu HUD\n\n");

    // Main init function
    gsc.push_str("#using scripts\\core_common\\callbacks_shared;\n");
    gsc.push_str("#using scripts\\core_common\\util_shared;\n\n");
    gsc.push_str("function autoexec mod_menu_init() {\n");
    gsc.push_str("    level.mod_menu = spawnStruct();\n");
    gsc.push_str("    level.mod_menu.is_open = false;\n");
    gsc.push_str("    level.mod_menu.cursor = 0;\n");
    gsc.push_str("    level.mod_menu.page = 0;\n");
    gsc.push_str(&format!(
        "    level.mod_menu.mod_count = {};\n",
        active_mods.len()
    ));
    gsc.push_str("    level.mod_menu.hud_elements = [];\n\n");

    // Register each mod's options
    gsc.push_str("    level.mod_menu.mods = [];\n");
    for (i, (manifest, opts)) in active_mods.iter().enumerate() {
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}] = spawnStruct();\n",
            i
        ));
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}].name = {};\n",
            i, gsc_string(&manifest.info.name)
        ));
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}].author = {};\n",
            i, gsc_string(&manifest.info.author)
        ));
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}].version = {};\n",
            i, gsc_string(&manifest.info.version)
        ));
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}].desc = {};\n",
            i, gsc_string(&manifest.info.description)
        ));

        // Register options for this mod
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}].options = [];\n",
            i
        ));
        for (j, opt) in manifest.options.iter().enumerate() {
            let current_val = opts
                .get(&opt.id)
                .unwrap_or(&opt.default);

            gsc.push_str(&format!(
                "    level.mod_menu.mods[{}].options[{}] = spawnStruct();\n",
                i, j
            ));
            gsc.push_str(&format!(
                "    level.mod_menu.mods[{}].options[{}].id = {};\n",
                i, j, gsc_string(&opt.id)
            ));
            gsc.push_str(&format!(
                "    level.mod_menu.mods[{}].options[{}].label = {};\n",
                i, j, gsc_string(&opt.label)
            ));
            gsc.push_str(&format!(
                "    level.mod_menu.mods[{}].options[{}].type = \"{}\";\n",
                i, j, option_type_str(&opt.opt_type)
            ));
            gsc.push_str(&format!(
                "    level.mod_menu.mods[{}].options[{}].value = {};\n",
                i, j, current_val.to_gsc_literal()
            ));

            if let Some(min) = opt.min {
                gsc.push_str(&format!(
                    "    level.mod_menu.mods[{}].options[{}].min = {};\n",
                    i, j, min
                ));
            }
            if let Some(max) = opt.max {
                gsc.push_str(&format!(
                    "    level.mod_menu.mods[{}].options[{}].max = {};\n",
                    i, j, max
                ));
            }
            if let Some(step) = opt.step {
                gsc.push_str(&format!(
                    "    level.mod_menu.mods[{}].options[{}].step = {};\n",
                    i, j, step
                ));
            }
        }
        gsc.push_str(&format!(
            "    level.mod_menu.mods[{}].option_count = {};\n\n",
            i,
            manifest.options.len()
        ));
    }

    gsc.push_str("    level callback::on_connect(&on_player_connect);\n");
    gsc.push_str("}\n\n");

    // Player connect - start HUD watcher thread
    gsc.push_str("function on_player_connect() {\n");
    gsc.push_str("    self thread mod_menu_think();\n");
    gsc.push_str("}\n\n");

    // Menu think loop - watches for keybind
    gsc.push_str("function mod_menu_think() {\n");
    gsc.push_str("    self endon(\"disconnect\");\n");
    gsc.push_str("    self endon(\"death\");\n\n");
    gsc.push_str("    for (;;) {\n");
    gsc.push_str("        // F8 = actionslot 8 to toggle menu\n");
    gsc.push_str("        if (self actionSlotEightButtonPressed()) {\n");
    gsc.push_str("            if (level.mod_menu.is_open) {\n");
    gsc.push_str("                self close_mod_menu();\n");
    gsc.push_str("            } else {\n");
    gsc.push_str("                self open_mod_menu();\n");
    gsc.push_str("            }\n");
    gsc.push_str("            wait 0.3;\n");
    gsc.push_str("        }\n\n");
    gsc.push_str("        if (level.mod_menu.is_open) {\n");
    gsc.push_str("            self handle_menu_input();\n");
    gsc.push_str("        }\n\n");
    gsc.push_str("        wait 0.05;\n");
    gsc.push_str("    }\n");
    gsc.push_str("}\n\n");

    // Open menu
    gsc.push_str("function open_mod_menu() {\n");
    gsc.push_str("    level.mod_menu.is_open = true;\n");
    gsc.push_str("    level.mod_menu.cursor = 0;\n");
    gsc.push_str("    level.mod_menu.page = 0;\n\n");
    gsc.push_str("    // Background overlay\n");
    gsc.push_str("    bg = newClientHudElem(self);\n");
    gsc.push_str("    bg.x = 0;\n");
    gsc.push_str("    bg.y = 0;\n");
    gsc.push_str("    bg.width = 300;\n");
    gsc.push_str("    bg.height = 480;\n");
    gsc.push_str("    bg.align = \"left\";\n");
    gsc.push_str("    bg.horzAlign = \"left\";\n");
    gsc.push_str("    bg.vertAlign = \"top\";\n");
    gsc.push_str("    bg setShader(\"black\", 300, 480);\n");
    gsc.push_str("    bg.alpha = 0.75;\n");
    gsc.push_str("    bg.sort = 10;\n");
    gsc.push_str("    level.mod_menu.hud_elements[level.mod_menu.hud_elements.size] = bg;\n\n");
    gsc.push_str("    // Title\n");
    gsc.push_str("    title = self createFontString(\"objective\", 1.6);\n");
    gsc.push_str("    title setPoint(\"TOPLEFT\", \"TOPLEFT\", 16, 16);\n");
    gsc.push_str(&format!(
        "    title setText(\"MODS ({})\");\n",
        active_mods.len()
    ));
    gsc.push_str("    title.color = (0.4, 0.8, 1.0);\n");
    gsc.push_str("    title.sort = 11;\n");
    gsc.push_str("    level.mod_menu.hud_elements[level.mod_menu.hud_elements.size] = title;\n\n");
    gsc.push_str("    // Keybind hint\n");
    gsc.push_str("    hint = self createFontString(\"small\", 1.0);\n");
    gsc.push_str("    hint setPoint(\"TOPLEFT\", \"TOPLEFT\", 16, 455);\n");
    gsc.push_str("    hint setText(\"[UP/DN] Navigate  [USE] Toggle  [F8] Close\");\n");
    gsc.push_str("    hint.alpha = 0.6;\n");
    gsc.push_str("    hint.sort = 11;\n");
    gsc.push_str("    level.mod_menu.hud_elements[level.mod_menu.hud_elements.size] = hint;\n\n");
    gsc.push_str("    self refresh_menu_display();\n");
    gsc.push_str("}\n\n");

    // Close menu
    gsc.push_str("function close_mod_menu() {\n");
    gsc.push_str("    level.mod_menu.is_open = false;\n");
    gsc.push_str("    for (i = 0; i < level.mod_menu.hud_elements.size; i++) {\n");
    gsc.push_str("        level.mod_menu.hud_elements[i] destroy();\n");
    gsc.push_str("    }\n");
    gsc.push_str("    level.mod_menu.hud_elements = [];\n");
    gsc.push_str("}\n\n");

    // Input handler
    gsc.push_str("function handle_menu_input() {\n");
    gsc.push_str("    total_items = get_total_menu_items();\n\n");
    gsc.push_str("    if (self adsButtonPressed()) {\n");
    gsc.push_str("        // Up\n");
    gsc.push_str("        level.mod_menu.cursor--;\n");
    gsc.push_str("        if (level.mod_menu.cursor < 0)\n");
    gsc.push_str("            level.mod_menu.cursor = total_items - 1;\n");
    gsc.push_str("        self refresh_menu_display();\n");
    gsc.push_str("        wait 0.15;\n");
    gsc.push_str("    }\n\n");
    gsc.push_str("    if (self attackButtonPressed()) {\n");
    gsc.push_str("        // Down\n");
    gsc.push_str("        level.mod_menu.cursor++;\n");
    gsc.push_str("        if (level.mod_menu.cursor >= total_items)\n");
    gsc.push_str("            level.mod_menu.cursor = 0;\n");
    gsc.push_str("        self refresh_menu_display();\n");
    gsc.push_str("        wait 0.15;\n");
    gsc.push_str("    }\n\n");
    gsc.push_str("    if (self useButtonPressed()) {\n");
    gsc.push_str("        toggle_current_option();\n");
    gsc.push_str("        self refresh_menu_display();\n");
    gsc.push_str("        wait 0.2;\n");
    gsc.push_str("    }\n");
    gsc.push_str("}\n\n");

    // Refresh display
    gsc.push_str("function refresh_menu_display() {\n");
    gsc.push_str("    // Clear old item HUD elements (keep bg, title, hint = first 3)\n");
    gsc.push_str("    kept = [];\n");
    gsc.push_str("    for (i = 0; i < level.mod_menu.hud_elements.size; i++) {\n");
    gsc.push_str("        if (i < 3) {\n");
    gsc.push_str("            kept[kept.size] = level.mod_menu.hud_elements[i];\n");
    gsc.push_str("        } else {\n");
    gsc.push_str("            level.mod_menu.hud_elements[i] destroy();\n");
    gsc.push_str("        }\n");
    gsc.push_str("    }\n");
    gsc.push_str("    level.mod_menu.hud_elements = kept;\n\n");
    gsc.push_str("    y_pos = 48;\n");
    gsc.push_str("    item_index = 0;\n\n");
    gsc.push_str("    for (m = 0; m < level.mod_menu.mod_count; m++) {\n");
    gsc.push_str("        mod = level.mod_menu.mods[m];\n\n");
    gsc.push_str("        // Mod header\n");
    gsc.push_str("        header = self createFontString(\"objective\", 1.2);\n");
    gsc.push_str("        header setPoint(\"TOPLEFT\", \"TOPLEFT\", 16, y_pos);\n");
    gsc.push_str("        header setText(mod.name + \" v\" + mod.version);\n");
    gsc.push_str("        header.color = (1.0, 0.85, 0.3);\n");
    gsc.push_str("        header.sort = 11;\n");
    gsc.push_str("        level.mod_menu.hud_elements[level.mod_menu.hud_elements.size] = header;\n");
    gsc.push_str("        y_pos += 22;\n\n");
    gsc.push_str("        // Options\n");
    gsc.push_str("        for (o = 0; o < mod.option_count; o++) {\n");
    gsc.push_str("            opt = mod.options[o];\n");
    gsc.push_str("            line = self createFontString(\"small\", 1.1);\n");
    gsc.push_str("            line setPoint(\"TOPLEFT\", \"TOPLEFT\", 28, y_pos);\n\n");
    gsc.push_str("            val_str = \"\";\n");
    gsc.push_str("            if (opt.type == \"toggle\") {\n");
    gsc.push_str("                val_str = opt.value ? \"^2ON\" : \"^1OFF\";\n");
    gsc.push_str("            } else {\n");
    gsc.push_str("                val_str = \"\" + opt.value;\n");
    gsc.push_str("            }\n\n");
    gsc.push_str("            prefix = (item_index == level.mod_menu.cursor) ? \"^5> \" : \"  \";\n");
    gsc.push_str("            line setText(prefix + opt.label + \": \" + val_str);\n");
    gsc.push_str("            line.sort = 11;\n\n");
    gsc.push_str("            if (item_index == level.mod_menu.cursor) {\n");
    gsc.push_str("                line.color = (1.0, 1.0, 1.0);\n");
    gsc.push_str("            } else {\n");
    gsc.push_str("                line.color = (0.7, 0.7, 0.7);\n");
    gsc.push_str("            }\n\n");
    gsc.push_str("            level.mod_menu.hud_elements[level.mod_menu.hud_elements.size] = line;\n");
    gsc.push_str("            y_pos += 18;\n");
    gsc.push_str("            item_index++;\n");
    gsc.push_str("        }\n\n");
    gsc.push_str("        y_pos += 8; // spacing between mods\n");
    gsc.push_str("    }\n");
    gsc.push_str("}\n\n");

    // Total items helper
    gsc.push_str("function get_total_menu_items() {\n");
    gsc.push_str("    total = 0;\n");
    gsc.push_str("    for (m = 0; m < level.mod_menu.mod_count; m++) {\n");
    gsc.push_str("        total += level.mod_menu.mods[m].option_count;\n");
    gsc.push_str("    }\n");
    gsc.push_str("    return total;\n");
    gsc.push_str("}\n\n");

    // Toggle option
    gsc.push_str("function toggle_current_option() {\n");
    gsc.push_str("    idx = 0;\n");
    gsc.push_str("    for (m = 0; m < level.mod_menu.mod_count; m++) {\n");
    gsc.push_str("        mod = level.mod_menu.mods[m];\n");
    gsc.push_str("        for (o = 0; o < mod.option_count; o++) {\n");
    gsc.push_str("            if (idx == level.mod_menu.cursor) {\n");
    gsc.push_str("                opt = mod.options[o];\n");
    gsc.push_str("                if (opt.type == \"toggle\") {\n");
    gsc.push_str("                    opt.value = !opt.value;\n");
    gsc.push_str("                } else if (opt.type == \"slider\" || opt.type == \"number\") {\n");
    gsc.push_str("                    step = isDefined(opt.step) ? opt.step : 1;\n");
    gsc.push_str("                    opt.value += step;\n");
    gsc.push_str("                    if (isDefined(opt.max) && opt.value > opt.max)\n");
    gsc.push_str("                        opt.value = isDefined(opt.min) ? opt.min : 0;\n");
    gsc.push_str("                }\n");
    gsc.push_str("                // Notify mod scripts that an option changed\n");
    gsc.push_str("                level notify(\"mod_option_changed\", mod.name, opt.id, opt.value);\n");
    gsc.push_str("                return;\n");
    gsc.push_str("            }\n");
    gsc.push_str("            idx++;\n");
    gsc.push_str("        }\n");
    gsc.push_str("    }\n");
    gsc.push_str("}\n");

    gsc
}

fn option_type_str(t: &ModOptionType) -> &'static str {
    match t {
        ModOptionType::Toggle => "toggle",
        ModOptionType::Slider => "slider",
        ModOptionType::Choice => "choice",
        ModOptionType::Number => "number",
    }
}

// ─── Presets ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub is_default: bool,
    pub values: HashMap<String, OptionValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetStore {
    pub mod_id: String,
    pub presets: Vec<Preset>,
    pub active_preset: Option<String>,
}

impl PresetStore {
    pub fn new(mod_id: &str) -> Self {
        Self {
            mod_id: mod_id.to_string(),
            presets: Vec::new(),
            active_preset: None,
        }
    }

    pub fn load(mod_id: &str) -> Result<Self> {
        let path = mods_dir()
            .join("presets")
            .join(format!("{}.json", mod_id));
        if !path.exists() {
            return Ok(Self::new(mod_id));
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        let dir = mods_dir().join("presets");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", self.mod_id));
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add_preset(&mut self, name: &str, values: HashMap<String, OptionValue>) {
        self.presets.retain(|p| p.name != name);
        self.presets.push(Preset {
            name: name.to_string(),
            is_default: self.presets.is_empty(),
            values,
        });
    }

    pub fn get_active_values(&self, manifest: &ModManifest) -> HashMap<String, OptionValue> {
        let mut values = HashMap::new();

        for opt in &manifest.options {
            values.insert(opt.id.clone(), opt.default.clone());
        }

        if let Some(preset_name) = &self.active_preset {
            if let Some(preset) = self.presets.iter().find(|p| &p.name == preset_name) {
                for (k, v) in &preset.values {
                    values.insert(k.clone(), v.clone());
                }
            }
        }

        values
    }
}

// ─── Build pipeline (Substitute → Link → Guard → Output) ───────────────────

pub fn substitute_settings_markers(
    source: &str,
    values: &HashMap<String, OptionValue>,
) -> String {
    let mut output = source.to_string();

    // Strip SETTINGS declaration blocks: /*[[[SETTINGS]]] ... [[[/SETTINGS]]]*/
    while let Some(start) = output.find("/*[[[SETTINGS]]]") {
        if let Some(end) = output[start..].find("[[[/SETTINGS]]]*/") {
            let block_end = start + end + "[[[/SETTINGS]]]*/".len();
            output.replace_range(start..block_end, "");
        } else {
            break;
        }
    }

    // Strip PRESETS declaration blocks
    while let Some(start) = output.find("/*[[[PRESETS]]]") {
        if let Some(end) = output[start..].find("[[[/PRESETS]]]*/") {
            let block_end = start + end + "[[[/PRESETS]]]*/".len();
            output.replace_range(start..block_end, "");
        } else {
            break;
        }
    }

    // Replace inline markers [[[id]]] with their GSC literal values
    for (id, val) in values {
        let marker = format!("[[[{}]]]", id);
        let literal = val.to_gsc_literal();
        output = output.replace(&marker, &literal);
    }

    output
}

pub fn link_mod_scripts(
    _mod_id: &str,
    namespace: &str,
    scripts: &[(String, String)], // (filename, source)
) -> String {
    let mut linked = String::new();
    linked.push_str(&format!("#namespace {};\n\n", namespace));

    for (filename, source) in scripts {
        linked.push_str(&format!("// --- {} ---\n", filename));

        let processed = source
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.starts_with("#namespace")
            })
            .collect::<Vec<_>>()
            .join("\n");

        linked.push_str(&processed);
        linked.push_str("\n\n");
    }

    linked
}

pub fn insert_offline_guard(source: &str) -> String {
    let guard = r#"
// Offline-only safety guard
function autoexec __bocw_mod_guard() {
    if (!isDefined(level.mod_menu)) return;
    if (getDvarString("online_enabled") == "1") {
        iPrintLnBold("^1MODS DISABLED: Online play detected");
        level notify("mod_option_changed", "__system", "enabled", false);
    }
}
"#;

    format!("{}\n{}", source, guard)
}

/// `[[[id]]]` markers left after substitution, i.e. ids with no option value.
pub fn unresolved_markers(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find("[[[") {
        let after = &rest[start + 3..];
        match after.find("]]]") {
            Some(end) => {
                found.push(after[..end].to_string());
                rest = &after[end + 3..];
            }
            None => break,
        }
    }
    found
}

/// One mod's scripts for one zone after Substitute → Link → Guard.
#[derive(Debug, Clone)]
pub struct BuiltScript {
    pub mod_id: String,
    pub target_zone: String,
    pub source: String,
}

pub fn build_mod_scripts(
    mod_dir: &Path,
    manifest: &ModManifest,
    values: &HashMap<String, OptionValue>,
) -> Result<Vec<BuiltScript>> {
    let mut by_zone: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();

    for script in &manifest.scripts {
        let path = mod_file_path(mod_dir, &script.file)?;
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read script {}", path.display()))?;
        let substituted = substitute_settings_markers(&source, values);
        let unresolved = unresolved_markers(&substituted);
        if !unresolved.is_empty() {
            bail!(
                "Script {} uses unknown setting marker(s): {}",
                script.file,
                unresolved.join(", ")
            );
        }
        by_zone
            .entry(script.target_zone.as_str())
            .or_default()
            .push((script.file.clone(), substituted));
    }

    Ok(by_zone
        .into_iter()
        .map(|(zone, scripts)| BuiltScript {
            mod_id: manifest.info.id.clone(),
            target_zone: zone.to_string(),
            source: insert_offline_guard(&link_mod_scripts(
                &manifest.info.id,
                &manifest.info.id,
                &scripts,
            )),
        })
        .collect())
}

// ─── Conflict detection ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ModConflict {
    pub mod_a: String,
    pub mod_b: String,
    pub conflict_type: ConflictType,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum ConflictType {
    TargetZoneOverlap,
    NamespaceCollision,
    PatchOverlap,
}

impl std::fmt::Display for ConflictType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConflictType::TargetZoneOverlap => write!(f, "zone overlap"),
            ConflictType::NamespaceCollision => write!(f, "namespace collision"),
            ConflictType::PatchOverlap => write!(f, "patch overlap"),
        }
    }
}

pub fn detect_conflicts(mods: &[(ModManifest, HashMap<String, OptionValue>)]) -> Vec<ModConflict> {
    let mut conflicts = Vec::new();

    for (i, (mod_a, _)) in mods.iter().enumerate() {
        for (mod_b, _) in mods.iter().skip(i + 1) {
            // Check target zone overlap
            for zone_a in &mod_a.info.target_zones {
                for zone_b in &mod_b.info.target_zones {
                    if zone_a == zone_b {
                        conflicts.push(ModConflict {
                            mod_a: mod_a.info.id.clone(),
                            mod_b: mod_b.info.id.clone(),
                            conflict_type: ConflictType::TargetZoneOverlap,
                            detail: format!("Both target zone '{}'", zone_a),
                        });
                    }
                }
            }

            // Check patch file overlap
            for patch_a in &mod_a.patches {
                for patch_b in &mod_b.patches {
                    if patch_a.file == patch_b.file {
                        conflicts.push(ModConflict {
                            mod_a: mod_a.info.id.clone(),
                            mod_b: mod_b.info.id.clone(),
                            conflict_type: ConflictType::PatchOverlap,
                            detail: format!("Both patch '{}'", patch_a.file),
                        });
                    }
                }
            }
        }
    }

    conflicts
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch;

    fn sample_mod_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("mods/no_weapon_rarities")
    }

    fn values(pairs: &[(&str, OptionValue)]) -> HashMap<String, OptionValue> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bocw_mod_manager_{}_{}_{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sample_mod_matches_current_schemas() {
        let dir = sample_mod_dir();
        let manifest = load_mod(&dir).unwrap();
        assert_eq!(manifest.info.id, "no_weapon_rarities");
        assert_eq!(manifest.info.target_zones, ["zm_common"]);

        let defaults = PresetStore::new(&manifest.info.id).get_active_values(&manifest);
        assert_eq!(manifest.patches.len(), 2);
        for patch_ref in &manifest.patches {
            assert!(patch_ref.is_enabled(&defaults).unwrap());
            let set = patch::load_patch_set(&mod_file_path(&dir, &patch_ref.file).unwrap()).unwrap();
            assert!(manifest.info.target_zones.contains(&set.target_zone));
            assert!(!set.string_patches.is_empty());
            for sp in &set.string_patches {
                assert_eq!(sp.search.len(), sp.replace.len(), "{}", sp.search);
            }
        }
    }

    #[test]
    fn patch_conditions() {
        let on = values(&[("a", OptionValue::Bool(true)), ("b", OptionValue::Float(0.0))]);
        let patch = |condition: Option<&str>| ModPatchRef {
            file: "p.toml".into(),
            condition: condition.map(String::from),
        };
        assert!(patch(None).is_enabled(&on).unwrap());
        assert!(patch(Some(" ")).is_enabled(&on).unwrap());
        assert!(patch(Some("a")).is_enabled(&on).unwrap());
        assert!(!patch(Some("!a")).is_enabled(&on).unwrap());
        assert!(!patch(Some("b")).is_enabled(&on).unwrap());
        assert!(patch(Some("! b")).is_enabled(&on).unwrap());
        assert!(patch(Some("missing")).is_enabled(&on).is_err());
    }

    #[test]
    fn gsc_strings_are_escaped() {
        assert_eq!(gsc_string(r#"say "hi"\now"#), r#""say \"hi\"\\now""#);
        assert_eq!(gsc_string("a\r\nb"), r#""a\nb""#);
        assert_eq!(OptionValue::Str("x\"y".into()).to_gsc_literal(), r#""x\"y""#);
    }

    #[test]
    fn mod_menu_escapes_text_and_refresh_terminates() {
        let mut manifest = load_mod(&sample_mod_dir()).unwrap();
        manifest.info.name = "Quote \"Mod\"".into();
        manifest.options[0].label = "Label \"1\"".into();
        let defaults = PresetStore::new("x").get_active_values(&manifest);
        let gsc = generate_mod_menu_gsc(&[(manifest, defaults)]);

        assert!(gsc.contains(r#"level.mod_menu.mods[0].name = "Quote \"Mod\"";"#));
        assert!(gsc.contains(r#"options[0].label = "Label \"1\"";"#));
        assert!(gsc.contains("#using scripts\\core_common\\callbacks_shared;"));
        // The old refresh loop destroyed elements without shrinking the array.
        assert!(!gsc.contains("while (level.mod_menu.hud_elements.size > 3)"));
        assert!(gsc.contains("level.mod_menu.hud_elements = kept;"));
    }

    #[test]
    fn scripts_build_through_substitute_link_guard() {
        let dir = TempDir::new("scripts");
        std::fs::create_dir_all(dir.0.join("scripts")).unwrap();
        std::fs::write(
            dir.0.join("scripts/main.gsc"),
            "#namespace old;\n/*[[[SETTINGS]]] speed: slider [[[/SETTINGS]]]*/\nfunction f() { return [[[speed]]]; }\n",
        )
        .unwrap();
        std::fs::write(dir.0.join("scripts/other.gsc"), "function g() { return [[[label]]]; }\n").unwrap();

        let script = |file: &str, zone: &str| ModScript {
            file: file.into(),
            target_zone: zone.into(),
            inject_point: String::new(),
        };
        let mut manifest = load_mod(&sample_mod_dir()).unwrap();
        manifest.info.id = "speed_mod".into();
        manifest.scripts = vec![
            script("scripts/main.gsc", "zm_common"),
            script("scripts/other.gsc", "zm_common"),
        ];
        let vals = values(&[
            ("speed", OptionValue::Float(2.5)),
            ("label", OptionValue::Str("fast".into())),
        ]);

        let built = build_mod_scripts(&dir.0, &manifest, &vals).unwrap();
        assert_eq!(built.len(), 1);
        let source = &built[0].source;
        assert_eq!(built[0].target_zone, "zm_common");
        assert!(source.starts_with("#namespace speed_mod;"));
        assert!(!source.contains("#namespace old;"));
        assert!(!source.contains("SETTINGS"));
        assert!(source.contains("return 2.5;"));
        assert!(source.contains(r#"return "fast";"#));
        assert!(source.contains("function autoexec __bocw_mod_guard()"));

        let partial = values(&[("speed", OptionValue::Float(1.0))]);
        let err = build_mod_scripts(&dir.0, &manifest, &partial).unwrap_err();
        assert!(err.to_string().contains("label"), "{err}");

        manifest.scripts = vec![script("../escape.gsc", "zm_common")];
        assert!(build_mod_scripts(&dir.0, &manifest, &vals).is_err());
    }

    #[test]
    fn finds_unresolved_markers() {
        assert_eq!(unresolved_markers("a [[[x]]] b [[[y]]] [[[z"), ["x", "y"]);
        assert!(unresolved_markers("plain").is_empty());
    }
}
