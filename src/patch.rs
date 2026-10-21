use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryPatch {
    pub offset: usize,
    pub original: Vec<u8>,
    pub replacement: Vec<u8>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringPatch {
    pub search: String,
    pub replace: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub max_occurrences: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchSet {
    pub name: String,
    pub target_zone: String,
    #[serde(default)]
    pub binary_patches: Vec<BinaryPatch>,
    #[serde(default)]
    pub string_patches: Vec<StringPatch>,
}

pub fn apply_binary_patch(data: &mut Vec<u8>, patch: &BinaryPatch) -> Result<bool> {
    if patch.offset + patch.original.len() > data.len() {
        bail!(
            "Patch offset {:#x} + size {} exceeds data length {}",
            patch.offset,
            patch.original.len(),
            data.len()
        );
    }

    let slice = &data[patch.offset..patch.offset + patch.original.len()];
    if slice != patch.original.as_slice() {
        return Ok(false);
    }

    if patch.replacement.len() != patch.original.len() {
        bail!(
            "Patch '{}': replacement size {} differs from original size {} \
             (in-place patches must be same size)",
            patch.description,
            patch.replacement.len(),
            patch.original.len()
        );
    }

    data[patch.offset..patch.offset + patch.replacement.len()]
        .copy_from_slice(&patch.replacement);

    Ok(true)
}

pub fn apply_string_patch(data: &mut Vec<u8>, patch: &StringPatch) -> Result<usize> {
    let search_bytes = patch.search.as_bytes();
    let replace_bytes = patch.replace.as_bytes();

    if replace_bytes.len() > search_bytes.len() {
        bail!(
            "String patch '{}': replacement '{}' ({} bytes) is longer than search '{}' ({} bytes). \
             Replacements must be same length or shorter (padded with nulls).",
            patch.description,
            patch.replace,
            replace_bytes.len(),
            patch.search,
            search_bytes.len()
        );
    }

    let mut padded_replace = replace_bytes.to_vec();
    padded_replace.resize(search_bytes.len(), 0);

    let max = patch.max_occurrences.unwrap_or(usize::MAX);
    let mut count = 0;
    let mut offset = 0;

    while offset + search_bytes.len() <= data.len() && count < max {
        if &data[offset..offset + search_bytes.len()] == search_bytes {
            data[offset..offset + padded_replace.len()].copy_from_slice(&padded_replace);
            count += 1;
            offset += search_bytes.len();
        } else {
            offset += 1;
        }
    }

    Ok(count)
}

pub fn find_pattern(data: &[u8], pattern: &[u8]) -> Vec<usize> {
    let mut results = Vec::new();
    if pattern.is_empty() || data.len() < pattern.len() {
        return results;
    }

    for i in 0..=data.len() - pattern.len() {
        if &data[i..i + pattern.len()] == pattern {
            results.push(i);
        }
    }
    results
}

pub fn find_string(data: &[u8], needle: &str) -> Vec<usize> {
    find_pattern(data, needle.as_bytes())
}

pub fn load_patch_set(path: &Path) -> Result<PatchSet> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let ps: PatchSet = if path.extension().map_or(false, |e| e == "json") {
        serde_json::from_str(&content)?
    } else {
        toml::from_str(&content)?
    };
    Ok(ps)
}

pub fn save_patch_set(path: &Path, patch_set: &PatchSet) -> Result<()> {
    let content = if path.extension().map_or(false, |e| e == "json") {
        serde_json::to_string_pretty(patch_set)?
    } else {
        toml::to_string_pretty(patch_set)?
    };
    std::fs::write(path, content)?;
    Ok(())
}
