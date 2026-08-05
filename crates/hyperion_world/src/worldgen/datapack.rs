//! Load density functions + noise params + noise_settings from the official jar.
//!
//! Path: `tools/mc-ref/server-inner-26.2.jar` (same as join-data tools).
//! Optional at runtime — tests skip when the jar is missing.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zip::ZipArchive;

use crate::worldgen::density::DensityLibrary;
use crate::worldgen::noise_settings::NoiseSettings;

/// Default relative path from workspace root.
pub fn default_server_inner_jar(workspace_root: &Path) -> PathBuf {
    workspace_root.join("tools/mc-ref/server-inner-26.2.jar")
}

/// Try to find the workspace root by walking parents.
pub fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("crates/hyperion_world").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Loads overworld noise settings + all density_function + noise JSONs.
pub fn load_overworld_from_jar(
    jar_path: &Path,
    seed: i64,
) -> Result<(NoiseSettings, DensityLibrary), String> {
    if !jar_path.is_file() {
        return Err(format!("jar not found: {}", jar_path.display()));
    }
    let file = File::open(jar_path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut lib = DensityLibrary::new(seed);

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_owned();
        if !name.ends_with(".json") {
            continue;
        }
        if let Some(rest) = name
            .strip_prefix("data/minecraft/worldgen/density_function/")
            .and_then(|s| s.strip_suffix(".json"))
        {
            let mut raw = String::new();
            entry.read_to_string(&mut raw).map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            lib.insert_density_json(format!("minecraft:{rest}"), json);
        } else if let Some(rest) = name
            .strip_prefix("data/minecraft/worldgen/noise/")
            .and_then(|s| s.strip_suffix(".json"))
        {
            let mut raw = String::new();
            entry.read_to_string(&mut raw).map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            lib.insert_noise_json(&format!("minecraft:{rest}"), &json)?;
        }
    }

    // Re-open for noise_settings (zip iterator already consumed names — re-open).
    let file = File::open(jar_path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entry = zip
        .by_name("data/minecraft/worldgen/noise_settings/overworld.json")
        .map_err(|e| format!("overworld noise_settings: {e}"))?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let settings = NoiseSettings::from_json(&json)?;

    Ok((settings, lib))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_overworld_if_jar_present() {
        let Some(root) = find_workspace_root() else {
            return;
        };
        let jar = default_server_inner_jar(&root);
        if !jar.is_file() {
            eprintln!("skip: no {}", jar.display());
            return;
        }
        let (settings, lib) = load_overworld_from_jar(&jar, 1).expect("load");
        assert_eq!(settings.noise.min_y, -64);
        assert!(
            lib.density_count() > 20,
            "density defs {}",
            lib.density_count()
        );
        assert!(
            lib.noise_param_count() > 20,
            "noise defs {}",
            lib.noise_param_count()
        );
        // Try resolving final_density — may fail until spline/old_blended land.
        let mut lib = lib;
        let fd = settings
            .final_density(&mut lib)
            .expect("overworld final_density should resolve");
        let deep = fd
            .compute(
                crate::worldgen::density::DensityContext::new(0, -32, 0),
                &mut lib.noises,
            )
            .expect("sample deep");
        let high = fd
            .compute(
                crate::worldgen::density::DensityContext::new(0, 200, 0),
                &mut lib.noises,
            )
            .expect("sample high");
        eprintln!("final_density deep={deep} high={high}");
        // In a real overworld, deep should tend solid (>0) more often than sky.
        // We only assert the graph is evaluable and finite for now (parity later).
        assert!(deep.is_finite() && high.is_finite());
    }

    #[test]
    fn overworld_density_fill_smoke_if_jar_present() {
        use crate::worldgen::chunk_fill::generate_column_from_density;

        let Some(root) = find_workspace_root() else {
            return;
        };
        let jar = default_server_inner_jar(&root);
        if !jar.is_file() {
            return;
        }
        let (settings, mut lib) = load_overworld_from_jar(&jar, 1).expect("load");
        assert!(settings.has_climate_router(), "overworld should expose climate axes");
        // Small fill of chunk 0,0 — may not match official yet (noise tables WIP)
        // but must complete without error.
        let col = generate_column_from_density(1, 0, 0, &settings, &mut lib).expect("fill");
        assert_eq!(col.sections.len(), 24);
        assert!(
            col.sections[0].biome.starts_with("minecraft:"),
            "biome={}",
            col.sections[0].biome
        );
        eprintln!("overworld chunk biome={}", col.sections[0].biome);
        // Network encode must succeed.
        let payload = col.encode_network_payload(true).expect("network");
        assert!(payload.len() > 50_000);
    }
}
