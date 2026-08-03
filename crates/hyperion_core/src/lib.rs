//! Hyperion core types.
//!
//! Base types, world math, and shared abstractions.
//! Phase 0: zero external dependencies — everything here is original code
//! (policy: docs/DEPENDENCIES.md).

/// Hyperion version string exposed by the binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A 3D block coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}
