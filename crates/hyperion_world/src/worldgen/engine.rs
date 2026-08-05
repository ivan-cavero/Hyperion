//! Runtime worldgen backend selection.
//!
//! **Play default = scaffold** (fast hills). Density is opt-in and, when used
//! for Play, always runs at `GenDetail::Terrain` (no veins/carvers/trees) so
//! join stays usable. Full decoration is for tools/tests only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::chunk::ChunkColumn;
use crate::worldgen::chunk_fill::{GenDetail, generate_column_from_density_with_detail};
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
    ///
    /// Default (unset) is always **scaffold** — density is not ready for a
    /// smooth vanilla join yet.
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
    /// Cache generated columns so join does not re-run the same chunk.
    cache: Mutex<HashMap<(i32, i32), ChunkColumn>>,
}

struct DensityBackend {
    seed: i64,
    settings: NoiseSettings,
    lib: Mutex<DensityLibrary>,
    /// Always Terrain for Play (see `load_density`).
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
        if mode == WorldgenMode::Scaffold {
            eprintln!(
                "hyperion worldgen: scaffold (fast Play hills). Set HYPERION_WORLDGEN=density for experimental density terrain."
            );
        }
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

    /// Feet Y for player spawn at world origin — solid ground + headroom.
    ///
    /// Never returns 0. Prefer land near origin if (0,0) is ocean.
    pub fn spawn_feet_y(&self) -> i32 {
        match &self.density {
            None => {
                let y = scaffold_spawn_feet(self.seed);
                y.clamp(16, 300)
            }
            Some(d) => {
                let col = self.generate_column(0, 0);
                let mut best = find_spawn_feet_y(&col, 0, 0);
                let ground = col.get_block(0, best.saturating_sub(1), 0);
                // Search nearby land if origin is water or buried oddly.
                if ground.is_fluid() || best <= d.settings.sea_level {
                    for radius in [8i32, 16, 24, 32] {
                        for (dx, dz) in [
                            (radius, 0),
                            (-radius, 0),
                            (0, radius),
                            (0, -radius),
                            (radius, radius),
                            (-radius, -radius),
                        ] {
                            let cx = dx.div_euclid(16);
                            let cz = dz.div_euclid(16);
                            let probe = self.generate_column(cx, cz);
                            let y = find_spawn_feet_y(&probe, dx, dz);
                            let g = probe.get_block(dx, y.saturating_sub(1), dz);
                            if !g.is_fluid() && !g.is_air() && y > d.settings.sea_level {
                                return y.clamp(16, 300);
                            }
                            best = best.max(y);
                        }
                    }
                }
                // Absolute floor: never spawn at y=0 (void / bedrock feel).
                best.max(d.settings.sea_level.max(64)).clamp(16, 300)
            }
        }
    }

    /// Generate one column (does not touch disk). Cached.
    pub fn generate_column(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
        if let Ok(cache) = self.cache.lock()
            && let Some(col) = cache.get(&(chunk_x, chunk_z))
        {
            return col.clone();
        }

        let col = if let Some(d) = &self.density {
            match d.column(chunk_x, chunk_z) {
                Ok(c) => c,
                Err(err) => {
                    eprintln!(
                        "density fill failed chunk {chunk_x},{chunk_z}: {err}; scaffold fallback"
                    );
                    scaffold_generate(self.seed, chunk_x, chunk_z)
                }
            }
        } else {
            scaffold_generate(self.seed, chunk_x, chunk_z)
        };

        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() > 512 {
                cache.clear();
            }
            cache.insert((chunk_x, chunk_z), col.clone());
        }
        col
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
    // Play always uses Terrain detail — Full is multi-second per chunk and
    // unusable for view-distance 8 (289 columns on join).
    let detail = GenDetail::Terrain;
    if matches!(
        std::env::var("HYPERION_WORLDGEN_DETAIL")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "full" | "all"
    ) {
        eprintln!(
            "hyperion: ignoring HYPERION_WORLDGEN_DETAIL=full for Play (too slow). Using terrain-only density."
        );
    }
    eprintln!(
        "hyperion density worldgen ready (detail={detail:?}, play-safe); jar={}",
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
        assert!((16..320).contains(&feet), "feet={feet}");
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
        assert!(feet >= 16, "feet={feet} must not be near void");
        let head = col.get_block(0, feet, 0);
        assert!(
            head.is_air() || head.is_fluid(),
            "spawn feet={feet} head block={}",
            head.name
        );
        // Cache hit
        let col2 = generator.generate_column(0, 0);
        assert_eq!(col.surface_y, col2.surface_y);
    }
}
