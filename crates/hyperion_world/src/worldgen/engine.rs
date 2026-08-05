//! Runtime worldgen backend selection.
//!
//! - **scaffold** (default): temporary Play hills — always available / fast.
//! - **density**: `final_density` from official jar (set `HYPERION_WORLDGEN=density`).
//!
//! Density detail (decoration cost):
//! - default / `HYPERION_WORLDGEN_DETAIL=terrain` — terrain + surface (fast enough for join)
//! - `HYPERION_WORLDGEN_DETAIL=full` — veins, carvers, trees, structures

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::chunk::ChunkColumn;
use crate::worldgen::chunk_fill::{
    GenDetail, generate_column_from_density_with_detail,
};
use crate::worldgen::datapack::{
    default_server_inner_jar, find_workspace_root, load_overworld_from_jar,
};
use crate::worldgen::density::DensityLibrary;
use crate::worldgen::noise_settings::NoiseSettings;
use crate::worldgen::scaffold::{
    generate_column as scaffold_generate, spawn_feet_y as scaffold_spawn_feet,
};
use crate::worldgen::surface_rules::find_spawn_feet_y;

/// Which generator produces new columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldgenMode {
    Scaffold,
    Density,
}

impl WorldgenMode {
    /// Resolve from `HYPERION_WORLDGEN` (scaffold | density | auto).
    pub fn from_env() -> Self {
        match std::env::var("HYPERION_WORLDGEN")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "density" => Self::Density,
            "auto" => {
                if resolve_server_jar().is_some() {
                    Self::Density
                } else {
                    Self::Scaffold
                }
            }
            _ => Self::Scaffold,
        }
    }
}

/// Locate `server-inner-*.jar` for datapack load.
pub fn resolve_server_jar() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HYPERION_SERVER_JAR") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let root = find_workspace_root()?;
    let jar = default_server_inner_jar(&root);
    if jar.is_file() {
        Some(jar)
    } else {
        None
    }
}

/// Shared generator used by Play / Anvil ensure paths.
pub struct ColumnGenerator {
    mode: WorldgenMode,
    seed: u64,
    density: Option<DensityBackend>,
    /// Cache generated density columns (join streams many chunks).
    cache: Mutex<HashMap<(i32, i32), ChunkColumn>>,
}

struct DensityBackend {
    seed: i64,
    settings: NoiseSettings,
    lib: Mutex<DensityLibrary>,
    detail: GenDetail,
}

impl ColumnGenerator {
    /// Build generator for this world seed (respects `HYPERION_WORLDGEN`).
    pub fn new(seed: u64) -> Self {
        Self::with_mode(seed, WorldgenMode::from_env())
    }

    pub fn with_mode(seed: u64, mode: WorldgenMode) -> Self {
        let density = match mode {
            WorldgenMode::Scaffold => None,
            WorldgenMode::Density => match load_density(seed as i64) {
                Ok(backend) => Some(backend),
                Err(err) => {
                    eprintln!("hyperion density worldgen unavailable ({err}); using scaffold");
                    None
                }
            },
        };
        let mode = if density.is_some() {
            WorldgenMode::Density
        } else {
            WorldgenMode::Scaffold
        };
        Self {
            mode,
            seed,
            density,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn mode(&self) -> WorldgenMode {
        self.mode
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Feet Y at world origin for player spawn (safe solid + headroom).
    pub fn spawn_feet_y(&self) -> i32 {
        let Some(d) = &self.density else {
            return scaffold_spawn_feet(self.seed);
        };
        let col = self.generate_column(0, 0);
        // Prefer origin; if ocean, search nearby land for a solid stand.
        let mut best = find_spawn_feet_y(&col, 0, 0);
        let b0 = col.get_block(0, best - 1, 0);
        if b0.is_fluid() || best < d.settings.sea_level {
            for radius in (8..=48).step_by(8) {
                for (dx, dz) in [
                    (radius, 0i32),
                    (-radius, 0),
                    (0, radius),
                    (0, -radius),
                    (radius, radius),
                    (-radius, radius),
                ] {
                    let cx = dx.div_euclid(16);
                    let cz = dz.div_euclid(16);
                    let probe = if cx == 0 && cz == 0 {
                        col.clone()
                    } else {
                        self.generate_column(cx, cz)
                    };
                    let y = find_spawn_feet_y(&probe, dx, dz);
                    let ground = probe.get_block(dx, y - 1, dz);
                    if !ground.is_fluid() && !ground.is_air() && y >= d.settings.sea_level {
                        return y;
                    }
                    best = best.max(y);
                }
            }
        }
        // Never bury the player under the sea floor height.
        best.max(d.settings.sea_level + 1).min(320)
    }

    /// Generate one column (does not touch disk). Cached for density mode.
    pub fn generate_column(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
        if let Some(d) = &self.density {
            if let Ok(cache) = self.cache.lock()
                && let Some(col) = cache.get(&(chunk_x, chunk_z))
            {
                return col.clone();
            }
            match d.column(chunk_x, chunk_z) {
                Ok(col) => {
                    if let Ok(mut cache) = self.cache.lock() {
                        // Cap cache so RAM stays bounded during long sessions.
                        if cache.len() > 512 {
                            cache.clear();
                        }
                        cache.insert((chunk_x, chunk_z), col.clone());
                    }
                    return col;
                }
                Err(err) => {
                    eprintln!(
                        "density fill failed chunk {chunk_x},{chunk_z}: {err}; scaffold fallback"
                    );
                }
            }
        }
        scaffold_generate(self.seed, chunk_x, chunk_z)
    }

    /// Ensure column exists on disk (generate if missing).
    pub fn ensure_on_disk(
        &self,
        world_dir: impl AsRef<std::path::Path>,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Result<bool, crate::error::WorldError> {
        let world_dir = world_dir.as_ref();
        let (rx, rz) = crate::anvil::chunk_to_region(chunk_x, chunk_z);
        let path = crate::anvil::region_path(world_dir, rx, rz);
        let region = crate::anvil::RegionFile::open_or_create(&path)?;
        if region.has_chunk(chunk_x, chunk_z)? {
            return Ok(false);
        }
        let column = self.generate_column(chunk_x, chunk_z);
        let nbt = column.encode_storage_nbt()?;
        region.write_chunk(chunk_x, chunk_z, &nbt)?;
        Ok(true)
    }
}

impl DensityBackend {
    fn column(&self, chunk_x: i32, chunk_z: i32) -> Result<ChunkColumn, String> {
        let mut lib = self
            .lib
            .lock()
            .map_err(|_| "density library lock poisoned".to_owned())?;
        generate_column_from_density_with_detail(
            self.seed,
            chunk_x,
            chunk_z,
            &self.settings,
            &mut lib,
            self.detail,
        )
    }
}

fn load_density(seed: i64) -> Result<DensityBackend, String> {
    let jar = resolve_server_jar().ok_or_else(|| "server jar not found".to_owned())?;
    let (settings, lib) = load_overworld_from_jar(&jar, seed)?;
    let detail = GenDetail::from_env();
    eprintln!(
        "hyperion density worldgen ready (detail={detail:?}); jar={}",
        jar.display()
    );
    Ok(DensityBackend {
        seed,
        settings,
        lib: Mutex::new(lib),
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_mode_always_works() {
        let generator = ColumnGenerator::with_mode(99, WorldgenMode::Scaffold);
        assert_eq!(generator.mode(), WorldgenMode::Scaffold);
        let col = generator.generate_column(0, 0);
        assert_eq!(col.sections.len(), 24);
        assert!(col.surface_y > 0);
        let feet = generator.spawn_feet_y();
        assert!(feet > 0 && feet < 320, "feet={feet}");
    }

    #[test]
    fn density_mode_if_jar_present() {
        if resolve_server_jar().is_none() {
            return;
        }
        let generator = ColumnGenerator::with_mode(1, WorldgenMode::Density);
        assert_eq!(generator.mode(), WorldgenMode::Density);
        let col = generator.generate_column(0, 0);
        assert_eq!(col.sections.len(), 24);
        assert_eq!(
            col.get_block(0, -64, 0),
            crate::chunk::BlockState::bedrock()
        );
        let feet = generator.spawn_feet_y();
        // Must not be buried in solid.
        let ground = col.get_block(0, feet - 1, 0);
        let head = col.get_block(0, feet, 0);
        assert!(
            !head.name.contains("stone") && !head.name.contains("dirt"),
            "spawn feet={feet} inside solid head={}",
            head.name
        );
        let _ = ground;
        // Cache hit
        let col2 = generator.generate_column(0, 0);
        assert_eq!(col.surface_y, col2.surface_y);
    }
}
