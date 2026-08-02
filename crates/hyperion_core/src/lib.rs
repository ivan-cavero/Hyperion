//! Fundamentos de Hyperion.
//!
//! Tipos base, matemáticas de mundo y abstracciones compartidas.
//! Fase 0: se mantiene sin dependencias externas — todo lo que vive aquí
//! es código propio (política: docs/DEPENDENCIES.md).

/// Número de version de Hyperion expuesto por el binario.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Punto 3D en coordenadas de bloque.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_pos_default_is_origin() {
        assert_eq!(BlockPos::default(), BlockPos::new(0, 0, 0));
    }
}
