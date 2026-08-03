//! Hyperion simulation.
//!
//! Per-region ticking (multicore), ECS, physics, liquids, redstone, and
//! entities. The performance core: one thread per region, zero global locks,
//! cross-region interaction only via messages.
//! Roadmap Phase 3.

/// Target ticks per second (vanilla: 20).
pub const TPS: u32 = 20;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_tps() {
        assert_eq!(TPS, 20);
    }
}
