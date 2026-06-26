//! Actor model + core stat derivation (`j.java` + the pure parts of `h.java`).
//!
//! `j.java` is a flat field blob (the player/NPC state). Per `spec.txt` we keep
//! the original obfuscated field names for now (renaming risks behavioral drift;
//! rename once combat is fully understood) and port the *pure, deterministic*
//! stat math first — the health/fatigue derivation that `h.java` recomputes in
//! many places (`void_a`, `f`, the item-apply paths), always with the same
//! formula:
//!
//! ```text
//! max_health  (var_short_o) = level*4 + (str + O)*2 + endurance*2 + I
//! health_rate (var_short_d) = 40000 / max_health
//! max_fatigue (var_short_p) = level*4 + agility*2 + J
//! fatigue_rate(var_short_f) = 40000 / max_fatigue
//! ```
//! where `level = var_byte_o`, `str = var_short_s`, `endurance = var_short_x`,
//! `agility = var_short_t`, and `O/I/J` are equipment/spell bonuses.
//!
//! Combat resolution, inventory item-application, and the class/level bonus
//! tables in `h.f` (which are coupled to the `.scr` stat tables) are the next
//! slices of M8 and are not yet ported.

/// A faithful, scalar subset of `j.java` (the fields the stat math and save
/// touch). Names mirror the decompiled source; types match Java widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    // --- identity / class ---
    pub var_byte_c: i8, // appearance/sex flag
    pub var_byte_f: i8, // class id
    pub var_byte_j: i8, // race/spec id
    pub var_byte_o: i8, // level

    // --- base attributes (the stat inputs) ---
    pub var_short_s: i16, // strength
    pub var_short_t: i16, // agility
    pub var_short_u: i16,
    pub var_short_v: i16,
    pub var_short_w: i16,
    pub var_short_x: i16, // endurance
    pub i_bonus: i16,     // j.I  (health bonus from equipment/spells)
    pub j_bonus: i16,     // j.J  (fatigue bonus)
    pub o_bonus: i16,     // j.O  (strength-ish bonus feeding health)
    pub e_field: i16,     // j.E
    pub f_field: i16,     // j.F

    // --- derived stats (outputs of [`Actor::recompute`]) ---
    pub var_short_o: i16, // max health
    pub var_short_p: i16, // max fatigue
    pub var_short_q: i16, // current health
    pub var_short_r: i16, // current fatigue
    pub var_short_d: i16, // health regen rate
    pub var_short_f: i16, // fatigue regen rate

    pub var_byte_r: i8,
    pub var_int_b: i32,
}

impl Default for Actor {
    /// Initial values from `j.java`'s field initializers.
    fn default() -> Self {
        Self {
            var_byte_c: 0,
            var_byte_f: -1,
            var_byte_j: 0,
            var_byte_o: 0,
            var_short_s: 0,
            var_short_t: 0,
            var_short_u: 0,
            var_short_v: 0,
            var_short_w: 0,
            var_short_x: 0,
            i_bonus: 0,
            j_bonus: 0,
            o_bonus: 0,
            e_field: 0,
            f_field: 0,
            var_short_o: 100,
            var_short_p: 100,
            var_short_q: 1,
            var_short_r: 1,
            var_short_d: 0,
            var_short_f: 0,
            var_byte_r: 1,
            var_int_b: 0,
        }
    }
}

impl Actor {
    /// Recompute max health/fatigue and their regen rates from base attributes,
    /// exactly as `h.java` does. Division-by-zero (degenerate stats) yields 0
    /// rather than panicking (Java would throw; real actors never hit it).
    pub fn recompute(&mut self) {
        let level = i32::from(self.var_byte_o);
        let max_health = level * 4
            + (i32::from(self.var_short_s) + i32::from(self.o_bonus)) * 2
            + i32::from(self.var_short_x) * 2
            + i32::from(self.i_bonus);
        let max_fatigue = level * 4 + i32::from(self.var_short_t) * 2 + i32::from(self.j_bonus);

        self.var_short_o = max_health as i16;
        self.var_short_d = if max_health != 0 {
            (40000 / max_health) as i16
        } else {
            0
        };
        self.var_short_p = max_fatigue as i16;
        self.var_short_f = if max_fatigue != 0 {
            (40000 / max_fatigue) as i16
        } else {
            0
        };
    }

    /// Recompute derived stats and set current health/fatigue to full — the
    /// `h.void_a` initialization (`var_short_q = var_short_o`, `var_short_r =
    /// var_short_p`).
    pub fn recompute_to_full(&mut self) {
        self.recompute();
        self.var_short_q = self.var_short_o;
        self.var_short_r = self.var_short_p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_health_and_fatigue() {
        let mut a = Actor {
            var_byte_o: 5,   // level
            var_short_s: 40, // strength
            var_short_x: 30, // endurance
            var_short_t: 50, // agility
            o_bonus: 10,
            i_bonus: 7,
            j_bonus: 3,
            ..Actor::default()
        };
        a.recompute_to_full();
        // max_health = 5*4 + (40+10)*2 + 30*2 + 7 = 20 + 100 + 60 + 7 = 187
        assert_eq!(a.var_short_o, 187);
        assert_eq!(a.var_short_q, 187); // full
        assert_eq!(a.var_short_d, (40000 / 187) as i16); // 213
                                                         // max_fatigue = 5*4 + 50*2 + 3 = 20 + 100 + 3 = 123
        assert_eq!(a.var_short_p, 123);
        assert_eq!(a.var_short_r, 123);
        assert_eq!(a.var_short_f, (40000 / 123) as i16); // 325
    }

    #[test]
    fn zero_stats_do_not_divide_by_zero() {
        let mut a = Actor::default(); // level 0, all attrs 0 -> max 0
        a.recompute();
        assert_eq!(a.var_short_o, 0);
        assert_eq!(a.var_short_d, 0); // guarded, no panic
        assert_eq!(a.var_short_f, 0);
    }

    #[test]
    fn default_matches_j_java_initializers() {
        let a = Actor::default();
        assert_eq!(a.var_byte_f, -1);
        assert_eq!(a.var_short_o, 100);
        assert_eq!(a.var_short_q, 1);
        assert_eq!(a.var_byte_r, 1);
    }
}
