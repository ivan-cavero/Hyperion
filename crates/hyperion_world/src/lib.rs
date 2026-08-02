//! Mundo de Hyperion.
//!
//! Chunks, worldgen con paridad vanilla 1:1 (misma seed, mismo mundo),
//! formato Anvil (compat) y el formato propio HCF.
//! Fase 2 del ROADMAP.

/// Semilla del mundo (u64, como en Java Edition).
pub type Seed = u64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_u64() {
        let _seed: Seed = 42;
    }
}
