//! Runtime worldgen backend selection.
//!
//! - **scaffold** (default): temporary Play hills — always available.
//! - **density**: `final_density` fill from official jar noise_settings when present.
//!
//! Selection:
//! - env `HYPERION_WORLDGEN=scaffold|density|auto`
//! - `auto` = density if the server jar is found, else scaffold
//! - default when unset: `scaffold` until density is fast enough for default Play

use std::path::PathBuf;
use std::sync::Mutex;

use crate::chunk::ChunkColumn;
use crate::worldgen::chunk_fill::generate_column_from_density;
use crate::worldgen::datapack::{
    default_server_inner_jar, find_workspace_root, load_overworld_from_jar,
};
use crate::worldgen::density::DensityLibrary;
use crate::worldgen::noise_settings::NoiseSettings;
use crate::worldgen::scaffold::{
    generate_column as scaffold_generate, spawn_feet_y as scaffold_spawn_feet,
};

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
}

struct DensityBackend {
    seed: i64,
    settings: NoiseSettings,
    lib: Mutex<DensityLibrary>,
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
        }
    }

    pub fn mode(&self) -> WorldgenMode {
        self.mode
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Feet Y at world origin for player spawn.
    pub fn spawn_feet_y(&self) -> i32 {
        match &self.density {
            Some(d) => match d.column(0, 0) {
                Ok(col) => col.surface_y.max(64),
                Err(_) => scaffold_spawn_feet(self.seed),
            },
            None => scaffold_spawn_feet(self.seed),
        }
    }

    /// Generate one column (does not touch disk).
    pub fn generate_column(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
        if let Some(d) = &self.density {
            match d.column(chunk_x, chunk_z) {
                Ok(col) => return col,
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
        generate_column_from_density(self.seed, chunk_x, chunk_z, &self.settings, &mut lib)
    }
}

fn load_density(seed: i64) -> Result<DensityBackend, String> {
    let jar = resolve_server_jar().ok_or_else(|| "server jar not found".to_owned())?;
    let (settings, lib) = load_overworld_from_jar(&jar, seed)?;
    Ok(DensityBackend {
        seed,
        settings,
        lib: Mutex::new(lib),
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
        // Bedrock floor from surface pass
        assert_eq!(
            col.get_block(0, -64, 0),
            crate::chunk::BlockState::bedrock()
        );
    }
}
