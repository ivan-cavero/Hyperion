//! Simulación de Hyperion.
//!
//! Ticking por regiones (multinúcleo), ECS, física, líquidos, redstone y
//! entidades. El corazón del rendimiento: un hilo por región, cero locks
//! globales, interacción cross-región solo por mensajes.
//! Fase 3 del ROADMAP.

/// Ticks por segundo objetivo (vanilla: 20).
pub const TPS: u32 = 20;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_tps() {
        assert_eq!(TPS, 20);
    }
}
