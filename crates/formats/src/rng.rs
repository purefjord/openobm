//! Faithful reimplementation of `java.util.Random` (the 48-bit linear
//! congruential generator), needed so the combat port consumes the *identical*
//! pseudo-random sequence as the game's `b.var_java_util_Random_a`.
//!
//! The game only ever calls the unbounded `nextInt()` (then takes `% n`), so
//! that is all we model. Seeding matches `new Random(seed)` / `setSeed(seed)`
//! exactly (`(seed ^ MULT) & MASK`), and [`JavaRandom::raw_seed`] exposes the
//! internal 48-bit state so an oracle can read back `Random.seed` and confirm
//! the same number of draws were consumed.

const MULT: i64 = 0x5_DEEC_E66D;
const ADD: i64 = 0xB;
const MASK: i64 = (1 << 48) - 1;

/// A bit-exact `java.util.Random`.
#[derive(Debug, Clone)]
pub struct JavaRandom {
    seed: i64,
}

impl JavaRandom {
    /// Equivalent to `new Random(seed)`.
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed ^ MULT) & MASK,
        }
    }

    /// Equivalent to `setSeed(seed)`.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULT) & MASK;
    }

    /// `protected int next(int bits)`.
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULT).wrapping_add(ADD) & MASK;
        // seed is a non-negative 48-bit value; logical >>> then truncate to i32.
        (self.seed >> (48 - i64::from(bits))) as i32
    }

    /// `public int nextInt()` (the full 32-bit form).
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// The internal 48-bit seed state (matches `Random`'s private `seed` field),
    /// for diffing RNG consumption against the live oracle.
    pub fn raw_seed(&self) -> i64 {
        self.seed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_java_known_sequence() {
        // Verified against the JDK: new Random(0).nextInt() x3.
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next_int(), -1_155_484_576);
        assert_eq!(r.next_int(), -723_955_400);
        assert_eq!(r.next_int(), 1_033_096_058);

        // Verified against the JDK: new Random(42).nextInt() x2.
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(), -1_170_105_035);
        assert_eq!(r.next_int(), 234_785_527);
    }

    #[test]
    fn set_seed_matches_constructor() {
        let mut a = JavaRandom::new(12345);
        let mut b = JavaRandom::new(0);
        b.set_seed(12345);
        for _ in 0..5 {
            assert_eq!(a.next_int(), b.next_int());
        }
        assert_eq!(a.raw_seed(), b.raw_seed());
    }
}
