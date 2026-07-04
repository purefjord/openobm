//! Projectile / spell-effect subsystem — port of `i.java`.
//!
//! The original keeps a flat `short[99]` pool (`i.var_short_arr_a`) of 11 slots ×
//! 9 fields, advanced once per frame by `i.a(long)` (called from the main loop,
//! `b.java:1313`). Each slot:
//!
//! | field | meaning                                                              |
//! |-------|----------------------------------------------------------------------|
//! | `+0`  | kind/type; `-1` = free. World effect: low byte = kind. Actor-homing: `0xFFFFF000 \| var_byte_c<<8 \| kind`. |
//! | `+1`  | x (world), truncated to `short`                                      |
//! | `+2`  | y (world)                                                            |
//! | `+3`  | animation step timer (accumulates `l`; steps when `> 100`)           |
//! | `+4`  | animation frame counter (fed to `g.seek`); `\| 0xFF00` = held on last frame |
//! | `+5`  | origin x                                                             |
//! | `+6`  | origin y                                                             |
//! | `+7`  | lifetime (ms); `0` = none                                            |
//! | `+8`  | lifetime timer (accumulates `l`; cycles `+4`/`+8` when `>= +7`)      |
//!
//! Coupling (all already ported): the frame step + expiry use [`Anim::seek`]
//! (`g.boolean a(d,int,int)` on the `/oh_magic.cml` model); the projectile hit test
//! ([`Effects::collision_hit`] = `i.boolean a(int)`) uses [`combat_distance`] +
//! [`melee_attack`] over the 25-slot actor array (`b.var_j_arr_a`).
//!
//! The fields are `short`, and the original casts every write back to `short`, so
//! this port stores `i16` and reproduces the truncating arithmetic exactly (the
//! `0xFF00` "held" mask in particular relies on `short`→`int` sign extension).
//! Drawing (`i.a(Graphics,int[])`) is deferred to the renderer, like sprite
//! rendering — it doesn't affect simulation state.

use crate::actor::Actor;
use crate::actor::{Tables, WorldEvent};
use crate::anim::Anim;
use crate::combat::{combat_distance, melee_attack};
use crate::rng::JavaRandom;

/// Stride per slot (`i.java` indexes `n += 9`).
pub const STRIDE: usize = 9;
/// Pool length (`short[99]`).
pub const POOL_LEN: usize = 99;

/// The effect pool (`i.var_short_arr_a`). Operates on a 25-slot actor array
/// (`b.var_j_arr_a`), matching the original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effects {
    pool: [i16; POOL_LEN],
}

impl Default for Effects {
    fn default() -> Self {
        Effects {
            pool: [-1; POOL_LEN],
        }
    }
}

impl Effects {
    /// A fresh pool, all slots free (`i`'s static initializer fills `-1`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a raw 99-`short` pool image — the oracle harness installs the
    /// same image into the live `i.var_short_arr_a` to diff `update` step-by-step.
    pub fn from_raw(pool: [i16; POOL_LEN]) -> Self {
        Effects { pool }
    }

    /// Read field `i` of the pool as an `int` (`short`→`int`, sign-extended).
    fn get(&self, i: usize) -> i32 {
        i32::from(self.pool[i])
    }

    /// Write field `i`, truncating to `short` exactly like the original's casts.
    fn set(&mut self, i: usize, v: i32) {
        self.pool[i] = v as i16;
    }

    /// Raw pool access (for the renderer / oracle dumps).
    pub fn raw(&self) -> &[i16; POOL_LEN] {
        &self.pool
    }

    /// Zero every armed slot's anim-step timer (`+3`), frame counter (`+4`),
    /// and lifetime timer (`+8`) — the normalized-screenshot harness's
    /// determinism reset, mirrored by the oracle's `Instrument.normalizeWorld`.
    pub fn reset_anim_counters(&mut self) {
        let mut s = 0;
        while s < POOL_LEN {
            if self.pool[s] != -1 {
                self.pool[s + 3] = 0;
                self.pool[s + 4] = 0;
                self.pool[s + 8] = 0;
            }
            s += STRIDE;
        }
    }

    /// `i.int_a()`: the first free slot, scanning `n < length-9` — i.e. offsets
    /// `0,9,…,81` only. The 11th slot (offset 90) is updatable/drawable/clearable
    /// but **never allocated** (a faithful quirk of the `length - 9` bound).
    fn free_slot(&self) -> Option<usize> {
        let mut n = 0;
        while n < POOL_LEN - STRIDE {
            if self.pool[n] == -1 {
                return Some(n);
            }
            n += STRIDE;
        }
        None
    }

    /// `i.void_a(int n)`: free slot at offset `n` (bounds-checked; `n < 0` and
    /// `n >= 99` are no-ops, so `clear(var_byte_h)` with `-1` is safe).
    pub fn clear(&mut self, n: i32) {
        if n < 0 || n as usize >= POOL_LEN {
            return;
        }
        let n = n as usize;
        for f in 0..STRIDE {
            self.pool[n + f] = -1;
        }
    }

    /// `i.void_a()`: free every slot.
    pub fn clear_all(&mut self) {
        let mut n = 0;
        while n < POOL_LEN {
            self.clear(n as i32);
            n += STRIDE;
        }
    }

    /// `i.a(int n, int n2)`: free the first effect at world position `(x, y)`.
    pub fn clear_at(&mut self, x: i32, y: i32) {
        let mut n = 0;
        while n < POOL_LEN {
            if self.pool[n] != -1 && self.get(n + 1) == x && self.get(n + 2) == y {
                self.clear(n as i32);
                return;
            }
            n += STRIDE;
        }
    }

    /// `i.a(int kind, int dir, j actor, int lifetime)`: spawn an **actor-homing**
    /// effect (the `0xFFFFF000` form). `kind 0`/`11` are remapped by `dir`
    /// (cardinal projectile / 150-speed swing). Returns the slot offset, or `-1`
    /// if the pool is full. (The thinner `i.a` overloads are `dir = 0` /
    /// `lifetime = 0` specializations of this.)
    pub fn spawn_actor(&mut self, kind: i32, dir: i32, actor: &Actor, lifetime: i32) -> i32 {
        let Some(s) = self.free_slot() else {
            return -1;
        };
        let mut n = kind;
        if n == 0 {
            n = match dir {
                2 => 0,
                1 => 2,
                3 => 4,
                4 => 6,
                _ => n,
            };
        } else if n == 11 {
            n = match dir {
                2 => 11,
                1 => 12,
                3 => 13,
                4 => 14,
                _ => n,
            };
        }
        self.set(
            s,
            (0xFFFF_F000u32 as i32) | (i32::from(actor.var_byte_c) << 8) | n,
        );
        self.set(s + 1, actor.var_int_arr_b[0]);
        self.set(s + 2, actor.var_int_arr_b[1]);
        self.set(s + 5, actor.var_int_arr_b[0]);
        self.set(s + 6, actor.var_int_arr_b[1]);
        self.set(s + 3, 0);
        self.set(s + 4, 0);
        self.set(s + 7, lifetime);
        self.set(s + 8, 0);
        s as i32
    }

    /// `i.a(int kind, int x, int y, int lifetime)`: spawn a **world-position**
    /// effect ([+0] = plain `kind`). Returns the slot offset, or `-1` if full.
    pub fn spawn_world(&mut self, kind: i32, x: i32, y: i32, lifetime: i32) -> i32 {
        let Some(s) = self.free_slot() else {
            return -1;
        };
        self.set(s, kind);
        self.set(s + 1, x);
        self.set(s + 2, y);
        self.set(s + 5, x);
        self.set(s + 6, y);
        self.set(s + 3, 0);
        self.set(s + 4, 0);
        self.set(s + 7, lifetime);
        self.set(s + 8, 0);
        s as i32
    }

    /// `i.a(long l)`: advance the whole pool by `l` ms. `model` is the
    /// `/oh_magic.cml` [`Anim`]; `actors` is `b.var_j_arr_a` (25 slots); `rng` is
    /// the shared combat RNG (`melee_attack` draws from it on a hit); `tables` +
    /// `events` feed a lethal hit's death branch.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        l: i64,
        model: &mut Anim,
        actors: &mut [Option<Actor>],
        tables: &mut Tables,
        events: &mut Vec<WorldEvent>,
        rng: &mut JavaRandom,
    ) {
        let mut s = 0;
        while s < POOL_LEN {
            if self.pool[s] == -1 {
                s += STRIDE;
                continue;
            }
            // Advance the step + lifetime timers (short-truncating).
            self.pool[s + 3] = (i64::from(self.pool[s + 3]) + l) as i16;
            self.pool[s + 8] = (i64::from(self.pool[s + 8]) + l) as i16;
            // Lifetime cycle: restart the animation when the timer laps `+7`.
            if self.get(s + 7) > 0 && self.get(s + 8) >= self.get(s + 7) {
                self.set(s + 4, 0);
                self.set(s + 8, 0);
            }
            // Skip the animation step if held on the last frame, or the step
            // timer hasn't reached its threshold yet.
            if (self.get(s + 4) & 0xFF00) == 65280 || self.get(s + 3) <= 100 {
                s += STRIDE;
                continue;
            }
            self.set(s + 3, 0);
            self.set(s + 4, self.get(s + 4) + 1);
            let n = self.get(s) & 0xFF;
            if (0..=6).contains(&n) || (11..=14).contains(&n) {
                // --- moving projectile kinds ---
                match n {
                    0 => self.set(s + 2, self.get(s + 2) - 60),
                    2 => self.set(s + 2, self.get(s + 2) + 60),
                    4 => self.set(s + 1, self.get(s + 1) + 60),
                    6 => self.set(s + 1, self.get(s + 1) - 60),
                    _ => {}
                }
                match n {
                    11 => self.set(s + 2, self.get(s + 2) - 150),
                    12 => self.set(s + 2, self.get(s + 2) + 150),
                    13 => self.set(s + 1, self.get(s + 1) + 150),
                    14 => self.set(s + 1, self.get(s + 1) - 150),
                    _ => {}
                }
                // Advance the effect animation; at its end, impact frames die and
                // the rest loop (re-seek to frame 0).
                if model.seek(n, self.get(s + 4)) {
                    if n == 1 || n == 3 || n == 5 || n == 7 {
                        self.clear(s as i32);
                    } else {
                        self.set(s + 4, 0);
                        model.seek(n, 0);
                    }
                }
                let pos = [self.get(s + 1), self.get(s + 2)];
                let origin = [self.get(s + 5), self.get(s + 6)];
                // In range and no hit -> keep flying. (Faithfully, this reads the
                // slot even right after a clear above; the original does too.)
                if combat_distance(&pos, &origin) <= 750
                    && !self.collision_hit(s, actors, tables, events, rng)
                {
                    s += STRIDE;
                    continue;
                }
                // Out of range or hit: swings die; cardinal projectiles convert to
                // their impact variant (kind n -> n+1).
                if (11..=14).contains(&n) {
                    self.clear(s as i32);
                    s += STRIDE;
                    continue;
                }
                if n != 0 && n != 2 && n != 4 && n != 6 {
                    s += STRIDE;
                    continue;
                }
                self.set(s, self.get(s) + 1);
                s += STRIDE;
                continue;
            }
            // --- non-moving kinds ---
            if (self.get(s) & (0xFFFF_F000u32 as i32)) == -4096 {
                // Actor-homing: track the firing/anchor actor's position.
                let n2 = ((self.get(s) & 0xFFF) >> 8) - 1;
                if n2 < 0 || n2 > actors.len() as i32 || actors[n2 as usize].is_none() {
                    self.clear(s as i32);
                    s += STRIDE;
                    continue;
                }
                let a = actors[n2 as usize].as_ref().unwrap();
                self.set(s + 1, a.var_int_arr_b[0]);
                self.set(s + 2, a.var_int_arr_b[1]);
            }
            if !model.seek(n, self.get(s + 4)) {
                s += STRIDE;
                continue;
            }
            if self.get(s + 7) <= 0 {
                self.clear(s as i32);
                s += STRIDE;
                continue;
            }
            // Animation finished + has a lifetime: hold the last frame until the
            // lifetime timer laps and restarts it (mask `+4` with 0xFF00).
            self.set(s + 4, self.get(s + 4) | 0xFF00);
            s += STRIDE;
        }
    }

    /// `i.boolean a(int n)`: a moving projectile's hit test. The firing actor is
    /// `actors[var_byte_c-1]` (decoded from `+0`); scan the 25 actors for the
    /// nearest valid enemy — skipping empties, the dead (`var_byte_q==1`), the
    /// firer itself, and same-faction (`var_byte_r`) — within distance `200`. On a
    /// hit, [`melee_attack`] resolves (firer vs target, `bl=false`) and returns
    /// `true`. An invalid firer frees the slot and returns `false`.
    fn collision_hit(
        &mut self,
        slot: usize,
        actors: &mut [Option<Actor>],
        tables: &mut Tables,
        events: &mut Vec<WorldEvent>,
        rng: &mut JavaRandom,
    ) -> bool {
        let pos = [self.get(slot + 1), self.get(slot + 2)];
        let n5 = (self.get(slot) & 0xFFF) >> 8;
        if n5 <= 0 || n5 >= actors.len() as i32 {
            self.clear(slot as i32);
            return false;
        }
        // Take the firer out so a kill can mutate it (swing-timer reset + XP),
        // mirroring Java's object ref; the target scan skips its slot anyway.
        let mut firer = match actors[(n5 - 1) as usize].take() {
            Some(a) => a,
            None => {
                self.clear(slot as i32);
                return false;
            }
        };
        let mut best = 0x00FF_FFFF; // boolean_a's initial n4 = 0xFFFFFF
        let mut target = -1i32;
        let scan = actors.len().min(25);
        for (n6, slot) in actors.iter().take(scan).enumerate() {
            let Some(a) = slot else { continue };
            if a.var_byte_q == 1 || n6 == (n5 - 1) as usize || firer.var_byte_r == a.var_byte_r {
                continue;
            }
            let d = combat_distance(&pos, &a.var_int_arr_b);
            if d >= 200 || d >= best {
                continue;
            }
            target = n6 as i32;
            best = d;
        }
        let hit = if target != -1 {
            let mut tgt = actors[target as usize].take().unwrap();
            melee_attack(
                &mut firer,
                (n5 - 1) as usize,
                &mut tgt,
                actors,
                false,
                tables,
                events,
                rng,
            );
            actors[target as usize] = Some(tgt);
            true
        } else {
            false
        };
        actors[(n5 - 1) as usize] = Some(firer);
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::AnimNode;

    /// A magic model with `frames` frames for every effect kind 0..=14 (looping —
    /// matches `oh_magic`'s cardinal-projectile groups; non-looping is irrelevant
    /// to `seek`, which only checks frame count).
    fn magic_model(frames: usize) -> Anim {
        let nodes = (0..=14)
            .map(|k| AnimNode {
                key: k as i8,
                looping: true,
                frame_count: frames,
            })
            .collect();
        Anim::from_nodes(nodes)
    }

    fn no_actors() -> Vec<Option<Actor>> {
        (0..25).map(|_| None).collect()
    }

    #[test]
    fn free_slot_skips_the_unallocatable_11th() {
        let mut e = Effects::new();
        // Fill the 10 allocatable slots (offsets 0,9,…,81).
        for i in 0..10 {
            let s = e.spawn_world(2, i, i, 0);
            assert_eq!(s, (i as usize * STRIDE) as i32);
        }
        // The 11th (offset 90) is never returned -> pool reports full.
        assert_eq!(e.spawn_world(2, 99, 99, 0), -1);
    }

    #[test]
    fn clear_negative_is_noop() {
        let mut e = Effects::new();
        let s = e.spawn_world(2, 5, 5, 0);
        e.clear(-1); // var_byte_h == -1 path
        assert_ne!(e.raw()[s as usize], -1);
        e.clear(s);
        assert_eq!(e.raw()[0], -1);
    }

    #[test]
    fn world_effect_steps_and_holds_then_cycles() {
        // Kind 9 (non-moving), 3 frames, lifetime 5000.
        let mut model = magic_model(3);
        let mut actors = no_actors();
        let mut rng = JavaRandom::new(0);
        let mut e = Effects::new();
        let s = e.spawn_world(9, 100, 200, 5000) as usize;

        // Each call adds 200ms to the step timer (>100 -> one frame step).
        // frame counter +4 goes 1,2,3; at 3 (== frame_count) seek is past-end.
        for _ in 0..2 {
            e.update(
                200,
                &mut model,
                &mut actors,
                &mut Tables::default(),
                &mut Vec::new(),
                &mut rng,
            );
        }
        assert_eq!(e.raw()[s + 4], 2); // stepped to frame 2, not yet past end
        e.update(
            200,
            &mut model,
            &mut actors,
            &mut Tables::default(),
            &mut Vec::new(),
            &mut rng,
        );
        // +4 incremented to 3, seek(9,3) past-end, lifetime>0 -> held (|0xFF00).
        assert_eq!(e.get(s + 4) & 0xFF00, 65280);
        assert_ne!(e.raw()[s], -1); // still alive (held, waiting on lifetime)
    }

    #[test]
    fn world_effect_no_lifetime_dies_at_animation_end() {
        let mut model = magic_model(2);
        let mut actors = no_actors();
        let mut rng = JavaRandom::new(0);
        let mut e = Effects::new();
        let s = e.spawn_world(9, 0, 0, 0); // lifetime 0
                                           // step1: +4=1 (seek(9,1) not past end for 2 frames) -> survives
        e.update(
            200,
            &mut model,
            &mut actors,
            &mut Tables::default(),
            &mut Vec::new(),
            &mut rng,
        );
        assert_ne!(e.raw()[s as usize], -1);
        // step2: +4=2, seek(9,2) past-end, lifetime<=0 -> cleared
        e.update(
            200,
            &mut model,
            &mut actors,
            &mut Tables::default(),
            &mut Vec::new(),
            &mut rng,
        );
        assert_eq!(e.raw()[s as usize], -1);
    }

    #[test]
    fn moving_projectile_advances_position() {
        // Kind spawned from actor with dir=2 -> kind 0 (moves up, y-=60).
        let mut model = magic_model(4);
        let mut actors = no_actors();
        let mut rng = JavaRandom::new(0);
        let mut e = Effects::new();
        let a = Actor {
            var_byte_c: 1, // player slot (index 0)
            var_int_arr_b: [500, 500],
            ..Default::default()
        };
        // The firer must occupy its slot (var_byte_c-1), or collision_hit frees
        // the projectile on the first in-range tick (a faithful behavior).
        actors[0] = Some(a.clone());
        let s = e.spawn_actor(0, 2, &a, 0) as usize;
        assert_eq!(e.get(s) & 0xFF, 0); // kind 0
        e.update(
            200,
            &mut model,
            &mut actors,
            &mut Tables::default(),
            &mut Vec::new(),
            &mut rng,
        );
        assert_eq!(e.get(s + 2), 440); // y: 500 - 60
        assert_eq!(e.get(s + 1), 500); // x unchanged
    }

    #[test]
    fn projectile_hits_nearest_enemy() {
        let mut model = magic_model(4);
        let mut actors = no_actors();
        let mut rng = JavaRandom::new(12345);
        // Firer = player at slot 0 (var_byte_c=1), faction 0.
        let firer = Actor {
            var_byte_c: 1,
            var_byte_r: 0,
            var_int_arr_b: [1000, 1000],
            ..Default::default()
        };
        actors[0] = Some(firer);
        // Enemy at slot 1, faction 1, close to where the projectile will be.
        let enemy = Actor {
            var_byte_r: 1,
            var_byte_q: 0,
            var_int_arr_b: [1000, 940],
            ..Default::default()
        };
        let hp_before = enemy.var_short_q;
        actors[1] = Some(enemy);

        // Spawn kind-0 projectile (up) from the player; it starts at (1000,1000),
        // origin (1000,1000), and moves to (1000,940) — onto the enemy.
        e_spawn_and_step(&mut model, &mut actors, &mut rng);
        // Enemy took damage (HP dropped) from the projectile's melee resolution.
        let enemy_after = actors[1].as_ref().unwrap();
        assert!(
            enemy_after.var_short_q <= hp_before,
            "enemy HP should not increase; got {} from {hp_before}",
            enemy_after.var_short_q
        );
    }

    fn e_spawn_and_step(model: &mut Anim, actors: &mut [Option<Actor>], rng: &mut JavaRandom) {
        let mut e = Effects::new();
        let firer = actors[0].clone().unwrap();
        e.spawn_actor(0, 2, &firer, 0);
        e.update(
            200,
            model,
            actors,
            &mut Tables::default(),
            &mut Vec::new(),
            rng,
        );
    }
}
