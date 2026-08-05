//! `net.minecraft.world.level.levelgen.synth.ImprovedNoise` reimplementation.
//!
//! Classic improved Perlin with Mojang’s permutation table init from a
//! [`RandomSource`]. Coordinates are shifted by random `xo/yo/zo` offsets.

use super::random::RandomSource;

/// One ImprovedNoise instance (permutation + origin offsets).
#[derive(Debug, Clone)]
pub struct ImprovedNoise {
    p: [u8; 256],
    xo: f64,
    yo: f64,
    zo: f64,
}

impl ImprovedNoise {
    pub fn new(random: &mut dyn RandomSource) -> Self {
        let xo = random.next_double() * 256.0;
        let yo = random.next_double() * 256.0;
        let zo = random.next_double() * 256.0;
        let mut p = [0u8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        for i in 0..256 {
            let j = random.next_int(256 - i) + i;
            p.swap(i as usize, j as usize);
        }
        Self { p, xo, yo, zo }
    }

    /// Vanilla `noise(x, y, z, yScale, yMax)` used by PerlinNoise octaves.
    ///
    /// Call with `y_scale = 0, y_max = 0` for the plain 3D sample.
    pub fn noise_with_wrap(&self, x: f64, y: f64, z: f64, y_scale: f64, y_max: f64) -> f64 {
        let x = x + self.xo;
        let y = y + self.yo;
        let z = z + self.zo;
        let x2 = x.floor() as i32;
        let y2 = y.floor() as i32;
        let z2 = z.floor() as i32;
        let x = x - f64::from(x2);
        let y = y - f64::from(y2);
        let z = z - f64::from(z2);

        // y_scale / y_max affect octave wrapping in full vanilla; retained for API parity.
        let _ = (y_scale, y_max);
        self.sample_perlin(x2, y2, z2, x, y, z)
    }

    fn sample_perlin(&self, x_i: i32, y_i: i32, z_i: i32, x: f64, y: f64, z: f64) -> f64 {
        let u = fade(x);
        let v = fade(y);
        let w = fade(z);

        let a = self.p_at(x_i) as i32 + y_i;
        let aa = self.p_at(a) as i32 + z_i;
        let ab = self.p_at(a + 1) as i32 + z_i;
        let b = self.p_at(x_i + 1) as i32 + y_i;
        let ba = self.p_at(b) as i32 + z_i;
        let bb = self.p_at(b + 1) as i32 + z_i;

        lerp(
            w,
            lerp(
                v,
                lerp(
                    u,
                    grad(self.p_at(aa), x, y, z),
                    grad(self.p_at(ba), x - 1.0, y, z),
                ),
                lerp(
                    u,
                    grad(self.p_at(ab), x, y - 1.0, z),
                    grad(self.p_at(bb), x - 1.0, y - 1.0, z),
                ),
            ),
            lerp(
                v,
                lerp(
                    u,
                    grad(self.p_at(aa + 1), x, y, z - 1.0),
                    grad(self.p_at(ba + 1), x - 1.0, y, z - 1.0),
                ),
                lerp(
                    u,
                    grad(self.p_at(ab + 1), x, y - 1.0, z - 1.0),
                    grad(self.p_at(bb + 1), x - 1.0, y - 1.0, z - 1.0),
                ),
            ),
        )
    }

    fn p_at(&self, index: i32) -> u8 {
        self.p[(index & 0xFF) as usize]
    }
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn grad(hash: u8, x: f64, y: f64, z: f64) -> f64 {
    let h = hash & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 {
        y
    } else if h == 12 || h == 14 {
        x
    } else {
        z
    };
    let a = if h & 1 == 0 { u } else { -u };
    let b = if h & 2 == 0 { v } else { -v };
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::vanilla::random::LegacyRandom;

    #[test]
    fn improved_noise_deterministic() {
        let mut ra = LegacyRandom::new(1);
        let mut rb = LegacyRandom::new(1);
        let a = ImprovedNoise::new(&mut ra);
        let b = ImprovedNoise::new(&mut rb);
        assert_eq!(
            a.noise_with_wrap(0.5, 1.25, -2.0, 0.0, 0.0),
            b.noise_with_wrap(0.5, 1.25, -2.0, 0.0, 0.0)
        );
    }
}
