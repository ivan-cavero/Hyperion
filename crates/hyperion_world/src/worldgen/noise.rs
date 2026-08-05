//! Lightweight deterministic 2D value noise (own core, no external crates).

/// Smooth value noise in roughly `[-1, 1]` for world coordinates.
pub fn value_noise_2d(x: f64, z: f64, seed: u64) -> f64 {
    let x0 = x.floor() as i32;
    let z0 = z.floor() as i32;
    let fx = x - f64::from(x0);
    let fz = z - f64::from(z0);
    let u = smoothstep(fx);
    let v = smoothstep(fz);

    let n00 = lattice(x0, z0, seed);
    let n10 = lattice(x0 + 1, z0, seed);
    let n01 = lattice(x0, z0 + 1, seed);
    let n11 = lattice(x0 + 1, z0 + 1, seed);

    let nx0 = lerp(n00, n10, u);
    let nx1 = lerp(n01, n11, u);
    lerp(nx0, nx1, v)
}

/// Surface height (highest solid block Y) for a world column.
///
/// Base sea-level hills: roughly Y 52..=76 with gentle variation.
pub fn surface_height(world_x: i32, world_z: i32, seed: u64) -> i32 {
    let x = f64::from(world_x);
    let z = f64::from(world_z);
    let continental = value_noise_2d(x * 0.004, z * 0.004, seed) * 10.0;
    let hills = value_noise_2d(x * 0.02, z * 0.02, seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)) * 8.0;
    let detail = value_noise_2d(x * 0.08, z * 0.08, seed.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)) * 3.0;
    let h = 64.0 + continental + hills + detail;
    h.round().clamp(-59.0, 310.0) as i32
}

#[inline]
fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Lattice contribution in `[-1, 1]`.
fn lattice(x: i32, z: i32, seed: u64) -> f64 {
    let h = mix64(
        seed ^ (x as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)
            ^ (z as u64).wrapping_mul(0xA076_1D64_78BD_642F),
    );
    // Map high bits to [-1, 1].
    let unit = (h >> 11) as f64 / ((1u64 << 53) as f64);
    unit * 2.0 - 1.0
}

/// SplitMix64-style finalizer.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic() {
        let a = value_noise_2d(12.5, -3.25, 42);
        let b = value_noise_2d(12.5, -3.25, 42);
        assert_eq!(a, b);
        let c = value_noise_2d(12.5, -3.25, 43);
        assert_ne!(a, c);
    }

    #[test]
    fn height_in_overworld_range() {
        for z in -32..32 {
            for x in -32..32 {
                let h = surface_height(x, z, 12345);
                assert!((-59..=310).contains(&h), "height {h} at {x},{z}");
            }
        }
    }

    #[test]
    fn same_seed_same_height() {
        assert_eq!(surface_height(0, 0, 99), surface_height(0, 0, 99));
        // Distant columns differ with high probability; scan a strip.
        let origin = surface_height(0, 0, 99);
        let differs = (1..64).any(|x| surface_height(x, 0, 99) != origin);
        assert!(differs, "height should vary along X for a given seed");
    }
}
