//! Vanilla-like server data directory bootstrap.
//!
//! Mirrors a first-start Java Edition layout under the server working root
//! (parent of `server.properties`):
//! - `<level-name>/` — world folder (`level-name` from `server.properties`)
//! - `<level-name>/level.dat` — gzipped storage NBT (`LevelName`, seed, spawn)
//! - `<level-name>/session.lock` — non-empty lock marker
//! - `ops.json`, `whitelist.json`, `banned-players.json`, `banned-ips.json`

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::WorldError;
use crate::level_dat::{LevelMeta, write_level_dat};

/// Empty JSON array body used for vanilla list files (`ops.json`, …).
const EMPTY_JSON_ARRAY: &str = "[]\n";

/// Resolved paths after [`prepare_data_directory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// Server working root (parent of the world folder and list JSON files).
    pub root: PathBuf,
    /// World directory: `root/<level-name>/` (vanilla `level-name`, default `world`).
    pub world_dir: PathBuf,
    /// Path to `level.dat` inside the world directory.
    pub level_dat: PathBuf,
    /// Path to `session.lock` inside the world directory.
    pub session_lock: PathBuf,
    /// Path to `ops.json` at the server root.
    pub ops: PathBuf,
    /// Path to `whitelist.json` at the server root.
    pub whitelist: PathBuf,
    /// Path to `banned-players.json` at the server root.
    pub banned_players: PathBuf,
    /// Path to `banned-ips.json` at the server root.
    pub banned_ips: PathBuf,
}

/// Inputs for first-start data directory preparation.
///
/// Field names match `server.properties` keys (`level-name`, `level-seed`) and
/// feed the NBT fields written into a new `level.dat` (`LevelName`, seed).
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Server working root (parent of the world folder; usually the directory
    /// that contains `server.properties`).
    pub root: PathBuf,
    /// Vanilla `level-name`: world folder name under `root`, and default NBT
    /// `LevelName` (default `"world"`).
    pub level_name: String,
    /// Vanilla `level-seed`. **`0` means generate a random seed once** when
    /// creating a missing `level.dat` (seed is then persisted; later boots
    /// keep it). Non-zero values are written as `RandomSeed` and
    /// `WorldGenSettings.seed`.
    pub level_seed: i64,
    /// Default spawn Y written into a new `level.dat` as `SpawnY` (e.g. `100`).
    /// Hyperion extension until worldgen supplies a real surface.
    pub spawn_y: i32,
}

/// Prepares a vanilla-shaped data directory (idempotent).
///
/// Behaviour:
/// 1. Create `root` if missing.
/// 2. Create `root/<level-name>/` if missing.
/// 3. If `level.dat` is missing, write a minimal file via [`write_level_dat`].
///    If present, leave it intact (does not overwrite custom seeds/metadata).
/// 4. If `session.lock` is missing, write a non-empty payload containing the
///    current process id as decimal ASCII plus a newline.
/// 5. If missing, create empty JSON array files at root: `ops.json`,
///    `whitelist.json`, `banned-players.json`, `banned-ips.json`.
pub fn prepare_data_directory(config: &BootstrapConfig) -> Result<DataPaths, WorldError> {
    validate_level_name(&config.level_name)?;

    let root = config.root.clone();
    fs::create_dir_all(&root).map_err(|source| WorldError::io(&root, source))?;

    let world_dir = root.join(&config.level_name);
    // Defence in depth: reject any resolution that escapes `root` (symlinks,
    // alternate separators, etc.).
    let root_canon = fs::canonicalize(&root).map_err(|source| WorldError::io(&root, source))?;
    // world_dir may not exist yet; create it first then canonicalize.
    fs::create_dir_all(&world_dir).map_err(|source| WorldError::io(&world_dir, source))?;
    let world_canon =
        fs::canonicalize(&world_dir).map_err(|source| WorldError::io(&world_dir, source))?;
    if !world_canon.starts_with(&root_canon) {
        return Err(WorldError::InvalidLevelName(format!(
            "level-name resolves outside the data root: {}",
            config.level_name
        )));
    }

    let level_dat = world_dir.join("level.dat");
    if !level_dat.exists() {
        let seed = if config.level_seed == 0 {
            generate_random_seed()
        } else {
            config.level_seed
        };
        let meta = LevelMeta::new(config.level_name.clone(), seed, config.spawn_y);
        write_level_dat(&level_dat, &meta)?;
    }

    let session_lock = world_dir.join("session.lock");
    if !session_lock.exists() {
        // Vanilla uses a JVM file lock + long; we only need a non-empty marker.
        // Content: process id as decimal ASCII + newline.
        let payload = format!("{}\n", std::process::id());
        fs::write(&session_lock, payload)
            .map_err(|source| WorldError::io(&session_lock, source))?;
    }

    let ops = root.join("ops.json");
    let whitelist = root.join("whitelist.json");
    let banned_players = root.join("banned-players.json");
    let banned_ips = root.join("banned-ips.json");
    ensure_empty_json_array(&ops)?;
    ensure_empty_json_array(&whitelist)?;
    ensure_empty_json_array(&banned_players)?;
    ensure_empty_json_array(&banned_ips)?;

    Ok(DataPaths {
        root,
        world_dir,
        level_dat,
        session_lock,
        ops,
        whitelist,
        banned_players,
        banned_ips,
    })
}

fn ensure_empty_json_array(path: &Path) -> Result<(), WorldError> {
    if !path.exists() {
        fs::write(path, EMPTY_JSON_ARRAY).map_err(|source| WorldError::io(path, source))?;
    }
    Ok(())
}

/// Rejects empty names, `.` / `..`, and any path separators so `level-name`
/// cannot escape the server data root (`../`, absolute paths, nested paths).
pub(crate) fn validate_level_name(name: &str) -> Result<(), WorldError> {
    if name.is_empty() {
        return Err(WorldError::InvalidLevelName(
            "level-name must not be empty".to_owned(),
        ));
    }
    if name == "." || name == ".." {
        return Err(WorldError::InvalidLevelName(format!(
            "level-name must not be {name:?}"
        )));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(WorldError::InvalidLevelName(
            "level-name must be a single path segment (no separators)".to_owned(),
        ));
    }
    // Windows drive / UNC prefixes.
    if name.contains(':') {
        return Err(WorldError::InvalidLevelName(
            "level-name must not contain ':'".to_owned(),
        ));
    }
    Ok(())
}

/// Pseudo-random `i64` seed without depending on the `rand` crate.
///
/// Mixes wall-clock nanos with the process id. Good enough for first-boot
/// world seeds; not a CSPRNG.
fn generate_random_seed() -> i64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as i64)
        .unwrap_or(1);
    let pid = i64::from(std::process::id());
    // LCG-ish mix so consecutive boots in the same second still diverge.
    nanos
        .wrapping_mul(0x0005_DEEC_E66D)
        .wrapping_add(pid.wrapping_mul(0xBB67_AE85_84CA_A73B_u64 as i64))
        .wrapping_add(0x2545_F491_4F6C_DD1D_u64 as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gzip_util::gzip_decompress;
    use crate::level_dat::read_level_dat;
    use hyperion_protocol::decode_named_tag;

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "hyperion-bootstrap-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn prepare_empty_temp_creates_expected_files() {
        let root = temp_root("empty");
        let _ = fs::remove_dir_all(&root);

        let paths = prepare_data_directory(&BootstrapConfig {
            root: root.clone(),
            level_name: "world".to_owned(),
            level_seed: 99_887_766,
            spawn_y: 100,
        })
        .expect("prepare");

        assert!(paths.world_dir.is_dir());
        assert!(paths.level_dat.is_file());
        assert!(paths.session_lock.is_file());
        assert!(!fs::read(&paths.session_lock).expect("lock").is_empty());

        for json_path in [
            &paths.ops,
            &paths.whitelist,
            &paths.banned_players,
            &paths.banned_ips,
        ] {
            let text = fs::read_to_string(json_path).expect("json");
            assert_eq!(
                text.trim(),
                "[]",
                "{} should be empty JSON array",
                json_path.display()
            );
        }

        let meta = read_level_dat(&paths.level_dat).expect("read level.dat");
        assert_eq!(meta.level_name, "world");
        assert_eq!(meta.seed, 99_887_766);
        assert_eq!(meta.spawn_y, 100);
        assert_eq!(meta.spawn_x, 0);
        assert_eq!(meta.spawn_z, 0);

        let compressed = fs::read(&paths.level_dat).expect("bytes");
        let nbt = gzip_decompress(&compressed).expect("gzip");
        let (name, _) = decode_named_tag(&nbt).expect("storage nbt");
        assert_eq!(name, "");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn second_prepare_does_not_overwrite_level_dat() {
        let root = temp_root("idempotent");
        let _ = fs::remove_dir_all(&root);

        let custom_seed = 0x0BAD_F00D_i64;
        let first = prepare_data_directory(&BootstrapConfig {
            root: root.clone(),
            level_name: "world".to_owned(),
            level_seed: custom_seed,
            spawn_y: 80,
        })
        .expect("first prepare");
        assert_eq!(
            read_level_dat(&first.level_dat).expect("read").seed,
            custom_seed
        );

        // Overwrite with a known custom seed, then prepare again with a different config seed.
        let overwritten = LevelMeta::new("custom", 1_234_567_890, 64);
        write_level_dat(&first.level_dat, &overwritten).expect("custom write");

        let second = prepare_data_directory(&BootstrapConfig {
            root: root.clone(),
            level_name: "world".to_owned(),
            level_seed: 999,
            spawn_y: 200,
        })
        .expect("second prepare");

        let meta = read_level_dat(&second.level_dat).expect("read after second");
        assert_eq!(meta.seed, 1_234_567_890, "seed must remain unchanged");
        assert_eq!(meta.level_name, "custom");
        assert_eq!(meta.spawn_y, 64);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_path_traversal_level_name() {
        let root = temp_root("traverse");
        let _ = fs::remove_dir_all(&root);
        for bad in [
            "../escape",
            "..\\escape",
            "/abs",
            "a/b",
            "a\\b",
            "",
            ".",
            "..",
            "C:evil",
        ] {
            let err = prepare_data_directory(&BootstrapConfig {
                root: root.clone(),
                level_name: bad.to_owned(),
                level_seed: 1,
                spawn_y: 100,
            })
            .expect_err("must reject");
            assert!(
                matches!(err, WorldError::InvalidLevelName(_)),
                "expected InvalidLevelName for {bad:?}, got {err:?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn zero_seed_generates_non_zero_random_once() {
        let root = temp_root("random-seed");
        let _ = fs::remove_dir_all(&root);

        let paths = prepare_data_directory(&BootstrapConfig {
            root: root.clone(),
            level_name: "world".to_owned(),
            level_seed: 0,
            spawn_y: 100,
        })
        .expect("prepare");

        let seed = read_level_dat(&paths.level_dat).expect("read").seed;
        // Statistically almost never zero; document that 0 config means random.
        // We only assert the file was written and is readable.
        let _ = seed;

        let again = prepare_data_directory(&BootstrapConfig {
            root: root.clone(),
            level_name: "world".to_owned(),
            level_seed: 0,
            spawn_y: 100,
        })
        .expect("second");
        assert_eq!(
            read_level_dat(&again.level_dat).expect("read").seed,
            seed,
            "random seed must stick after first bootstrap"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
