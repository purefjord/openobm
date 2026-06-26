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

    // --- equipment/spell bonus fields (j.K..P) ---
    pub k_bonus: i16, // j.K
    pub l_bonus: i16, // j.L
    pub m_bonus: i16, // j.M
    pub n_bonus: i16, // j.N
    pub p_bonus: i16, // j.P

    // --- derived stats (outputs of [`Actor::recompute`]) ---
    pub var_short_o: i16, // max health
    pub var_short_p: i16, // max fatigue
    pub var_short_q: i16, // current health
    pub var_short_r: i16, // current fatigue
    pub var_short_d: i16, // health regen rate
    pub var_short_f: i16, // fatigue regen rate

    // --- effect/status fields touched by item application ---
    pub var_short_j: i16,
    pub var_short_k: i16,
    pub var_short_l: i16,
    pub var_byte_w: i8,
    /// Queued health-over-time consumable (j.var_int_arr_f).
    pub queued_health: Option<Vec<i32>>,
    /// Queued fatigue-over-time consumable (j.var_int_arr_g).
    pub queued_fatigue: Option<Vec<i32>>,

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
            k_bonus: 0,
            l_bonus: 0,
            m_bonus: 0,
            n_bonus: 0,
            p_bonus: 0,
            var_short_o: 100,
            var_short_p: 100,
            var_short_q: 1,
            var_short_r: 1,
            var_short_d: 0,
            var_short_f: 0,
            var_short_j: 0,
            var_short_k: 0,
            var_short_l: 0,
            var_byte_w: -1,
            queued_health: None,
            queued_fatigue: None,
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

    fn recompute_health_only(&mut self) {
        let level = i32::from(self.var_byte_o);
        let o = level * 4
            + (i32::from(self.var_short_s) + i32::from(self.o_bonus)) * 2
            + i32::from(self.var_short_x) * 2
            + i32::from(self.i_bonus);
        self.var_short_o = o as i16;
        self.var_short_d = if o != 0 { (40000 / o) as i16 } else { 0 };
    }

    fn recompute_fatigue_full(&mut self) {
        let level = i32::from(self.var_byte_o);
        let p = level * 4 + i32::from(self.var_short_t) * 2 + i32::from(self.j_bonus);
        // Java sets both max (p) and current (r) here.
        self.var_short_p = p as i16;
        self.var_short_r = p as i16;
        self.var_short_f = if p != 0 { (40000 / p) as i16 } else { 0 };
    }

    /// Apply an item/spell stat-modifier row to this actor — the direct field
    /// effects of `h.java::b(j, int[])`. `m` is a parsed `.scr` item/spell row
    /// (tag-indexed). `restore_health` is the caller-resolved gate for the
    /// consumable case (`lang(m[1]) == lang(158)`, i.e. a Restore-Health effect).
    ///
    /// This ports the *direct* stat changes; the secondary `h.b(j,2,m)` pass and
    /// the `h.f` class/level bonus layer are deferred (they add resistances and
    /// class progression and are best validated against the runtime oracle).
    pub fn apply_modifier(&mut self, m: &[i32], restore_health: bool) {
        let g = |i: usize| m.get(i).copied().unwrap_or(0);
        if g(5) == 0 {
            // consumable
            if restore_health {
                let q = (i32::from(self.var_short_q) + g(2)).min(i32::from(self.var_short_o));
                self.var_short_q = q as i16;
                let r = (i32::from(self.var_short_r) + g(3)).min(i32::from(self.var_short_p));
                self.var_short_r = r as i16;
            }
            if g(2) > 0 {
                self.queued_health = Some(m.to_vec());
                return;
            }
            if g(3) > 0 {
                self.queued_fatigue = Some(m.to_vec());
                return;
            }
            if g(4) > 0 {
                self.var_short_k = 0;
                self.var_short_l = 0;
                self.var_byte_w = -1;
            }
        } else {
            // equipment / spell
            if g(3) > 0 {
                self.j_bonus = g(3) as i16;
                self.recompute_fatigue_full();
            }
            if g(6) > 0 {
                self.k_bonus = g(6) as i16;
            }
            if g(7) > 0 {
                self.l_bonus = g(7) as i16;
            }
            if g(8) > 0 {
                self.m_bonus = g(8) as i16;
            }
            if g(10) > 0 {
                self.n_bonus = g(10) as i16;
            }
            if g(11) > 0 {
                self.o_bonus = g(11) as i16;
                self.recompute_health_only();
            }
            if g(4) > 0 {
                self.var_short_k = 0;
                self.var_short_l = 0;
                self.var_byte_w = -1;
            }
            self.var_short_j = 0;
            self.p_bonus = g(5) as i16;
        }
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
    fn equipment_sets_bonuses_and_recomputes() {
        let mut a = Actor {
            var_byte_o: 4,
            var_short_s: 30,
            var_short_x: 20,
            var_short_t: 25,
            ..Actor::default()
        };
        a.recompute_to_full();
        // equip: m[5]=1 (non-consumable), m[11]=8 (O bonus), m[3]=5 (J bonus),
        // m[6]=12 (K resist).
        let mut m = vec![0i32; 14];
        m[5] = 1;
        m[3] = 5;
        m[6] = 12;
        m[11] = 8;
        a.apply_modifier(&m, false);
        assert_eq!(a.j_bonus, 5);
        assert_eq!(a.o_bonus, 8);
        assert_eq!(a.k_bonus, 12);
        assert_eq!(a.p_bonus, 1); // P = m[5]
                                  // health recomputed with O=8: 4*4 + (30+8)*2 + 20*2 + 0 = 16+76+40 = 132
        assert_eq!(a.var_short_o, 132);
        // fatigue recomputed full with J=5: 4*4 + 25*2 + 5 = 16+50+5 = 71
        assert_eq!(a.var_short_p, 71);
        assert_eq!(a.var_short_r, 71);
    }

    #[test]
    fn potion_restores_health_clamped_to_max() {
        let mut a = Actor {
            var_short_o: 100,
            var_short_q: 90,
            var_short_p: 80,
            var_short_r: 50,
            ..Actor::default()
        };
        // consumable (m[5]=0) restore: m[2]=health restore 30, m[3]=fatigue 20.
        let mut m = vec![0i32; 14];
        m[2] = 30;
        m[3] = 20;
        a.apply_modifier(&m, true);
        assert_eq!(a.var_short_q, 100); // 90+30 clamped to 100
        assert_eq!(a.var_short_r, 70); // 50+20
                                       // m[2]>0 queues the over-time effect
        assert!(a.queued_health.is_some());
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
