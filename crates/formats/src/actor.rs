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

use crate::anim::Anim;
use crate::effects::Effects;
use crate::rng::JavaRandom;

/// A faithful, scalar subset of `j.java` (the fields the stat math and save
/// touch). Names mirror the decompiled source; types match Java widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    // --- identity / class ---
    pub var_byte_c: i8, // actor id: player = 1, else array slot + 1 (b.java:2547); var_byte_c-1 = slot
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
    pub var_short_y: i16, // luck (incremented on level-up)
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
    /// Damage-over-time per-lap amount (`j.var_byte_x`), applied while
    /// `var_short_k > 0` (set by the poison applicator `h.a(j,j,n,n2)`).
    pub var_byte_x: i8,
    pub var_byte_w: i8,
    /// Queued health-over-time consumable (j.var_int_arr_f).
    pub queued_health: Option<Vec<i32>>,
    /// Queued fatigue-over-time consumable (j.var_int_arr_g).
    pub queued_fatigue: Option<Vec<i32>>,

    pub var_byte_r: i8,
    pub var_int_b: i32,

    // --- class/level progression fields, set by `h.f` (`class_progression`) ---
    pub var_byte_i: i8,   // j.var_byte_i  (= race row[3])
    pub var_short_z: i16, // j.var_short_z (= sum of equipped item row[4])
    pub prog_a: i16,      // j.var_short_A
    pub prog_b: i16,      // j.B
    pub prog_c: i16,      // j.C
    pub prog_d: i16,      // j.D
    /// Inventory: indices into the item table (subtype 1); `-1` = empty slot.
    /// `j.var_int_arr_n` (`new int[8]`, default all 0).
    pub var_int_arr_n: [i32; 8],

    // --- combat fields (read/written by the attack resolution) ---
    pub var_byte_u: i8, // j.var_byte_u  (1 = cannot be attacked)
    pub var_byte_q: i8, // j.var_byte_q  (1 = dead)
    pub var_byte_t: i8, // j.var_byte_t  (creature-class flag)
    pub h_field: i16,   // j.H  (dodge-skill %, default 100)
    /// Equipped weapon row (`j.var_int_arr_l`); `None` = unarmed.
    pub var_int_arr_l: Option<Vec<i32>>,
    /// Whether `j.var_j_a` (the last aggressor back-ref) has been set non-null.
    pub var_j_a_set: bool,
    /// The DoT dealer's actor-array index (`j.var_j_b`, a Java object ref modeled
    /// as a slot index; `-1` = none). Set by the (unported) poison applicator and
    /// read by the `var_short_k` DoT tick to resolve the damage dealer.
    pub var_j_b: i32,
    /// World position `[x, y]` (`j.var_int_arr_b`); only `[0]`/`[1]` feed combat.
    pub var_int_arr_b: [i32; 2],

    // --- movement / map-collision sample state ---
    /// Collision-box corner world positions (`j.var_int_arr_c`/`_d`); their low 7
    /// bits give the sub-tile position used for slope tiles.
    pub var_int_arr_c: [i32; 2],
    pub var_int_arr_d: [i32; 2],
    /// The three sampled tile coordinates `[row, col]` (`j.var_byte_arr_b/c/d`),
    /// i.e. each world corner `>> 7`.
    pub var_byte_arr_b: [i8; 2],
    pub var_byte_arr_c: [i8; 2],
    pub var_byte_arr_d: [i8; 2],
    /// Collision-enabled flag (`j.var_byte_p`, default 1).
    pub var_byte_p: i8,
    /// Previous world position before the last step (`j.var_int_arr_e`), used to
    /// revert a blocked move.
    pub var_int_arr_e: [i32; 2],
    /// Iso/screen-space position (`j.var_int_arr_i`), derived from `var_int_arr_b`.
    pub var_int_arr_i: [i32; 2],
    /// The primary sampled tile (`j.var_byte_arr_a`), chosen by `h.e` for draw order.
    pub var_byte_arr_a: [i8; 2],
    /// Collision-box half-extents (`j.var_byte_a`/`_b`) feeding the corner offsets.
    pub var_byte_a: i8,
    pub var_byte_b: i8,
    /// Facing direction (`j.var_byte_d`, default 2), set when stepping.
    pub var_byte_d: i8,
    /// Move-step accumulator (`j.var_short_g`) and walk-anim timer (`j.var_short_a`).
    pub var_short_g: i16,
    pub var_short_a: i16,

    // --- per-actor tick fields (`h.a(j,long,boolean)`) ---
    /// Animation frame timer (`j.var_short_b`): advances the sprite every >125ms.
    pub var_short_b: i16,
    /// Free-running timer (`j.var_int_a`) and attack-cooldown timer (`j.var_int_e`).
    pub var_int_a: i32,
    pub var_int_e: i32,
    /// Animation state (`j.var_byte_e`); indexes [`ANIM_STATE_OFFSETS`].
    pub var_byte_e: i8,
    /// Health/fatigue regen accumulators (`j.var_short_c`/`j.var_short_e`); tick a
    /// point when they reach the regen rate (`var_short_d`/`var_short_f`).
    pub var_short_c: i16,
    pub var_short_e: i16,
    /// Corpse timer for the dead (`j.var_short_i`).
    pub var_short_i: i16,
    /// A status timer flag (`j.var_byte_y`, default -1) + its countdown (`j.var_short_n`).
    pub var_byte_y: i8,
    pub var_short_n: i16,
    /// Move-to target (`j.var_int_arr_j`, default `[-1,-1]`); `[0] != -1` = moving.
    pub var_int_arr_j: [i32; 2],
    /// Timed buff duration (`j.G`); on expiry it strips the J..P bonus block.
    pub g_field: i16,
    /// Aggression flag (`j.var_byte_z`, default 1); gates the NPC attack AI.
    pub var_byte_z: i8,
    /// The actor's attached effect-pool slot (`j.var_byte_h`, default -1); cleared
    /// when a buff expires.
    pub var_byte_h: i8,

    // --- floating damage text (the rising number over an actor) ---
    /// The text to display (`j.var_java_lang_String_a`); `None` = no active text.
    /// Only its presence drives the tick; the content is set by the combat/UI code.
    pub floating_text: Option<String>,
    /// Fade timer (`j.var_short_h`), rise position (`j.Q`/`j.R`), and the color
    /// (`j.var_int_c`, default 0xFF0000) decremented by `j.var_int_d` each step.
    pub var_short_h: i16,
    pub q_field: i16,
    pub r_field: i16,
    pub var_int_c: i32,
    pub var_int_d: i32,
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
            var_short_y: 0,
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
            var_byte_x: 0,
            var_byte_w: -1,
            queued_health: None,
            queued_fatigue: None,
            var_byte_r: 1,
            var_int_b: 0,
            var_byte_i: 0,
            var_short_z: 0,
            prog_a: 0,
            prog_b: 0,
            prog_c: 100,
            prog_d: 100,
            var_int_arr_n: [0; 8],
            var_byte_u: 0,
            var_byte_q: 0,
            var_byte_t: 0,
            h_field: 100,
            var_int_arr_l: None,
            var_j_a_set: false,
            var_j_b: -1,
            var_int_arr_b: [0; 2],
            var_int_arr_c: [0; 2],
            var_int_arr_d: [0; 2],
            var_byte_arr_b: [0; 2],
            var_byte_arr_c: [0; 2],
            var_byte_arr_d: [0; 2],
            var_byte_p: 1,
            var_int_arr_e: [0; 2],
            var_int_arr_i: [0; 2],
            var_byte_arr_a: [0; 2],
            var_byte_a: 0,
            var_byte_b: 0,
            var_byte_d: 2,
            var_short_g: 0,
            var_short_a: 0,
            var_short_b: 0,
            var_int_a: 0,
            var_int_e: 0,
            var_byte_e: 0,
            var_short_c: 0,
            var_short_e: 0,
            var_short_i: 0,
            var_byte_y: -1,
            var_short_n: 0,
            var_int_arr_j: [-1, -1],
            g_field: 0,
            var_byte_z: 1,
            var_byte_h: -1,
            floating_text: None,
            var_short_h: 0,
            q_field: 0,
            r_field: 0,
            var_int_c: 0xFF_0000,
            var_int_d: 0,
        }
    }
}

/// `h.var_byte_arr_a`: the per-state animation-group offsets. The actor's current
/// group is `var_byte_d` (facing) `+ ANIM_STATE_OFFSETS[var_byte_e]` (state).
pub const ANIM_STATE_OFFSETS: [i8; 8] = [0, 4, 8, 12, 16, 20, 24, 25];

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

/// The `.scr` stat-table store (`e`'s `int[][]` tables, keyed by subtype). `h.f`
/// consumes subtype 4 (race/spec rows, `e.c`, 8 cols) and subtype 1 (item rows,
/// `e.d`, 10 cols). Built from a parsed/accumulated `.scr` program or from the
/// oracle's live-store dump.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tables {
    by_subtype: std::collections::BTreeMap<u8, Vec<Vec<i32>>>,
}

impl Tables {
    pub fn insert(&mut self, subtype: u8, rows: Vec<Vec<i32>>) {
        self.by_subtype.insert(subtype, rows);
    }

    /// Rows of a subtype's table (empty slice if the subtype is absent).
    pub fn rows(&self, subtype: u8) -> &[Vec<i32>] {
        self.by_subtype
            .get(&subtype)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// A single row, mirroring `e.int_arr_a(subtype, idx)` for in-range indices
    /// (the only case `h.f` ever hits; the original's out-of-range fall-through
    /// is deliberately *not* modeled — `h.f` indexes are always valid).
    pub fn row(&self, subtype: u8, idx: i32) -> Option<&[i32]> {
        if idx < 0 {
            return None;
        }
        self.by_subtype
            .get(&subtype)
            .and_then(|t| t.get(idx as usize))
            .map(Vec::as_slice)
    }
}

impl Actor {
    /// `h.java::f(j)` — the class/level/race progression table. Sets `var_byte_i`
    /// from the race row, `var_short_z` from the equipped-item sum, then the
    /// class-specific skill fields (`prog_a/b/c/d` = `j.var_short_A/B/C/D`) by
    /// level breakpoint. Transcribed verbatim from the decompiled switch,
    /// **including** its redundant double-writes (e.g. classes 5/8 set `C`
    /// twice) — compatibility-first: do not "simplify" these.
    ///
    /// The original's `break`/`return` are equivalent here (the switch is the last
    /// statement in `f`), so each becomes an early `return` from the per-class
    /// helper. Panics if the race/item row is absent, mirroring the original NPE.
    pub fn class_progression(&mut self, tables: &Tables) {
        let race: Vec<i32> = tables
            .row(4, i32::from(self.var_byte_j))
            .expect("h.f: race row (subtype 4) must be loaded")
            .to_vec();
        self.var_byte_i = race[3] as i8;
        self.var_short_z = 0;
        for k in 0..self.var_int_arr_n.len() {
            let item_idx = self.var_int_arr_n[k];
            if item_idx == -1 {
                continue;
            }
            let item = tables
                .row(1, item_idx)
                .expect("h.f: item row (subtype 1) must be loaded");
            self.var_short_z = (i32::from(self.var_short_z) + item[4]) as i16;
        }
        match self.var_byte_f {
            4 => self.hf_class4(&race),
            3 => self.hf_class3(&race),
            8 => self.hf_class8(&race),
            5 => self.hf_class5(&race),
            1 => self.hf_class1(&race),
            2 => self.hf_class2(&race),
            7 => self.hf_class7(&race),
            6 => self.hf_class6(&race),
            _ => {}
        }
    }

    fn hf_class4(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o == 20 {
            self.prog_c = 130;
        }
        if race[2] == 1 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] != 4 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 7 {
            self.prog_d = 110;
        }
        if o != 16 {
            return;
        }
        self.prog_d = 125;
    }

    fn hf_class3(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_b = 0;
        }
        if o == 5 {
            self.prog_b = 3;
        }
        if o == 17 {
            self.prog_b = 10;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o == 20 {
            self.prog_c = 130;
        }
        if race[2] == 1 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] == 2 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] == 3 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 6 {
                self.prog_d = 110;
            }
            if o != 15 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if i32::from(self.var_byte_j) == 0 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 5 {
                self.prog_d = 110;
            }
            if o != 15 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] != 0 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 8 {
            self.prog_d = 110;
        }
        if o != 18 {
            return;
        }
        self.prog_d = 125;
    }

    fn hf_class8(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o == 20 {
            self.prog_c = 130;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 7 {
            self.prog_c = 115;
        }
        if o == 17 {
            self.prog_c = 130;
        }
        if race[2] == 1 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] == 2 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] != 0 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 8 {
            self.prog_d = 110;
        }
        if o != 18 {
            return;
        }
        self.prog_d = 125;
    }

    fn hf_class5(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_b = 0;
        }
        if o == 5 {
            self.prog_b = 3;
        }
        if o == 17 {
            self.prog_b = 10;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o == 20 {
            self.prog_c = 130;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 7 {
            self.prog_c = 115;
        }
        if o == 17 {
            self.prog_c = 130;
        }
        if race[2] == 1 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] == 2 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] != 0 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 8 {
            self.prog_d = 110;
        }
        if o != 18 {
            return;
        }
        self.prog_d = 125;
    }

    fn hf_class1(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_a = 5;
        }
        if o == 7 {
            self.prog_a = 10;
        }
        if o == 15 {
            self.prog_a = 15;
        }
        if race[2] == 4 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 7 {
                self.prog_d = 110;
            }
            if o != 16 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if i32::from(self.var_byte_j) != 0 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 5 {
            self.prog_d = 110;
        }
        if o == 15 {
            self.prog_d = 125;
        }
        if o == 1 {
            self.prog_c = 110;
        }
        if o == 5 {
            self.prog_c = 125;
        }
        if o != 15 {
            return;
        }
        self.prog_c = 140;
    }

    fn hf_class2(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_a = 5;
        }
        if o == 7 {
            self.prog_a = 10;
        }
        if o == 15 {
            self.prog_a = 15;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o == 20 {
            self.prog_c = 130;
        }
        if race[2] == 2 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] != 3 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 6 {
            self.prog_d = 110;
        }
        if o != 15 {
            return;
        }
        self.prog_d = 125;
    }

    fn hf_class7(&mut self, _race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_a = 5;
        }
        if o == 7 {
            self.prog_a = 10;
        }
        if o == 15 {
            self.prog_a = 15;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o != 20 {
            return;
        }
        self.prog_c = 130;
    }

    fn hf_class6(&mut self, race: &[i32]) {
        let o = i32::from(self.var_byte_o);
        if o == 1 {
            self.prog_b = 0;
        }
        if o == 5 {
            self.prog_b = 3;
        }
        if o == 17 {
            self.prog_b = 10;
        }
        if o == 1 {
            self.prog_c = 100;
        }
        if o == 10 {
            self.prog_c = 115;
        }
        if o == 20 {
            self.prog_c = 130;
        }
        if race[2] == 2 {
            if o == 1 {
                self.prog_d = 100;
            }
            if o == 8 {
                self.prog_d = 110;
            }
            if o != 18 {
                return;
            }
            self.prog_d = 125;
            return;
        }
        if race[2] != 4 {
            return;
        }
        if o == 1 {
            self.prog_d = 100;
        }
        if o == 7 {
            self.prog_d = 110;
        }
        if o != 16 {
            return;
        }
        self.prog_d = 125;
    }
}

/// XP required to *reach* each level (`h.var_short_arr_a`, index = level).
pub const XP_THRESHOLD: [i32; 26] = [
    0, 0, 100, 210, 340, 500, 700, 950, 1260, 1640, 2100, 2650, 3300, 4060, 4940, 5950, 7100, 8400,
    9860, 11490, 13300, 15300, 17500, 19910, 22540, 25400,
];
/// XP awarded for killing an enemy of level `n` (`h.var_short_arr_b`).
pub const XP_REWARD: [i32; 26] = [
    0, 10, 12, 15, 19, 24, 30, 37, 45, 54, 64, 75, 87, 100, 114, 129, 145, 162, 180, 199, 219, 240,
    262, 285, 309, 334,
];

impl Actor {
    /// `h.java::c(j, int)` — award XP for a kill (enemy level `n`), and if the
    /// next-level threshold is crossed (player only, level < 25), **level up**:
    /// +1 to all seven attributes, the class level bonus (`h.g`), a health/fatigue
    /// recompute, then the progression pass (`h.f`). One kill levels up at most
    /// once (a single `if`, not a loop). Player-only; the `var_j_d` owner redirect
    /// and the floating-text messages (`b.a`, pure UI) are out of scope.
    pub fn award_xp(&mut self, n: usize, tables: &Tables) {
        if self.var_byte_c != 1 {
            return;
        }
        self.var_int_b += XP_REWARD[n];
        if i32::from(self.var_byte_o) < 25
            && self.var_int_b >= XP_THRESHOLD[(i32::from(self.var_byte_o) + 1) as usize]
        {
            self.var_byte_o += 1;
            self.var_short_s += 1;
            self.var_short_t += 1;
            self.var_short_u += 1;
            self.var_short_v += 1;
            self.var_short_w += 1;
            self.var_short_x += 1;
            self.var_short_y += 1;
            self.level_up_class_bonus();
            self.recompute();
            self.class_progression(tables);
        }
    }

    /// `h.java::g(j)` — a class-specific attribute bonus applied when the *new*
    /// level is exactly 5/10/15/20.
    fn level_up_class_bonus(&mut self) {
        let o = i32::from(self.var_byte_o);
        match self.var_byte_f {
            1 => {
                if o == 5 {
                    self.var_short_w += 25;
                } else if o == 10 {
                    self.var_short_v += 1;
                } else if o == 15 {
                    self.var_short_x += 2;
                } else if o == 20 {
                    self.var_short_s += 2;
                }
            }
            2 => {
                if o == 5 {
                    self.var_short_v += 1;
                } else if o == 10 {
                    self.var_short_u += 1;
                } else if o == 15 {
                    self.var_short_t += 2;
                } else if o == 20 {
                    self.var_short_v += 2;
                }
            }
            3 => {
                if o == 5 {
                    self.var_short_s += 1;
                } else if o == 10 {
                    self.var_short_x += 1;
                } else if o == 15 {
                    self.var_short_x += 2;
                } else if o == 20 {
                    self.var_short_s += 2;
                }
            }
            4 => {
                if o == 5 {
                    self.var_short_w += 25;
                } else if o == 10 {
                    self.var_short_v += 2;
                } else if o == 15 {
                    self.var_short_s += 1;
                } else if o == 20 {
                    self.var_short_s += 2;
                }
            }
            5 => {
                if o == 5 {
                    self.var_short_s += 1;
                } else if o == 10 {
                    self.var_short_x += 1;
                } else if o == 15 {
                    self.var_short_s += 2;
                } else if o == 20 {
                    self.var_short_x += 2;
                }
            }
            6 => {
                if o == 5 {
                    self.var_short_v += 1;
                } else if o == 10 {
                    self.var_short_u += 1;
                } else if o == 15 {
                    self.var_short_t += 2;
                } else if o == 20 {
                    self.var_short_u += 2;
                }
            }
            7 => {
                if o == 5 {
                    self.var_short_t += 1;
                } else if o == 10 {
                    self.var_short_u += 1;
                } else if o == 15 {
                    self.var_short_u += 2;
                } else if o == 20 {
                    self.var_short_t += 2;
                }
            }
            8 => {
                if o == 5 {
                    self.var_short_u += 1;
                } else if o == 10 {
                    self.var_short_s += 1;
                } else if o == 15 {
                    self.var_short_t += 2;
                } else if o == 20 {
                    self.var_short_u += 2;
                }
            }
            _ => {}
        }
    }

    /// `h.a(j, long l, boolean bl)` — advance this actor by `l` ms (the per-frame
    /// per-actor update called from `b.java`'s main loop). `model` is the actor's
    /// `var_d_a` animation [`Anim`] (its cursor is advanced; `None` = no model,
    /// matching `g.advance(null,…)`'s no-op); `tables` feed the regen's `h.f`.
    ///
    /// **Ported subset (the rest of `h.a` is deferred):** timers, the animation
    /// advance gate, the move-to-target step (`var_int_arr_j` → `world::apply_delta`
    /// = `h.d`), the player attack-windup (`var_short_a`), the `var_short_k`
    /// damage-over-time, the player health/fatigue regen (every `var_short_d`/`_f`
    /// ms, recomputing via [`Actor::class_progression`]), the `P`/`G` buff-expiry
    /// resets, the floating damage text, the `var_byte_y` status countdown, and
    /// the corpse timer/removal (`b.a`). The remaining branch — the NPC attack AI
    /// (`h.boolean_b` + melee; armed NPCs hit the unported spell path) — asserts
    /// its gating precondition so a caller that reaches it fails loudly rather
    /// than diverging silently. `bl` only gates the (deferred) NPC attack.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        idx: usize,
        actors: &mut [Option<Actor>],
        rng: &mut JavaRandom,
        l: i64,
        bl: bool,
        model: Option<&mut Anim>,
        tables: &Tables,
        effects: &mut Effects,
    ) {
        // Pull self out of the array so the cross-actor branches (DoT dealer,
        // corpse removal, and the deferred NPC AI) can borrow other slots freely,
        // mirroring `Effects::collision_hit`. The slot is `None` for the duration;
        // the actor-array scans already skip self (unique `var_byte_c` / empty).
        let Some(mut me) = actors[idx].take() else {
            return;
        };
        let keep = me.tick_inner(idx, actors, rng, l, bl, model, tables, effects);
        if keep {
            actors[idx] = Some(me);
        }
    }

    /// The body of [`Actor::tick`], run on `self` = the actor taken out of
    /// `actors[idx]`. Returns `true` to write the actor back, `false` to remove it
    /// (corpse removal leaves `actors[idx] = None`).
    #[allow(clippy::too_many_arguments)]
    fn tick_inner(
        &mut self,
        idx: usize,
        actors: &mut [Option<Actor>],
        rng: &mut JavaRandom,
        l: i64,
        _bl: bool,
        model: Option<&mut Anim>,
        tables: &Tables,
        effects: &mut Effects,
    ) -> bool {
        self.var_short_b = (i64::from(self.var_short_b) + l) as i16;
        self.var_int_a = (i64::from(self.var_int_a) + l) as i32;
        self.var_int_e = (i64::from(self.var_int_e) + l) as i32;

        if self.var_short_b > 125 && self.var_byte_q == 0 {
            if let Some(m) = model {
                let key = i32::from(self.var_byte_d)
                    + i32::from(ANIM_STATE_OFFSETS[self.var_byte_e as usize]);
                m.advance(key);
            }
            self.var_short_b = 0;
        }

        if self.var_byte_q == 0 {
            if self.var_int_arr_j[0] != -1 {
                // Move toward var_int_arr_j: step the timer, and once it passes 50
                // advance toward the target along one axis (x first), clamped to not
                // overshoot. On arrival, clear the target. Note: unlike the input
                // move, this does NOT collision-check/revert, and its timer is
                // gated at `>= 50` and clamped to 100 (vs `> 50`/400 in h.void_a).
                self.var_short_g = (i64::from(self.var_short_g) + l) as i16;
                if self.var_short_g >= 50 {
                    if self.var_short_g > 100 {
                        self.var_short_g = 100;
                    }
                    let n3 = i32::from(self.var_short_w) / (1000 / i32::from(self.var_short_g));
                    let (mut n, mut n2) = (0, 0);
                    if self.var_int_arr_b[0] < self.var_int_arr_j[0] {
                        n = n3.min(self.var_int_arr_j[0] - self.var_int_arr_b[0]);
                    } else if self.var_int_arr_b[0] > self.var_int_arr_j[0] {
                        n = (-n3).max(self.var_int_arr_j[0] - self.var_int_arr_b[0]);
                    } else if self.var_int_arr_b[1] < self.var_int_arr_j[1] {
                        n2 = n3.min(self.var_int_arr_j[1] - self.var_int_arr_b[1]);
                    } else if self.var_int_arr_b[1] > self.var_int_arr_j[1] {
                        n2 = (-n3).max(self.var_int_arr_j[1] - self.var_int_arr_b[1]);
                    } else {
                        self.var_int_arr_j[0] = -1;
                        if self.var_byte_e != 2 {
                            self.var_byte_e = 0;
                        }
                    }
                    crate::world::apply_delta(self, n, n2); // h.d(j,n,n2)
                    self.var_short_g = 0;
                }
            } else if self.var_byte_c == 1 && self.var_short_a > 0 {
                self.var_short_a = (i64::from(self.var_short_a) - l) as i16;
                if self.var_short_a <= 0 {
                    self.var_byte_e = 0;
                }
            }

            // var_short_k damage-over-time (h.java:396-403). Both timers decrement
            // by `l`; when the lap timer (var_short_l) reaches 0 it spawns the
            // poison effect (i.a(8,j2)), resets the lap to 1000ms, and applies
            // var_byte_x damage from the dealer (var_j_b), bypassing defense
            // (bl2 = true).
            if self.var_short_k > 0 {
                self.var_short_k = (i64::from(self.var_short_k) - l) as i16;
                self.var_short_l = (i64::from(self.var_short_l) - l) as i16;
                if self.var_short_l <= 0 {
                    effects.spawn_actor(8, 0, self, 0); // i.a(8, j2)
                    self.var_short_l = 1000;
                    // Java's var_j_b is a GC-stable object ref still valid after the
                    // dealer leaves the array; the index model can't represent that,
                    // so an active DoT must have a live, non-self dealer. Fence the
                    // gap loudly rather than silently reading `None` (which would
                    // drop the dealer's var_byte_t RNG draw and desync the stream).
                    debug_assert!(
                        self.var_j_b >= 0
                            && (self.var_j_b as usize) < actors.len()
                            && self.var_j_b as usize != idx
                            && actors[self.var_j_b as usize].is_some(),
                        "DoT dealer must be a live, non-self actor in the array"
                    );
                    let dealer = actors[self.var_j_b as usize].clone().unwrap();
                    // h.a(var_byte_x, j2, var_j_b, false, true).
                    let (died, _) =
                        crate::combat::dot_damage(i32::from(self.var_byte_x), self, &dealer, rng);
                    debug_assert!(!died, "DoT death branch (XP/anim/sound) is out of scope");
                }
            } else if self.var_byte_w == -47 {
                self.var_byte_w = -1;
            }
            if self.var_byte_y == 2 {
                self.var_short_n = (i64::from(self.var_short_n) - l) as i16;
            }

            if self.var_byte_c == 1 {
                // Player health/fatigue regeneration.
                if self.var_short_q < self.var_short_o {
                    self.var_short_c = (i64::from(self.var_short_c) + l) as i16;
                    if self.var_short_c >= self.var_short_d {
                        if self.var_short_q < self.var_short_o {
                            self.var_short_q += 1;
                        }
                        self.var_short_c = 0;
                        self.class_progression(tables);
                    }
                }
                if self.var_short_r < self.var_short_p {
                    self.var_short_e = (i64::from(self.var_short_e) + l) as i16;
                    if self.var_short_e >= self.var_short_f {
                        if self.var_short_r < self.var_short_p {
                            self.var_short_r += 1;
                        }
                        self.var_short_e = 0;
                        self.class_progression(tables);
                    }
                }
                // P-buff expiry: when the buff timer reaches its duration, strip
                // the bonus block and recompute. (Player-only — inside `c == 1`.)
                if self.p_bonus > 0 {
                    if self.var_short_j >= self.p_bonus {
                        self.strip_buffs(effects, tables);
                    }
                    self.var_short_j = (i64::from(self.var_short_j) + l) as i16;
                }
            } else {
                // NPC attack AI (h.boolean_b + melee) is not ported yet; callers
                // keep non-players non-aggressive in the validated subset.
                debug_assert!(
                    self.var_byte_z != 1,
                    "NPC attack AI tick (h.boolean_b) not yet ported"
                );
            }

            // Floating damage text: while text is shown, raise it (`Q -= 2`) and
            // fade its color (`var_int_c -= var_int_d`) every >50ms; clear it once
            // the color runs out or it has risen more than 20px.
            if self.floating_text.is_some() {
                self.var_short_h = (i64::from(self.var_short_h) + l) as i16;
                if self.var_short_h > 50 {
                    self.q_field = (i32::from(self.q_field) - 2) as i16;
                    self.var_int_c -= self.var_int_d;
                    if self.var_int_c <= 0
                        || (i32::from(self.r_field) - i32::from(self.q_field)).abs() > 20
                    {
                        self.var_int_c = 0;
                        self.q_field = 0;
                        self.r_field = 0;
                        self.floating_text = None;
                    }
                    self.var_short_h = 0;
                }
            }

            // G-buff expiry: a countdown that strips the bonus block on reaching 0
            // (applies to all alive actors, not just the player).
            if self.g_field > 0 {
                self.g_field = (i64::from(self.g_field) - l) as i16;
                if self.g_field <= 0 {
                    self.strip_buffs(effects, tables);
                    // The original `return`s here; nothing follows the alive arm,
                    // so falling through is equivalent.
                }
            }
        } else {
            // Dead: corpse timer, then removal (b.a(var_byte_c - 1), b.java:2570) at
            // >= 250ms. We model only the array slot becoming null; the player path
            // (n == 0 -> sound/UI), the `b.g(null)` target-clear, and the
            // var_int_o/void_b draw-order bookkeeping belong to the b.java loop.
            if self.var_short_i >= 250 {
                debug_assert!(
                    self.var_byte_c != 1,
                    "player corpse removal (b.a(0): sound/UI) is out of scope"
                );
                debug_assert!(
                    idx == (self.var_byte_c as usize).wrapping_sub(1),
                    "var_byte_c must encode the actor's own slot + 1 (b.java:2547)"
                );
                return false; // remove: the caller leaves actors[idx] = None
            }
            self.var_short_i = (i64::from(self.var_short_i) + l) as i16;
        }
        true
    }

    /// The shared P/G buff-expiry reset (`h.a` ~433/~481): zero the J/K/L/M/N/H/P
    /// bonus block + `var_byte_w`, clear the attached effect slot, conditionally
    /// recompute max health if `O` was set (zeroing it), then re-run `h.f`. The
    /// original's `if (J != 0)` fatigue recompute is **dead code** (J is zeroed
    /// immediately above), so it never runs — preserved here as this comment.
    fn strip_buffs(&mut self, effects: &mut Effects, tables: &Tables) {
        self.var_short_j = 0;
        self.p_bonus = 0; // P
        self.j_bonus = 0; // J
        self.l_bonus = 0; // L
        self.k_bonus = 0; // K
        self.m_bonus = 0; // M
        self.n_bonus = 0; // N
        self.h_field = 0; // H
        self.var_byte_w = -1;
        effects.clear(i32::from(self.var_byte_h)); // i.a(var_byte_h)
        if self.o_bonus != 0 {
            self.o_bonus = 0; // O zeroed before the recompute uses it
            self.var_short_o = (i32::from(self.var_byte_o) * 4
                + (i32::from(self.var_short_s) + i32::from(self.o_bonus)) * 2
                + i32::from(self.var_short_x) * 2
                + i32::from(self.i_bonus)) as i16;
            self.var_short_d = (40000 / i32::from(self.var_short_o)) as i16;
        }
        self.class_progression(tables);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one `Actor::tick` on a lone actor, wrapped so the single-actor
    /// scenarios below keep asserting straight off `a`. These never reach corpse
    /// removal, so the put-back always succeeds.
    fn tick1(a: &mut Actor, l: i64, model: Option<&mut Anim>, t: &Tables, fx: &mut Effects) {
        let mut arr = vec![Some(std::mem::take(a))];
        let mut rng = JavaRandom::new(0);
        Actor::tick(0, &mut arr, &mut rng, l, false, model, t, fx);
        *a = arr[0].take().expect("tick unexpectedly removed the actor");
    }

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
        assert_eq!(a.var_int_arr_j, [-1, -1]);
        assert_eq!(a.var_byte_y, -1);
    }

    /// A `Tables` isn't needed for these branches (regen, which calls `h.f`, is
    /// validated against the real bytecode in `tick_matches_oracle`).
    fn full_health_player() -> Actor {
        Actor {
            var_byte_c: 1,
            var_short_q: 100,
            var_short_o: 100,
            var_short_r: 100,
            var_short_p: 100,
            ..Default::default()
        }
    }

    #[test]
    fn tick_timers_and_anim_gate() {
        let tables = Tables::default();
        let mut a = full_health_player();
        a.var_short_b = 100;
        tick1(&mut a, 200, None, &tables, &mut Effects::new());
        // var_short_b 100+200=300 > 125 -> reset to 0; other timers accumulate.
        assert_eq!(a.var_short_b, 0);
        assert_eq!(a.var_int_a, 200);
        assert_eq!(a.var_int_e, 200);
    }

    #[test]
    fn tick_attack_windup_counts_down_then_idles() {
        let tables = Tables::default();
        let mut a = full_health_player();
        a.var_short_a = 300;
        a.var_byte_e = 3;
        tick1(&mut a, 200, None, &tables, &mut Effects::new()); // 300-200 = 100, still > 0
        assert_eq!(a.var_short_a, 100);
        assert_eq!(a.var_byte_e, 3);
        tick1(&mut a, 200, None, &tables, &mut Effects::new()); // 100-200 = -100 <= 0 -> idle
        assert_eq!(a.var_byte_e, 0);
    }

    #[test]
    fn tick_dead_accumulates_corpse_timer() {
        let tables = Tables::default();
        let mut a = Actor {
            var_byte_q: 1,
            ..Default::default()
        };
        tick1(&mut a, 60, None, &tables, &mut Effects::new());
        tick1(&mut a, 60, None, &tables, &mut Effects::new());
        assert_eq!(a.var_short_i, 120);
        // The dead branch skips the animation gate, so var_short_b just accumulates.
        assert_eq!(a.var_short_b, 120);
    }

    #[test]
    fn tick_vy_timer_counts_down() {
        let tables = Tables::default();
        let mut a = full_health_player();
        a.var_byte_y = 2;
        a.var_short_n = 1000;
        tick1(&mut a, 200, None, &tables, &mut Effects::new());
        assert_eq!(a.var_short_n, 800);
    }

    // The P/G buff-expiry branches call `class_progression` (`h.f`), which needs
    // the real stat tables, so they are validated against the real bytecode in
    // `tick_matches_oracle` (like the regen branch) rather than unit-tested here.

    #[test]
    fn tick_moves_toward_target_then_clears() {
        let tables = Tables::default();
        let mut a = full_health_player();
        a.var_short_w = 800; // speed
        a.var_int_arr_b = [1000, 1000];
        a.var_int_arr_c = [1010, 1005];
        a.var_int_arr_d = [1005, 1010];
        a.var_int_arr_j = [1200, 1000]; // target: +200 x
                                        // step size = 800 / (1000/100) = 80; clamped to not overshoot.
        tick1(&mut a, 200, None, &tables, &mut Effects::new());
        assert_eq!(a.var_int_arr_b, [1080, 1000]); // moved +80
        assert_eq!(a.var_int_arr_e, [1000, 1000]); // prev saved
        assert_eq!(a.var_byte_d, 3); // facing right
        assert_eq!(a.var_int_arr_i, [10, 130]); // iso recomputed
        tick1(&mut a, 200, None, &tables, &mut Effects::new()); // -> 1160
        tick1(&mut a, 200, None, &tables, &mut Effects::new()); // -> 1200 (min(80,40))
        assert_eq!(a.var_int_arr_b, [1200, 1000]);
        tick1(&mut a, 200, None, &tables, &mut Effects::new()); // arrival: clears target
        assert_eq!(a.var_int_arr_j[0], -1);
    }

    #[test]
    fn tick_dot_damages_victim_each_lap() {
        let tables = Tables::default();
        // Dealer at slot 0; victim (a non-aggressive NPC) at slot 1 with active DoT.
        let dealer = Actor {
            var_byte_c: 1,
            var_byte_t: 1, // creature: no extra var_byte_t RNG draw
            ..Default::default()
        };
        let victim = Actor {
            var_byte_c: 2,     // slot 1 (idx + 1)
            var_byte_z: 0,     // non-aggressive: skip the deferred NPC AI branch
            var_short_q: 1000, // survivable
            var_short_o: 1000,
            var_short_k: 5000, // DoT duration
            var_short_l: 100,  // next lap fires this tick
            var_byte_x: 7,     // 7 damage per lap
            var_j_b: 0,        // dealer is at slot 0
            ..Default::default()
        };
        let mut arr = vec![Some(dealer), Some(victim)];
        let mut rng = JavaRandom::new(12345);
        let before = arr[1].as_ref().unwrap().var_short_q;
        // l=200: var_short_l 100-200 = -100 <= 0 -> lap fires (defense-bypassing).
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            200,
            false,
            None,
            &tables,
            &mut Effects::new(),
        );
        let v = arr[1].as_ref().unwrap();
        assert!(v.var_short_q < before, "DoT should reduce HP");
        assert_eq!(v.var_short_l, 1000, "lap timer resets to 1000");
        assert!(v.var_short_k < 5000, "DoT duration counts down");
    }

    #[test]
    fn tick_dead_npc_removed_at_corpse_timer() {
        let tables = Tables::default();
        let npc = Actor {
            var_byte_c: 2, // slot 1 (idx + 1)
            var_byte_q: 1, // dead
            var_short_i: 240,
            ..Default::default()
        };
        let mut arr = vec![None, Some(npc)];
        let mut rng = JavaRandom::new(0);
        // 240 < 250: not yet removed; corpse timer accumulates to 300.
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            60,
            false,
            None,
            &tables,
            &mut Effects::new(),
        );
        assert_eq!(arr[1].as_ref().unwrap().var_short_i, 300);
        // 300 >= 250: removed from the array.
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            60,
            false,
            None,
            &tables,
            &mut Effects::new(),
        );
        assert!(
            arr[1].is_none(),
            "dead NPC removed at the 250ms corpse threshold"
        );
    }
}
