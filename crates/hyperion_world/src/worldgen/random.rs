//! Seeded RNGs matching Minecraft Java Edition behaviour (default server math).
//!
//! - [`LegacyRandom`]: `java.util.Random` / `LegacyRandomSource` (48-bit LCG).
//! - [`XoroshiroRandom`]: `XoroshiroRandomSource` (modern overworld default).
//! - [`PositionalRandomFactory`]: `XoroshiroPositionalRandomFactory` — per-noise
//!   and per-block forks used by `RandomState`.
//!
//! Own reimplementation of the same algorithms; no third-party worldgen code.

/// Stafford-13 mix (`RandomSupport.mixStafford13`).
fn mix_stafford_13(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `RandomSupport.SILVER_RATIO_64` — used by upgradeSeedTo128bitUnmixed.
const SILVER_RATIO_64: u64 = 0x6A09_E667_F3BC_C909;
/// `RandomSupport.GOLDEN_RATIO_64`.
const GOLDEN_RATIO_64: u64 = 0x9E37_79B9_7F4A_7C15;

/// 128-bit seed pair (`RandomSupport.Seed128bit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed128 {
    pub lo: u64,
    pub hi: u64,
}

impl Seed128 {
    pub fn new(lo: u64, hi: u64) -> Self {
        Self { lo, hi }
    }

    pub fn xor(self, lo: u64, hi: u64) -> Self {
        Self {
            lo: self.lo ^ lo,
            hi: self.hi ^ hi,
        }
    }

    pub fn mixed(self) -> Self {
        Self {
            lo: mix_stafford_13(self.lo),
            hi: mix_stafford_13(self.hi),
        }
    }
}

/// `RandomSupport.upgradeSeedTo128bitUnmixed`.
pub fn upgrade_seed_to_128bit_unmixed(seed: i64) -> Seed128 {
    let lo = (seed as u64) ^ SILVER_RATIO_64;
    let hi = lo.wrapping_add(GOLDEN_RATIO_64);
    Seed128::new(lo, hi)
}

/// `RandomSupport.upgradeSeedTo128bit` (mixed).
pub fn upgrade_seed_to_128bit(seed: i64) -> Seed128 {
    upgrade_seed_to_128bit_unmixed(seed).mixed()
}

/// `RandomSupport.seedFromHashOf` — MD5 of UTF-8 string → two big-endian longs.
pub fn seed_from_hash_of(name: &str) -> Seed128 {
    let digest = md5::compute(name.as_bytes());
    let b = digest.0;
    let lo = i64::from_be_bytes(b[0..8].try_into().expect("8 bytes")) as u64;
    let hi = i64::from_be_bytes(b[8..16].try_into().expect("8 bytes")) as u64;
    Seed128::new(lo, hi)
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
    /// `new XoroshiroRandomSource(long)` — upgrades world seed via
    /// `RandomSupport.upgradeSeedTo128bit`.
    pub fn from_seed(seed: i64) -> Self {
        let s = upgrade_seed_to_128bit(seed);
        Self::from_state(s.lo, s.hi)
    }

    pub fn from_state(seed_lo: u64, seed_hi: u64) -> Self {
        Self {
            seed_lo,
            seed_hi,
            next_gaussian: None,
        }
    }

    pub fn from_seed128(seed: Seed128) -> Self {
        Self::from_state(seed.lo, seed.hi)
    }

    /// `fork()` — two `nextLong` values become the child state.
    pub fn fork(&mut self) -> Self {
        let lo = self.next_u64();
        let hi = self.next_u64();
        Self::from_state(lo, hi)
    }

    /// `forkPositional()` — two `nextLong` values seed the factory.
    pub fn fork_positional(&mut self) -> PositionalRandomFactory {
        let lo = self.next_u64();
        let hi = self.next_u64();
        PositionalRandomFactory::new(lo, hi)
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

    /// Unbounded `nextInt()` — low 32 bits of `nextLong` (`l2i`).
    fn next_int_unbounded(&mut self) -> i32 {
        self.next_u64() as i32
    }
}

impl RandomSource for XoroshiroRandom {
    fn next_bits(&mut self, bits: u32) -> i32 {
        // High bits (used by nextFloat / nextDouble path).
        (self.next_u64() >> (64 - bits)) as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        // Lemire almost-division-free method as in JE XoroshiroRandomSource.
        assert!(bound > 0, "bound must be positive");
        let bound_u = bound as u32 as u64;
        let mut m = (self.next_int_unbounded() as u32 as u64).wrapping_mul(bound_u);
        let mut low = m & 0xFFFF_FFFF;
        if low < bound_u {
            let t = ((!bound as u32).wrapping_add(1) % bound as u32) as u64;
            while low < t {
                m = (self.next_int_unbounded() as u32 as u64).wrapping_mul(bound_u);
                low = m & 0xFFFF_FFFF;
            }
        }
        (m >> 32) as i32
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

/// `XoroshiroPositionalRandomFactory` — derived RNGs for named noises / blocks.
#[derive(Debug, Clone, Copy)]
pub struct PositionalRandomFactory {
    seed_lo: u64,
    seed_hi: u64,
}

impl PositionalRandomFactory {
    pub fn new(seed_lo: u64, seed_hi: u64) -> Self {
        Self { seed_lo, seed_hi }
    }

    /// Build the factory the same way `RandomState` does for a world seed:
    /// `XoroshiroRandomSource(seed).forkPositional()`.
    pub fn from_world_seed(world_seed: i64) -> Self {
        let mut r = XoroshiroRandom::from_seed(world_seed);
        r.fork_positional()
    }

    /// `fromHashOf(String)` — MD5 of name XORed with factory seeds.
    pub fn from_hash_of(&self, name: &str) -> XoroshiroRandom {
        let hashed = seed_from_hash_of(name).xor(self.seed_lo, self.seed_hi);
        XoroshiroRandom::from_seed128(hashed)
    }

    /// `fromSeed(long)` — XOR seed into both halves.
    pub fn from_seed_long(&self, seed: i64) -> XoroshiroRandom {
        let s = seed as u64;
        XoroshiroRandom::from_state(s ^ self.seed_lo, s ^ self.seed_hi)
    }
}

/// Fork a positional random the way vanilla hashes block/seed pairs for features.
pub fn world_seed_to_xoroshiro(seed: i64) -> XoroshiroRandom {
    XoroshiroRandom::from_seed(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_next_int_power_of_two() {
        let mut r = LegacyRandom::new(1);
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
        let mut r = LegacyRandom::new(0);
        assert_eq!(r.next_bits(32), -1155484576);
    }

    #[test]
    fn upgrade_seed_matches_known_xor_path() {
        // seed=0 → lo = SILVER, hi = SILVER+GOLDEN, then mixed.
        let s = upgrade_seed_to_128bit_unmixed(0);
        assert_eq!(s.lo, SILVER_RATIO_64);
        assert_eq!(s.hi, SILVER_RATIO_64.wrapping_add(GOLDEN_RATIO_64));
        let m = s.mixed();
        assert_eq!(m.lo, mix_stafford_13(s.lo));
        assert_eq!(m.hi, mix_stafford_13(s.hi));
    }

    #[test]
    fn seed_from_hash_of_is_stable_md5() {
        let s = seed_from_hash_of("minecraft:temperature");
        // Cross-check with Python hashlib.md5 + big-endian i64.
        let digest = md5::compute(b"minecraft:temperature");
        let lo = i64::from_be_bytes(digest.0[0..8].try_into().unwrap()) as u64;
        let hi = i64::from_be_bytes(digest.0[8..16].try_into().unwrap()) as u64;
        assert_eq!(s.lo, lo);
        assert_eq!(s.hi, hi);
        // Different names → different seeds.
        assert_ne!(
            seed_from_hash_of("minecraft:temperature"),
            seed_from_hash_of("minecraft:vegetation")
        );
    }

    #[test]
    fn positional_from_hash_xor_factory() {
        let factory = PositionalRandomFactory::new(1, 2);
        let mut a = factory.from_hash_of("minecraft:ore");
        let mut b = factory.from_hash_of("minecraft:ore");
        assert_eq!(a.next_long(), b.next_long());
        let mut c = factory.from_hash_of("minecraft:aquifer");
        assert_ne!(a.next_long(), c.next_long());
    }

    #[test]
    fn world_seed_positional_is_deterministic() {
        let a = PositionalRandomFactory::from_world_seed(12345);
        let b = PositionalRandomFactory::from_world_seed(12345);
        let mut ra = a.from_hash_of("minecraft:temperature");
        let mut rb = b.from_hash_of("minecraft:temperature");
        for _ in 0..20 {
            assert_eq!(ra.next_long(), rb.next_long());
        }
    }

    #[test]
    fn xoroshiro_next_int_bound_in_range() {
        let mut r = XoroshiroRandom::from_seed(7);
        for _ in 0..200 {
            let v = r.next_int(17);
            assert!((0..17).contains(&v));
        }
    }
}
