//! Per-player chunk view: track loaded columns, stream new ones on move,
//! unload those outside the view radius.
//!
//! # Performance contract (Hyperion vs vanilla/Paper)
//!
//! | Concern | Policy |
//! |---------|--------|
//! | RAM | One flat network template per connection; **never** hold 289×50 KiB vectors |
//! | CPU | Clone+patch coords only; no light/NBT rebuild on send |
//! | Disk | **Not** on the send path — mark dirty, flush later (Paper-style) |
//! | Network | Caller batches TCP writes and flushes once per batch |
//!
//! Goal: serve/explore faster than vanilla while staying protocol-correct.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use hyperion_protocol::ProtocolError;
use hyperion_world::{FlatNetworkCache, ensure_flat_on_disk, position_to_chunk, snap_ground_y};
use tracing::debug;

/// One step of view update after the player changes chunk.
#[derive(Debug, Default)]
pub struct ViewUpdate {
    /// New chunk-cache center when the player crossed a chunk border.
    pub new_center: Option<(i32, i32)>,
    /// Chunk coordinates that must be sent (encode with [`ChunkView::payload`]).
    pub to_send: Vec<(i32, i32)>,
    /// Chunk coordinates to unload on the client.
    pub to_unload: Vec<(i32, i32)>,
}

/// Tracks which chunks a connection currently has loaded.
#[derive(Debug)]
pub struct ChunkView {
    center_x: i32,
    center_z: i32,
    radius: i32,
    loaded: HashSet<(i32, i32)>,
    /// Columns that need an Anvil write (lazy disk).
    dirty: HashSet<(i32, i32)>,
    world_dir: PathBuf,
    /// Shared flat network template (coords patched per send).
    network: Option<Arc<FlatNetworkCache>>,
    /// When false, only the initial void/synthetic spawn is used (unit tests).
    streaming: bool,
}

impl ChunkView {
    /// Opens a view around `(0, 0)` without encoding payloads (zero bulk RAM).
    ///
    /// Returns `(view, feet_y, initial_chunk_count)`.
    pub fn spawn(
        world_dir: impl Into<PathBuf>,
        view_distance: i32,
        spawn_y: i32,
    ) -> Result<(Self, i32, i32), ProtocolError> {
        let world_dir = world_dir.into();
        let radius = view_distance.clamp(1, 8);
        let ground_y = snap_ground_y(spawn_y.saturating_sub(1));
        let streaming = !world_dir.as_os_str().is_empty();

        if !streaming {
            let view = Self {
                center_x: 0,
                center_z: 0,
                radius,
                loaded: HashSet::new(),
                dirty: HashSet::new(),
                world_dir,
                network: None,
                streaming: false,
            };
            return Ok((view, spawn_y, 0));
        }

        let network = Arc::new(FlatNetworkCache::new(ground_y)?);
        let feet_y = network.surface_y;
        let edge = 2 * radius + 1;
        let count = edge * edge;
        let mut loaded = HashSet::with_capacity(count as usize);
        let mut dirty = HashSet::with_capacity(count as usize);

        for chunk_z in -radius..=radius {
            for chunk_x in -radius..=radius {
                loaded.insert((chunk_x, chunk_z));
                // Defer disk: mark dirty, flush after the client is playing.
                dirty.insert((chunk_x, chunk_z));
            }
        }

        let view = Self {
            center_x: 0,
            center_z: 0,
            radius,
            loaded,
            dirty,
            world_dir,
            network: Some(network),
            streaming: true,
        };
        Ok((view, feet_y, count))
    }

    /// Whether movement should trigger load/unload (real world directory).
    pub fn streaming_enabled(&self) -> bool {
        self.streaming
    }

    /// Number of columns currently marked loaded for this connection.
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Encodes one flat column (template clone + coord patch). Cheap.
    pub fn payload(&self, chunk_x: i32, chunk_z: i32) -> Option<Vec<u8>> {
        self.network
            .as_ref()
            .map(|cache| cache.payload(chunk_x, chunk_z))
    }

    /// Iterates spawn square coords in send order (z outer, x inner).
    pub fn initial_coords(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let r = self.radius;
        (-r..=r).flat_map(move |z| (-r..=r).map(move |x| (x, z)))
    }

    /// Updates the view from an absolute player position.
    pub fn on_position(&mut self, x: f64, z: f64) -> Result<ViewUpdate, ProtocolError> {
        if !self.streaming {
            return Ok(ViewUpdate::default());
        }
        let (cx, cz) = position_to_chunk(x, z);
        if cx == self.center_x && cz == self.center_z {
            return Ok(ViewUpdate::default());
        }
        Ok(self.retarget(cx, cz))
    }

    /// Best-effort flush of dirty columns to Anvil (append path).
    ///
    /// Call after spawn / between movement bursts — never inside the per-packet
    /// encode loop. Caps work per call so a huge dirty set cannot stall a tick.
    pub fn flush_dirty_budget(&mut self, max_writes: usize) -> usize {
        if max_writes == 0 || self.dirty.is_empty() {
            return 0;
        }
        let Some(network) = self.network.as_ref() else {
            return 0;
        };
        let ground_y = network.ground_y;
        let mut written = 0usize;
        let batch: Vec<(i32, i32)> = self.dirty.iter().copied().take(max_writes).collect();
        for (chunk_x, chunk_z) in batch {
            match ensure_flat_on_disk(&self.world_dir, chunk_x, chunk_z, ground_y) {
                Ok(_) => {
                    self.dirty.remove(&(chunk_x, chunk_z));
                    written += 1;
                }
                Err(error) => {
                    debug!(
                        world = %self.world_dir.display(),
                        chunk_x,
                        chunk_z,
                        %error,
                        "lazy flat persist failed; will retry"
                    );
                    // leave in dirty for retry
                }
            }
        }
        written
    }

    fn retarget(&mut self, cx: i32, cz: i32) -> ViewUpdate {
        let mut desired = HashSet::with_capacity(((2 * self.radius + 1) as usize).pow(2));
        for dz in -self.radius..=self.radius {
            for dx in -self.radius..=self.radius {
                desired.insert((cx + dx, cz + dz));
            }
        }

        let mut update = ViewUpdate {
            new_center: Some((cx, cz)),
            to_send: Vec::new(),
            to_unload: Vec::new(),
        };

        for &coord in &self.loaded {
            if !desired.contains(&coord) {
                update.to_unload.push(coord);
            }
        }
        for &coord in &desired {
            if !self.loaded.contains(&coord) {
                update.to_send.push(coord);
                self.dirty.insert(coord);
            }
        }

        for coord in &update.to_unload {
            self.loaded.remove(coord);
        }
        for coord in desired {
            self.loaded.insert(coord);
        }
        self.center_x = cx;
        self.center_z = cz;
        update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type ChunkSet = HashSet<(i32, i32)>;

    fn view_delta(
        old_center: (i32, i32),
        new_center: (i32, i32),
        radius: i32,
    ) -> (ChunkSet, ChunkSet) {
        let mut old = HashSet::new();
        let mut new = HashSet::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                old.insert((old_center.0 + dx, old_center.1 + dz));
                new.insert((new_center.0 + dx, new_center.1 + dz));
            }
        }
        (
            new.difference(&old).copied().collect(),
            old.difference(&new).copied().collect(),
        )
    }

    #[test]
    fn moving_one_chunk_loads_a_strip_and_unloads_the_opposite() {
        let (load, unload) = view_delta((0, 0), (1, 0), 2);
        assert_eq!(load.len(), 5);
        assert_eq!(unload.len(), 5);
        assert!(load.iter().all(|(x, _)| *x == 3));
        assert!(unload.iter().all(|(x, _)| *x == -2));
    }

    #[test]
    fn same_center_has_empty_delta() {
        let (load, unload) = view_delta((2, -1), (2, -1), 4);
        assert!(load.is_empty());
        assert!(unload.is_empty());
    }

    #[test]
    fn non_streaming_spawn_is_empty() {
        let (mut view, feet, count) = ChunkView::spawn(PathBuf::new(), 8, 64).expect("spawn");
        assert!(!view.streaming_enabled());
        assert_eq!(count, 0);
        assert_eq!(feet, 64);
        let u = view.on_position(999.0, 999.0).expect("move");
        assert!(u.to_send.is_empty());
        assert!(u.to_unload.is_empty());
        assert!(u.new_center.is_none());
    }

    #[test]
    fn spawn_does_not_allocate_payload_batch() {
        // Streaming spawn only marks coords; payloads are produced on demand.
        let dir = std::env::temp_dir().join(format!("hyperion-view-spawn-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("region"));
        let (view, feet, count) = ChunkView::spawn(&dir, 2, 64).expect("spawn");
        assert!(view.streaming_enabled());
        assert_eq!(count, 25);
        assert_eq!(feet, 64);
        assert_eq!(view.loaded_count(), 25);
        // Dirty set holds work for later; disk not required yet.
        assert_eq!(view.dirty.len(), 25);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flat_template_patches_coords() {
        let cache = FlatNetworkCache::new(63).expect("template");
        let a = cache.payload(0, 0);
        let b = cache.payload(3, -2);
        assert_eq!(&a[..4], &0i32.to_be_bytes());
        assert_eq!(&b[..4], &3i32.to_be_bytes());
        assert_eq!(&b[4..8], &(-2i32).to_be_bytes());
        assert_eq!(&a[8..], &b[8..]);
    }
}
