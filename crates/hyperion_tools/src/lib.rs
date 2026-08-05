//! Offline codegen helpers for Hyperion (mc-ref / Mojang data → committed assets).
//!
//! Not linked into `hyperion-server`. Run the bins:
//! ```text
//! cargo run -p hyperion_tools --bin gen-mc-ref
//! ```

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

/// Datapack folder under `data/minecraft/` → synchronized registry id.
pub const SYNC_FOLDERS: &[(&str, &str)] = &[
    ("banner_pattern", "minecraft:banner_pattern"),
    ("cat_sound_variant", "minecraft:cat_sound_variant"),
    ("cat_variant", "minecraft:cat_variant"),
    ("chat_type", "minecraft:chat_type"),
    ("chicken_sound_variant", "minecraft:chicken_sound_variant"),
    ("chicken_variant", "minecraft:chicken_variant"),
    ("cow_sound_variant", "minecraft:cow_sound_variant"),
    ("cow_variant", "minecraft:cow_variant"),
    ("damage_type", "minecraft:damage_type"),
    ("dialog", "minecraft:dialog"),
    ("dimension_type", "minecraft:dimension_type"),
    ("enchantment", "minecraft:enchantment"),
    ("frog_variant", "minecraft:frog_variant"),
    ("instrument", "minecraft:instrument"),
    ("jukebox_song", "minecraft:jukebox_song"),
    ("painting_variant", "minecraft:painting_variant"),
    ("pig_sound_variant", "minecraft:pig_sound_variant"),
    ("pig_variant", "minecraft:pig_variant"),
    ("sulfur_cube_archetype", "minecraft:sulfur_cube_archetype"),
    ("test_environment", "minecraft:test_environment"),
    ("test_instance", "minecraft:test_instance"),
    ("timeline", "minecraft:timeline"),
    ("trim_material", "minecraft:trim_material"),
    ("trim_pattern", "minecraft:trim_pattern"),
    ("wolf_sound_variant", "minecraft:wolf_sound_variant"),
    ("wolf_variant", "minecraft:wolf_variant"),
    ("world_clock", "minecraft:world_clock"),
    (
        "zombie_nautilus_variant",
        "minecraft:zombie_nautilus_variant",
    ),
    ("worldgen/biome", "minecraft:worldgen/biome"),
];

/// Resolve the Hyperion workspace root (directory with `crates/hyperion_world`).
pub fn workspace_root() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(root) = manifest.parent().and_then(|p| p.parent())
        && root.join("Cargo.toml").is_file()
        && root.join("crates/hyperion_world").is_dir()
    {
        return Ok(root.to_path_buf());
    }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("crates/hyperion_world").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("could not find workspace root".to_owned());
        }
    }
}

/// Path to `tools/mc-ref/server-inner-26.2.jar`.
pub fn server_inner_jar(root: &Path) -> PathBuf {
    root.join("tools/mc-ref/server-inner-26.2.jar")
}

/// Path to Mojang data generator reports directory.
pub fn reports_dir(root: &Path) -> PathBuf {
    root.join("tools/mc-ref/datagen/generated/reports")
}

/// Open the vanilla inner server jar as a ZIP archive.
pub fn open_server_jar(root: &Path) -> Result<ZipArchive<std::fs::File>, String> {
    let path = server_inner_jar(root);
    if !path.is_file() {
        return Err(format!(
            "missing {}\nExtract META-INF/versions/26.2/server-26.2.jar from the bundler (see tools/mc-ref/README.md).",
            path.display()
        ));
    }
    let file = std::fs::File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
    ZipArchive::new(file).map_err(|e| format!("zip {}: {e}", path.display()))
}

/// Lists bare entry paths (no nested folders) under `data/minecraft/{folder}/`.
pub fn list_entries<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    folder: &str,
) -> Result<Vec<String>, String> {
    let prefix = format!("data/minecraft/{folder}/");
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let name = file.name().to_owned();
        if name.starts_with(&prefix) && name.ends_with(".json") {
            let rest = &name[prefix.len()..name.len() - 5];
            if !rest.contains('/') {
                out.push(rest.to_owned());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Moves `first` to the front of the list when present (network id 0).
pub fn prioritize(entries: &mut Vec<String>, first: &str) {
    if let Some(pos) = entries.iter().position(|e| e == first) {
        let item = entries.remove(pos);
        entries.insert(0, item);
    }
}

/// Apply biome / dimension prioritization used by join data.
pub fn prioritize_registry(reg_id: &str, entries: &mut Vec<String>) {
    if reg_id == "minecraft:worldgen/biome" {
        prioritize(entries, "plains");
    }
    if reg_id == "minecraft:dimension_type" {
        prioritize(entries, "overworld");
    }
}

/// Escape a string as a Rust string literal.
pub fn rust_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Ensures parent directories exist, then writes UTF-8 text with `\n` newlines.
pub fn write_text(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Ensures parent directories exist, then writes raw bytes.
pub fn write_bytes(path: &Path, content: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))
}
