//! Melee combat damage resolution — `h.java::a(j,j,boolean)` (the attack entry)
//! and `h.java::a(int,j,j,bool,bool)` (damage application), the survivable path.
//!
//! Faithful transcription, validated against the **real `h.a` bytecode** with a
//! deterministically seeded `java.util.Random` (see `oracle/Instrument.dumpCombat`
//! and `oracle_match::combat_matches_oracle`). Java's operator precedence is
//! preserved verbatim: `>>` binds *looser* than `+`, so `s + O + i >> 1` is
//! `(s + O + i) >> 1` and `v + z + L >> 3` is `(v + z + L) >> 3`.
//!
//! **Scope:** the melee path. The **spell/cast** branch (`var_byte_c != 1 &&
//! (weapon != null || t == 1) && bl`) is handled by the caller *before* reaching
//! `melee_attack` ([`Actor::cast`](crate::Actor) from the tick — `melee_attack`
//! debug-asserts it is not invoked in that configuration). The **death** branch
//! (`var_short_q <= 0`, h.a:1134) is ported: the attacker's swing-timer reset +
//! XP award (`h.c`), one RNG draw (the original's death-sound path), the death
//! pose (`var_byte_e = 6` + `h.e`), and the world-layer effects — the death
//! trigger push (`e.void_a(var_byte_k)`) and the loot drop (`e.int_a()` roll +
//! `b.a(item,false,tile)`) — emitted as [`WorldEvent`]s.

use crate::actor::{Tables, WorldEvent};
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

/// `h.java::a(int[], int[])` — the octagonal distance approximation between two
/// world positions (`var_int_arr_b`). Pure; used by targeting (`h.j_a`), AI range
/// checks, spell AoE, and the combat E-update. Java precedence preserved:
/// `n2 + 512 >> 10` is `(n2 + 512) >> 10`, and `n3 << 4`.
pub fn combat_distance(a: &[i32], b: &[i32]) -> i32 {
    let n5 = (a[0] - b[0]).abs();
    let n6 = (a[1] - b[1]).abs();
    let (n3, n4) = if n5 < n6 { (n5, n6) } else { (n6, n5) }; // (min, max)
    let mut n2 = n4 * 1007 + n3 * 441;
    if n4 < n3 << 4 {
        n2 -= n4 * 40;
    }
    ((n2 + 512) >> 10).abs()
}

/// `h.java::j_a(j)` — pick the nearest valid target for `q` among `actors`
/// (the global `b.var_j_arr_a`): skip empty slots, the dead (`var_byte_q == 1`),
/// same-faction (`var_byte_r`), and same-kind (`var_byte_c`); of the rest, the
/// closest by [`combat_distance`], with the earliest index winning ties (the
/// original keeps the first because a later equal distance is `>= n`). Returns the
/// slot index, or `None`. (`q` is the querying actor, not part of `actors` here;
/// in the game the same-faction/kind skips also prevent self-targeting.)
pub fn nearest_target(actors: &[Option<Actor>], q: &Actor) -> Option<usize> {
    let mut best = 0x00FF_FFFF; // h.j_a's initial `n = 0xFFFFFF`
    let mut result = None;
    for (idx, slot) in actors.iter().enumerate() {
        let Some(a) = slot else { continue };
        if a.var_byte_q == 1 || a.var_byte_r == q.var_byte_r || a.var_byte_c == q.var_byte_c {
            continue;
        }
        let d = combat_distance(&q.var_int_arr_b, &a.var_int_arr_b);
        if d >= best {
            continue;
        }
        best = d;
        result = Some(idx);
    }
    result
}

/// `h.a(j j2, j j3, boolean bl)` — `attacker` strikes `target` (melee path only).
/// Mutates `target` (HP, dead flag, aggressor back-ref) and advances `rng`
/// exactly as the original. Returns `(died, outcome)`.
///
/// `attacker_idx` is the attacker's actor-array slot (stored into the target's
/// `var_j_a` back-ref on a first hit — `h.a:1114`); `actors` resolves a
/// *pre-existing* `var_j_a` aggressor's position for the E-update (`h.a:1125`).
/// Both the attacker's and the target's own slots may be `None` (taken out by
/// the caller, as in [`Actor::tick`]); the attacker is then read from `attacker`.
#[allow(clippy::too_many_arguments)]
pub fn melee_attack(
    attacker: &mut Actor,
    attacker_idx: usize,
    target: &mut Actor,
    actors: &[Option<Actor>],
    bl: bool,
    tables: &mut Tables,
    events: &mut Vec<WorldEvent>,
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
    apply_damage(
        n,
        target,
        attacker,
        attacker_idx,
        actors,
        crit,
        false,
        tables,
        events,
        rng,
    )
}

/// `h.a(int n, j j2, j j3, boolean bl, boolean bl2)` — apply `n` damage to `j2`
/// (here `target`), dealt by `j3` (here `attacker`, at slot `attacker_idx`).
/// Survivable path only. `bl` prefixes the floating damage text (the crit
/// marker, lang id 472).
#[allow(clippy::too_many_arguments)]
fn apply_damage(
    n: i32,
    target: &mut Actor,
    attacker: &mut Actor,
    attacker_idx: usize,
    actors: &[Option<Actor>],
    bl: bool,
    bl2: bool,
    tables: &mut Tables,
    events: &mut Vec<WorldEvent>,
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

    // 1114: the first aggressor is remembered in the var_j_a back-ref.
    if target.var_j_a == -1 {
        target.var_j_a = attacker_idx as i32;
    }

    // The floating damage text (1118/1121/1131): the real strings come from the
    // lang table (`b.a(471)` dodge, `b.a(470)` block, `b.a(472)` crit prefix);
    // only the text's *presence* (and `Q = 0`) drives behavior (the tick fade).
    if n6 <= n2 {
        target.floating_text = Some("<471>".into());
        target.q_field = 0;
        (false, CombatOutcome::Dodge)
    } else if n7 <= n3 {
        target.floating_text = Some("<470>".into());
        target.q_field = 0;
        (false, CombatOutcome::Block)
    } else if n5 > 0 {
        // 1124: a non-player target records the distance to its aggressor (the
        // *current* var_j_a — a pre-existing aggressor keeps precedence over the
        // striker) as alertness (`E`). Java reads the position through the object
        // ref; the index model requires that slot live — except the striker's
        // own (possibly taken-out) slot, read from `attacker`.
        if target.var_byte_c != 1 && target.var_j_a != -1 && !bl2 {
            let ja = target.var_j_a as usize;
            let agg_pos = if ja == attacker_idx {
                &attacker.var_int_arr_b
            } else {
                debug_assert!(
                    ja < actors.len() && actors[ja].is_some(),
                    "pre-existing var_j_a aggressor must be a live actor in the array"
                );
                &actors[ja].as_ref().unwrap().var_int_arr_b
            };
            let d = combat_distance(&target.var_int_arr_b, agg_pos);
            target.e_field = d.max(i32::from(target.e_field)) as i16;
        }
        // 1127: a non-creature attacker burns one extra RNG draw on a landed hit.
        if attacker.var_byte_t == 0 {
            rng.next_int();
        }
        target.var_short_q = (i32::from(target.var_short_q) - n5) as i16;
        target.floating_text = Some(if bl {
            format!("<472>{n5}")
        } else {
            n5.to_string()
        });
        target.q_field = 0;
        target.var_byte_q = i8::from(target.var_short_q <= 0);
        if target.var_byte_q == 1 {
            // h.a:1134 — the death branch. The attacker (`j3`, non-null on every
            // path here) resets its swing timer and takes the XP award (a no-op
            // for non-players inside h.c); one RNG draw (the original's
            // death-sound path); the death pose + `h.e` re-pick; then the
            // world-layer effects as deferred events.
            attacker.var_int_a = 0;
            attacker.award_xp(target.var_byte_o.max(0) as usize, tables);
            rng.next_int();
            target.var_byte_e = 6;
            crate::world::pick_primary(target);
            if target.var_byte_k >= 0 {
                events.push(WorldEvent::PushEntry(target.var_byte_k as u8));
            }
            if target.var_byte_s == 1 {
                let item = crate::actor::loot_roll(tables, rng); // e.int_a()
                if item != 0 {
                    events.push(WorldEvent::DropLoot {
                        item,
                        x: i32::from(target.var_byte_arr_c[0]),
                        y: i32::from(target.var_byte_arr_c[1]),
                    });
                }
            }
        }
        (target.var_byte_q == 1, CombatOutcome::Hit)
    } else {
        (false, CombatOutcome::Miss)
    }
}

/// `h.a(n, j2, j3, false, true)` — apply `damage` to `victim` dealt by `dealer`
/// (at slot `dealer_idx`), **bypassing defense** (`bl2 = true`: no
/// dodge/block/armor). This is the damage-over-time application (the
/// `var_short_k` lap in [`crate::Actor::tick`]) and the direct hit of the
/// poison applicator. `dealer` is read for its `var_byte_t` (the extra RNG
/// draw) and mutated on a kill; the on-hit E-update is skipped under `bl2`.
pub fn dot_damage(
    damage: i32,
    victim: &mut Actor,
    dealer: &mut Actor,
    dealer_idx: usize,
    tables: &mut Tables,
    events: &mut Vec<WorldEvent>,
    rng: &mut JavaRandom,
) -> (bool, CombatOutcome) {
    apply_damage(
        damage,
        victim,
        dealer,
        dealer_idx,
        &[],
        false,
        true,
        tables,
        events,
        rng,
    )
}

/// `h.a(j j2, j j3, int n, int n2)` — the poison applicator: `dealer` (at slot
/// `dealer_idx`) poisons `victim` with `damage` per lap for `duration` ms. Sets
/// the DoT fields (`var_byte_x`/`var_short_k`/`var_j_b`/`var_byte_w`), spawns
/// the poison puff (`i.a(8, j3)`), and applies one immediate defense-bypassing
/// hit. Called per-victim by the spell AoE (weapon type 4 / the type-2
/// fallthrough) and by scripts.
#[allow(clippy::too_many_arguments)]
pub fn apply_poison(
    dealer: &mut Actor,
    dealer_idx: usize,
    victim: &mut Actor,
    damage: i32,
    duration: i32,
    effects: &mut crate::effects::Effects,
    tables: &mut Tables,
    events: &mut Vec<WorldEvent>,
    rng: &mut JavaRandom,
) -> (bool, CombatOutcome) {
    victim.var_byte_x = damage as i8;
    victim.var_short_k = duration as i16;
    victim.var_j_b = dealer_idx as i32;
    victim.var_byte_w = -47;
    effects.spawn_actor(8, 0, victim, 0);
    apply_damage(
        damage,
        victim,
        dealer,
        dealer_idx,
        &[],
        false,
        true,
        tables,
        events,
        rng,
    )
}

/// `h.a(j j2, j j3, int n)` — direct spell damage: an impact effect
/// (`i.a(10, j3)`) then `damage` applied **with** full defenses
/// (dodge/block/armor; `bl2 = false`, so the non-player E-update runs — hence
/// `actors`). Called per-victim by the spell AoE (weapon row `[1] == 61618`).
#[allow(clippy::too_many_arguments)]
pub fn apply_spell_damage(
    dealer: &mut Actor,
    dealer_idx: usize,
    victim: &mut Actor,
    actors: &[Option<Actor>],
    damage: i32,
    effects: &mut crate::effects::Effects,
    tables: &mut Tables,
    events: &mut Vec<WorldEvent>,
    rng: &mut JavaRandom,
) -> (bool, CombatOutcome) {
    effects.spawn_actor(10, 0, victim, 0);
    apply_damage(
        damage, victim, dealer, dealer_idx, actors, false, false, tables, events, rng,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A construction guard: an unarmed NPC vs a high-HP player resolves on the
    /// survivable path without tripping the out-of-scope debug-asserts. (Numeric
    /// correctness is established by `oracle_match::combat_matches_oracle`.)
    #[test]
    fn survivable_melee_resolves() {
        let mut attacker = Actor {
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
            let mut tables = Tables::default();
            let mut events = Vec::new();
            let (died, _outcome) = melee_attack(
                &mut attacker,
                0,
                &mut target,
                &[],
                true,
                &mut tables,
                &mut events,
                &mut rng,
            );
            assert!(!died);
            assert!(events.is_empty());
            assert!(target.var_short_q <= 10_000);
        }
    }

    /// The death branch: a lethal hit sets the death pose, resets the attacker's
    /// swing timer, draws the death-path RNG, and emits the trigger/loot events.
    #[test]
    fn death_branch_emits_world_events() {
        let mut attacker = Actor {
            var_byte_c: 1, // player: takes the XP award
            var_byte_o: 1,
            var_short_s: 200,
            prog_d: 100,
            var_int_a: 555,
            ..Actor::default()
        };
        let mut tables = Tables::default();
        // A subtype-10 loot list: row 1 = item 7, modulus 1 (always), count 2.
        tables.insert(10, vec![vec![0; 4], vec![0, 7, 1, 2], vec![0; 4]]);
        // Find a seed whose draws (swing, dodge, block) land a lethal hit.
        'seed: for seed in 0..200i64 {
            let mut target = Actor {
                var_byte_c: 0,
                var_short_q: 1,
                var_byte_o: 3,
                var_byte_k: 9, // death-trigger entry
                var_byte_s: 1, // drops loot
                ..Actor::default()
            };
            let mut rng = JavaRandom::new(seed);
            let mut events = Vec::new();
            let (died, outcome) = melee_attack(
                &mut attacker,
                0,
                &mut target,
                &[],
                true,
                &mut tables,
                &mut events,
                &mut rng,
            );
            if outcome != CombatOutcome::Hit {
                continue 'seed;
            }
            assert!(died);
            assert_eq!(target.var_byte_q, 1);
            assert_eq!(target.var_byte_e, 6);
            assert_eq!(attacker.var_int_a, 0);
            assert_eq!(events[0], WorldEvent::PushEntry(9));
            assert!(matches!(events[1], WorldEvent::DropLoot { item: 7, .. }));
            return;
        }
        panic!("no seed produced a lethal hit");
    }
}
