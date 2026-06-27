//! Melee combat damage resolution — `h.java::a(j,j,boolean)` (the attack entry)
//! and `h.java::a(int,j,j,bool,bool)` (damage application), the survivable path.
//!
//! Faithful transcription, validated against the **real `h.a` bytecode** with a
//! deterministically seeded `java.util.Random` (see `oracle/Instrument.dumpCombat`
//! and `oracle_match::combat_matches_oracle`). Java's operator precedence is
//! preserved verbatim: `>>` binds *looser* than `+`, so `s + O + i >> 1` is
//! `(s + O + i) >> 1` and `v + z + L >> 3` is `(v + z + L) >> 3`.
//!
//! **Scope (this slice):** the melee path on a *survivable* target. Three coupled
//! branches in the original are intentionally out of scope because they reach
//! global/UI/animation state, not pure math:
//!  - the **spell/cast** path (`var_byte_c != 1 && (weapon != null || t == 1) && bl`)
//!    — calls `h.c`/`boolean_c`, which touch the map and projectiles;
//!  - the attacker's **E-update** on a hit against a *non-player* target
//!    (`target.var_byte_c != 1`) — needs `var_int_arr_b` + `h.a(int[],int[])`;
//!  - the **death** branch (`var_short_q <= 0`) — XP/level-up (`h.c`), death
//!    animation (`h.e`), sound, effects.
//!
//! `melee_attack` debug-asserts it is not invoked in those configurations.

use crate::{Actor, JavaRandom};

/// The resolved category of an attack (matches the oracle's outcome code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatOutcome {
    /// Damage absorbed by armor (`n5 <= 0`), no dodge/block. Code 0.
    Miss = 0,
    /// Blocked (`n7 <= n3`). Code 1.
    Block = 1,
    /// Dodged (`n6 <= n2`). Code 2.
    Dodge = 2,
    /// Landed; `var_short_q` reduced by the post-armor damage. Code 3.
    Hit = 3,
}

/// `h.a(j j2, j j3, boolean bl)` — `attacker` strikes `target` (melee path only).
/// Mutates `target` (HP, dead flag, aggressor back-ref) and advances `rng`
/// exactly as the original. Returns `(died, outcome)`.
pub fn melee_attack(
    attacker: &Actor,
    target: &mut Actor,
    bl: bool,
    rng: &mut JavaRandom,
) -> (bool, CombatOutcome) {
    debug_assert!(
        attacker.var_byte_c == 1
            || (attacker.var_int_arr_l.is_none() && attacker.var_byte_t != 1)
            || !bl,
        "spell/cast path is out of scope for melee_attack"
    );

    // Base melee damage: ((str + O + i) >> 1) + K + N, scaled by weapon-skill D%.
    let mut n = ((i32::from(attacker.var_short_s)
        + i32::from(attacker.o_bonus)
        + i32::from(attacker.var_byte_i))
        >> 1)
        + i32::from(attacker.k_bonus)
        + i32::from(attacker.n_bonus);
    let mut n2 = rng.next_int() % 16;
    let mut crit = false;
    n *= i32::from(attacker.prog_d);
    n /= 100;

    // Equipped weapon overrides the base damage with a per-level-tier value.
    if let Some(w) = &attacker.var_int_arr_l {
        n = if i32::from(attacker.var_byte_o) >= w[10] {
            w[5]
        } else if i32::from(attacker.var_byte_o) >= w[9] {
            w[4]
        } else {
            w[3]
        };
        if attacker.var_byte_c != 1 && w[2] != 4 {
            n >>= 1;
        }
    }

    // Critical hit (1-in-16): at least quarter of target HP, or double damage.
    n2 = n2.abs();
    if n2 == 1 {
        n = (i32::from(target.var_short_q) >> 2).max(n << 1);
        crit = true;
    }

    // The original call is `h.a(n, j3, j2, bl2, false)`: the crit flag goes into
    // h.a's `bl` (the damage-text prefix), and h.a's `bl2` (defense bypass) is the
    // literal `false`. So in melee, dodge/block/armor ALWAYS apply — even on crits.
    let _ = bl; // a(j,j,bool)'s own `bl` only gates the (out-of-scope) spell path.
    apply_damage(n, target, attacker, crit, false, rng)
}

/// `h.a(int n, j j2, j j3, boolean bl, boolean bl2)` — apply `n` damage to `j2`
/// (here `target`), dealt by `j3` (here `attacker`). Survivable path only.
fn apply_damage(
    n: i32,
    target: &mut Actor,
    attacker: &Actor,
    _bl: bool,
    bl2: bool,
    rng: &mut JavaRandom,
) -> (bool, CombatOutcome) {
    // 1096: an un-attackable target resolves to nothing (returns false).
    if target.var_byte_u == 1 {
        return (false, CombatOutcome::Miss);
    }

    let mut n2 = i32::from(target.prog_a) + (i32::from(target.prog_a) >> 1); // dodge
    let mut n3 = i32::from(target.prog_b) + (i32::from(target.prog_b) >> 1); // block
    let mut n4 = ((i32::from(target.var_short_v)
        + i32::from(target.var_short_z)
        + i32::from(target.l_bonus))
        >> 3)
        + i32::from(target.m_bonus); // armor
    if bl2 {
        n4 = 0;
        n3 = -1000;
        n2 = -1000;
    }
    let n5 = n - n4;
    let mut n6 = rng.next_int() % 100;
    let n7 = rng.next_int() % 100;
    n2 *= i32::from(target.h_field);
    n2 /= 100;
    n6 = n6.abs();
    let n7 = n7.abs();

    // 1114: first aggressor is remembered.
    target.var_j_a_set = true;

    if n6 <= n2 {
        (false, CombatOutcome::Dodge)
    } else if n7 <= n3 {
        (false, CombatOutcome::Block)
    } else if n5 > 0 {
        debug_assert!(
            target.var_byte_c == 1,
            "E-update against a non-player target is out of scope"
        );
        // 1127: a non-creature attacker burns one extra RNG draw on a landed hit.
        if attacker.var_byte_t == 0 {
            rng.next_int();
        }
        target.var_short_q = (i32::from(target.var_short_q) - n5) as i16;
        target.var_byte_q = i8::from(target.var_short_q <= 0);
        debug_assert!(
            target.var_byte_q == 0,
            "death branch (XP/animation/sound) is out of scope"
        );
        (target.var_byte_q == 1, CombatOutcome::Hit)
    } else {
        (false, CombatOutcome::Miss)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A construction guard: an unarmed NPC vs a high-HP player resolves on the
    /// survivable path without tripping the out-of-scope debug-asserts. (Numeric
    /// correctness is established by `oracle_match::combat_matches_oracle`.)
    #[test]
    fn survivable_melee_resolves() {
        let attacker = Actor {
            var_byte_c: 0,
            var_byte_o: 5,
            var_short_s: 40,
            o_bonus: 5,
            var_byte_i: 10,
            k_bonus: 2,
            n_bonus: 3,
            prog_d: 120,
            ..Actor::default()
        };
        for seed in 0..50i64 {
            let mut target = Actor {
                var_byte_c: 1,
                var_short_q: 10_000,
                prog_a: 50,
                prog_b: 40,
                ..Actor::default()
            };
            let mut rng = JavaRandom::new(seed);
            let (died, _outcome) = melee_attack(&attacker, &mut target, true, &mut rng);
            assert!(!died);
            assert!(target.var_short_q <= 10_000);
        }
    }
}
