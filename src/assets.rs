use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum AssetType {
    RawFile = 0x01,
    ScriptParseTree = 0x02,
    ScriptParseTreeDbg = 0x03,
    StringTable = 0x04,
    LocalizeEntry = 0x05,
    KeyValuePairs = 0x06,
    ScriptBundle = 0x07,
    BgCache = 0x08,
    Ddl = 0x09,
    WeaponFrontend = 0x0A,
    ZBarrier = 0x0B,
    Lua = 0x0C,
    Material = 0x10,
    Texture = 0x11,
    Model = 0x12,
    Sound = 0x20,
    FxDef = 0x30,
    MapEnts = 0x40,
    Unknown = 0xFF,
}

impl AssetType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x01 => Self::RawFile,
            0x02 => Self::ScriptParseTree,
            0x03 => Self::ScriptParseTreeDbg,
            0x04 => Self::StringTable,
            0x05 => Self::LocalizeEntry,
            0x06 => Self::KeyValuePairs,
            0x07 => Self::ScriptBundle,
            0x08 => Self::BgCache,
            0x09 => Self::Ddl,
            0x0A => Self::WeaponFrontend,
            0x0B => Self::ZBarrier,
            0x0C => Self::Lua,
            0x10 => Self::Material,
            0x11 => Self::Texture,
            0x12 => Self::Model,
            0x20 => Self::Sound,
            0x30 => Self::FxDef,
            0x40 => Self::MapEnts,
            _ => Self::Unknown,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::RawFile => "rawfile",
            Self::ScriptParseTree => "scriptparsetree",
            Self::ScriptParseTreeDbg => "scriptparsetreedbg",
            Self::StringTable => "stringtable",
            Self::LocalizeEntry => "localizeentry",
            Self::KeyValuePairs => "keyvaluepairs",
            Self::ScriptBundle => "scriptbundle",
            Self::BgCache => "bgcache",
            Self::Ddl => "ddl",
            Self::WeaponFrontend => "weapon_frontend",
            Self::ZBarrier => "zbarrier",
            Self::Lua => "lua",
            Self::Material => "material",
            Self::Texture => "texture",
            Self::Model => "model",
            Self::Sound => "sound",
            Self::FxDef => "fxdef",
            Self::MapEnts => "mapents",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_moddable(&self) -> bool {
        matches!(
            self,
            Self::RawFile
                | Self::ScriptParseTree
                | Self::StringTable
                | Self::LocalizeEntry
                | Self::KeyValuePairs
                | Self::ScriptBundle
                | Self::BgCache
                | Self::WeaponFrontend
                | Self::ZBarrier
                | Self::Lua
        )
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            Self::RawFile => "raw",
            Self::ScriptParseTree | Self::ScriptParseTreeDbg => "gsc",
            Self::StringTable => "csv",
            Self::LocalizeEntry => "str",
            Self::KeyValuePairs => "kvp",
            Self::ScriptBundle => "json",
            Self::Lua => "lua",
            Self::Material => "mtl",
            Self::Texture => "dds",
            Self::Model => "xmodel",
            Self::Sound => "flac",
            _ => "bin",
        }
    }
}

impl fmt::Display for AssetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub asset_type: AssetType,
    pub name_hash: u64,
    pub name: Option<String>,
    pub offset: usize,
    pub size: usize,
    pub data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPool {
    pub zone_name: String,
    pub assets: Vec<AssetEntry>,
    pub asset_counts: HashMap<String, usize>,
}

impl AssetPool {
    pub fn new(zone_name: &str) -> Self {
        Self {
            zone_name: zone_name.to_string(),
            assets: Vec::new(),
            asset_counts: HashMap::new(),
        }
    }

    pub fn add(&mut self, entry: AssetEntry) {
        let type_name = entry.asset_type.name().to_string();
        *self.asset_counts.entry(type_name).or_insert(0) += 1;
        self.assets.push(entry);
    }

    pub fn find_by_type(&self, asset_type: AssetType) -> Vec<&AssetEntry> {
        self.assets
            .iter()
            .filter(|a| a.asset_type == asset_type)
            .collect()
    }

    pub fn find_by_name(&self, name: &str) -> Option<&AssetEntry> {
        self.assets
            .iter()
            .find(|a| a.name.as_deref() == Some(name))
    }

    pub fn find_by_hash(&self, hash: u64) -> Option<&AssetEntry> {
        self.assets.iter().find(|a| a.name_hash == hash)
    }
}

pub fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325;
    for &byte in data {
        let b = if byte >= b'A' && byte <= b'Z' {
            byte + 32
        } else if byte == b'\\' {
            b'/'
        } else {
            byte
        };
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001B3);
    }
    hash
}

pub fn hash_asset_name(name: &str) -> u64 {
    fnv1a_hash(name.as_bytes())
}

pub fn known_asset_names() -> HashMap<u64, String> {
    let mut map = HashMap::new();

    let names = [
        "scripts/zm_common/zm_weapons.gsc",
        "scripts/zm_common/zm_utility.gsc",
        "scripts/zm_common/zm_perks.gsc",
        "scripts/zm_common/zm_powerups.gsc",
        "scripts/zm_common/zm_spawner.gsc",
        "scripts/zm_common/zm_game.gsc",
        "scripts/zm_common/zm_loadout.gsc",
        "scripts/zm_common/zm_rarity.gsc",
        "scripts/zm_common/zm_crafting.gsc",
        "scripts/zm_common/zm_pack_a_punch.gsc",
        "scripts/zm_common/zm_score.gsc",
        "scripts/zm_common/zm_armor.gsc",
        "scripts/zm_common/zm_field_upgrades.gsc",
        "scripts/zm_common/zm_hud.gsc",
        "scripts/core_common/clientfield_shared.gsc",
        "scripts/core_common/util_shared.gsc",
        "scripts/core_common/player_shared.gsc",
        "scripts/core_common/hud_shared.gsc",
        "scripts/core_common/callbacks_shared.gsc",
        "scripts/mp_common/mp_weapons.gsc",
        "gamedata/weapons/common/weapon_table.csv",
        "gamedata/weapons/zm/zm_weapon_table.csv",
        "gamedata/loot/zm_loot_table.csv",
        "gamedata/perks/zm_perk_table.csv",
        "gamedata/rarity/rarity_table.csv",
    ];

    for name in &names {
        map.insert(hash_asset_name(name), name.to_string());
    }

    map
}
