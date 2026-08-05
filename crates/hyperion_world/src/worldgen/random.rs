//! Seeded RNGs matching Minecraft Java Edition behaviour (default server math).
//!
//! - [`LegacyRandom`]: `java.util.Random` / `LegacyRandomSource` (48-bit LCG).
//! - [`XoroshiroRandom`]: `XoroshiroRandomSource` (modern overworld default).
//!
//! Own reimplementation of the same algorithms; no third-party worldgen code.

/// Bit-mixing used when promoting a 64-bit world seed to 128-bit Xoroshiro state.
///
/// Matches `RandomSupport.upgradeSeedTo128bitUnmixed` / Stafford-13 style mixing
/// used by modern Java Edition.
fn mix_stafford_13(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Common operations required by noise samplers.
pub trait RandomSource {
    fn next_bits(&mut self, bits: u32) -> i32;
    fn next_int(&mut self, bound: i32) -> i32;
    fn next_long(&mut self) -> i64;
    fn next_double(&mut self) -> f64;
    fn next_float(&mut self) -> f32;
    fn next_gaussian(&mut self) -> f64;
}

/// `java.util.Random` / `LegacyRandomSource`.
#[derive(Debug, Clone)]
pub struct LegacyRandom {
    seed: u64,
    /// Cached next Gaussian (Box–Muller), matching Java's `haveNextNextGaussian`.
    next_gaussian: Option<f64>,
}

impl LegacyRandom {
    const MULTIPLIER: u64 = 0x0005_DEEC_E66D;
    const ADDEND: u64 = 0xB;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> Self {
        let mut r = Self {
            seed: 0,
            next_gaussian: None,
        };
        r.set_seed(seed);
        r
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed as u64 ^ Self::MULTIPLIER) & Self::MASK;
        self.next_gaussian = None;
    }
}

impl RandomSource for LegacyRandom {
    fn next_bits(&mut self, bits: u32) -> i32 {
        self.seed = self
            .seed
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::ADDEND)
            & Self::MASK;
        (self.seed >> (48 - bits)) as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        // Java Random algorithm for nextInt(bound).
        if (bound as u32).is_power_of_two() {
            return ((i64::from(bound) * i64::from(self.next_bits(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next_bits(31);
            let val = bits % bound;
            if bits - val + (bound - 1) >= 0 {
                return val;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        let hi = i64::from(self.next_bits(32));
        let lo = i64::from(self.next_bits(32));
        (hi << 32).wrapping_add(lo)
    }

    fn next_double(&mut self) -> f64 {
        let a = i64::from(self.next_bits(26));
        let b = i64::from(self.next_bits(27));
        ((a << 27) + b) as f64 / ((1u64 << 53) as f64)
    }

    fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 / ((1u32 << 24) as f32)
    }

    fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.next_gaussian.take() {
            return g;
        }
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                self.next_gaussian = Some(v2 * multiplier);
                return v1 * multiplier;
            }
        }
    }
}

/// `XoroshiroRandomSource` (128-bit state).
#[derive(Debug, Clone)]
pub struct XoroshiroRandom {
    seed_lo: u64,
    seed_hi: u64,
    next_gaussian: Option<f64>,
}

impl XoroshiroRandom {
    /// Creates a source from a 64-bit world seed (vanilla upgrade path).
    pub fn from_seed(seed: i64) -> Self {
        let mixed = mix_stafford_13(seed as u64);
        let lo = mix_stafford_13(mixed);
        let hi = mix_stafford_13(mixed.wrapping_add(0x9E37_79B9_7F4A_7C15));
        Self {
            seed_lo: lo,
            seed_hi: hi,
            next_gaussian: None,
        }
    }

    pub fn from_state(seed_lo: u64, seed_hi: u64) -> Self {
        Self {
            seed_lo,
            seed_hi,
            next_gaussian: None,
        }
    }

    fn next_u64(&mut self) -> u64 {
        let s0 = self.seed_lo;
        let mut s1 = self.seed_hi;
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        self.seed_lo = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        self.seed_hi = s1.rotate_left(28);
        result
    }
}

impl RandomSource for XoroshiroRandom {
    fn next_bits(&mut self, bits: u32) -> i32 {
        (self.next_u64() >> (64 - bits)) as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0);
        // Minecraft Xoroshiro nextInt(bound) uses floorMod of nextInt().
        self.next_bits(32).rem_euclid(bound)
    }

    fn next_long(&mut self) -> i64 {
        self.next_u64() as i64
    }

    fn next_double(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / ((1u64 << 53) as f64))
    }

    fn next_float(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / ((1u64 << 24) as f32))
    }

    fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.next_gaussian.take() {
            return g;
        }
        // Same polar method as LegacyRandom.
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                self.next_gaussian = Some(v2 * multiplier);
                return v1 * multiplier;
            }
        }
    }
}

/// Fork a positional random the way vanilla hashes block/seed pairs for features.
///
/// Uses the same Xoroshiro seed promotion as worldgen for now; feature-level
/// `WorldgenRandom` decorators land with the features layer.
pub fn world_seed_to_xoroshiro(seed: i64) -> XoroshiroRandom {
    XoroshiroRandom::from_seed(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_next_int_power_of_two() {
        let mut r = LegacyRandom::new(1);
        // Deterministic stream: just ensure bounds.
        for _ in 0..100 {
            let v = r.next_int(16);
            assert!((0..16).contains(&v));
        }
    }

    #[test]
    fn legacy_is_deterministic() {
        let mut a = LegacyRandom::new(12345);
        let mut b = LegacyRandom::new(12345);
        for _ in 0..50 {
            assert_eq!(a.next_long(), b.next_long());
            assert_eq!(a.next_double(), b.next_double());
        }
    }

    #[test]
    fn xoroshiro_is_deterministic() {
        let mut a = XoroshiroRandom::from_seed(999);
        let mut b = XoroshiroRandom::from_seed(999);
        for _ in 0..50 {
            assert_eq!(a.next_long(), b.next_long());
        }
        let mut c = XoroshiroRandom::from_seed(1000);
        assert_ne!(a.next_long(), c.next_long());
    }

    #[test]
    fn legacy_matches_java_util_random_known_stream() {
        // seed=0 → Java Random nextInt sequence first values (known reference).
        let mut r = LegacyRandom::new(0);
        // From Java: new Random(0).nextInt() first call is -1155484576
        assert_eq!(r.next_bits(32), -1155484576);
    }
}
