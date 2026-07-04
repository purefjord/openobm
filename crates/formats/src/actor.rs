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
    /// The attack-target/aggressor back-ref (`j.var_j_a`, a Java object ref
    /// modeled as an actor-array slot index; `-1` = null). Set by the NPC attack
    /// AI (`h.boolean_b`) to its chosen target, and by `apply_damage`
    /// (`h.a:1114`) to the first attacker if unset.
    pub var_j_a: i32,
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
    /// Attack period in ms (`j.var_short_m`, default 1000): the NPC melee fires
    /// when the cooldown timer `var_int_e` reaches it.
    pub var_short_m: i16,
    /// Summoner wander phase (`j.var_byte_A`): `1` = vanished (teleported
    /// off-map by `h.boolean_c`), waiting to reappear near the player.
    pub a_phase: i8,
    /// Timed buff duration (`j.G`); on expiry it strips the J..P bonus block.
    pub g_field: i16,
    /// Aggression flag (`j.var_byte_z`, default 1); gates the NPC attack AI.
    pub var_byte_z: i8,
    /// The actor's attached effect-pool slot (`j.var_byte_h`, default -1); cleared
    /// when a buff expires.
    pub var_byte_h: i8,

    // --- world coupling (the b/e-layer fields, M11 gameplay slice) ---
    /// Inventory (`j.var_int_arr_k`, 255 slots): each entry is `kind << 8 | id`
    /// (kind 0 = weapon, 1 = armor, 2 = consumable/spell); 0 = empty.
    pub var_int_arr_k: Vec<i32>,
    /// Drops-loot-on-death flag (`j.var_byte_s`, default 1; the spawner zeroes
    /// it for the player).
    pub var_byte_s: i8,
    /// Death-trigger script entry (`j.var_byte_k`, default -1; set by op32 /
    /// `h.c(j,int,int)`; pushed via `e.void_a` when the actor dies).
    pub var_byte_k: i8,
    /// Overlay-layer samples under the actor (`j.var_byte_l/m/n`, default -1):
    /// the enter value, the leave value, and the action value (`h.a(j,[B[B)` /
    /// `h.a(j,[B)`).
    pub var_byte_l: i8,
    pub var_byte_m: i8,
    pub var_byte_n: i8,
    /// Active health/fatigue regen consumable rows (`j.var_int_arr_f/g`).
    pub var_int_arr_f: Option<Vec<i32>>,
    pub var_int_arr_g: Option<Vec<i32>>,
    /// The subtype-0 spawn stat row (`j.var_int_arr_o`; kept for the summon
    /// re-spawn `b.var_b_a.a("/oh_scamp.cml", …, var_int_arr_o)`).
    pub var_int_arr_o: Option<Vec<i32>>,
    /// The subtype-5 class row + its `e.j` aux row (`j.var_int_arr_a` /
    /// `j.var_int_arr_h`, set by the class init `h.a(j,byte,boolean)`).
    pub var_int_arr_a: Option<Vec<i32>>,
    pub var_int_arr_h: Option<Vec<i32>>,
    /// The player's alternate weapon/spell row (`j.var_int_arr_m`, set by the
    /// weapon-toggle `h.a(j,String)`; read by the equip gating).
    pub var_int_arr_m: Option<Vec<i32>>,
    /// Summon link slots (`j.var_j_c` = my summon, `j.var_j_d` = my master);
    /// -1 = none (Java object refs, modeled as array slots).
    pub var_j_c: i32,
    pub var_j_d: i32,
    /// HUD status icon anim key (`j.var_byte_v`, default -45).
    pub var_byte_v: i8,
    /// Dialogue-facing anim key (`j.var_byte_g`, default -1; set by op46/op53
    /// via `h.b(j,byte)`, cleared by the e-prologue's post-dialogue reset).
    pub var_byte_g: i8,
    /// Model resource name (`j.var_java_lang_String_b`, e.g. "/oh_pc.cml") and
    /// display name (`j.var_java_lang_String_c`, the op15 spawn name).
    pub model_name: String,
    pub display_name: Option<String>,

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
            var_j_a: -1,
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
            var_short_m: 1000,
            a_phase: 0,
            g_field: 0,
            var_byte_z: 1,
            var_byte_h: -1,
            floating_text: None,
            var_short_h: 0,
            q_field: 0,
            r_field: 0,
            var_int_c: 0xFF_0000,
            var_int_d: 0,
            var_int_arr_k: vec![0; 255],
            var_byte_s: 1,
            var_byte_k: -1,
            var_byte_l: -1,
            var_byte_m: -1,
            var_byte_n: -1,
            var_int_arr_f: None,
            var_int_arr_g: None,
            var_int_arr_o: None,
            var_int_arr_a: None,
            var_int_arr_h: None,
            var_int_arr_m: None,
            var_j_c: -1,
            var_j_d: -1,
            var_byte_v: -45,
            var_byte_g: -1,
            model_name: String::new(),
            display_name: None,
        }
    }
}

/// A side effect an actor tick/combat resolution asks the `b`/`e` world layer to
/// perform — the branches that reach past the actor array (script-entry pushes,
/// loot-pickup drops, the summon spawner). Emitted in execution order; the world
/// applies them after the per-actor call returns. Deferral is faithful: nothing
/// later in the same `h.a` tick reads the affected state (the summoned actor is
/// same-faction — skipped by the fall-through AoE — and the script stack only
/// executes on the *next* `e.a` tick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldEvent {
    /// `e.void_a(entry)` — push a script entry (the death trigger `var_byte_k`).
    PushEntry(u8),
    /// `b.var_b_a.a(item, false, tile_x, tile_y)` — drop a loot pickup marker
    /// at the victim's corpse tile (item id already drawn from `e.int_a()`).
    DropLoot { item: i32, x: i32, y: i32 },
    /// `b.a(slot)` — remove an actor (the summoner replacing its old summon).
    RemoveActor(usize),
    /// `b.var_b_a.a("/oh_scamp.cml", x, y, row)` + link wiring — spawn a summon
    /// for `caster` (its slot) using the caster's spawn stat row.
    Summon { caster: usize, x: i32, y: i32 },
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
    /// Subtype-5 aux lists, indexed by class id: `e.i` (class-permission tags)
    /// and `e.j` (the per-class row `h.a(j,byte,bool)` hangs on the actor).
    class_aux_i: Vec<Vec<i32>>,
    class_aux_j: Vec<Vec<i32>>,
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

    /// Mutable row access (the loot list `e.l` decrements its counts).
    pub fn row_mut(&mut self, subtype: u8, idx: i32) -> Option<&mut Vec<i32>> {
        if idx < 0 {
            return None;
        }
        self.by_subtype
            .get_mut(&subtype)
            .and_then(|t| t.get_mut(idx as usize))
    }

    /// Mutable iteration over a subtype's rows (the per-load subtype-10 clear).
    pub fn row_iter_mut(&mut self, subtype: u8) -> Option<impl Iterator<Item = &mut Vec<i32>>> {
        self.by_subtype.get_mut(&subtype).map(|t| t.iter_mut())
    }

    /// Owned copies of the subtype-5 aux tables (for read-modify-write merges).
    pub fn class_aux_i_rows(&self) -> Vec<Vec<i32>> {
        self.class_aux_i.clone()
    }
    pub fn class_aux_j_rows(&self) -> Vec<Vec<i32>> {
        self.class_aux_j.clone()
    }

    /// `e.boolean_a(int n, int n2)` — true if the subtype-5 aux list `e.i[n]`
    /// (the class-permission tag list) contains `n2`. The list is stored here as
    /// the aux rows of subtype 5 (see [`Tables::insert_class_aux`]).
    pub fn class_allows(&self, class: i8, tag: i32) -> bool {
        self.class_aux_i
            .get(class.max(0) as usize)
            .is_some_and(|l| l.contains(&tag))
    }

    /// The subtype-5 `e.j` aux row for a class (`j.var_int_arr_h`).
    pub fn class_aux_j(&self, class: i8) -> Option<&[i32]> {
        self.class_aux_j
            .get(class.max(0) as usize)
            .map(Vec::as_slice)
    }

    /// Install the subtype-5 aux lists (`e.i` / `e.j`, indexed by class id).
    pub fn insert_class_aux(&mut self, aux_i: Vec<Vec<i32>>, aux_j: Vec<Vec<i32>>) {
        self.class_aux_i = aux_i;
        self.class_aux_j = aux_j;
    }
}

/// `e.int_a()` — the loot draw: one RNG draw, then walk the subtype-10 list
/// (`e.l`, `[30][4]`) from row 1: a row with a zero item id (`[1]`) ends the
/// walk (no loot); the first row whose modulus matches (`n % [2] == 0`) with a
/// positive count (`[3]`) is decremented and its item id returned. Rows never
/// filled by a load read as zeroed.
pub fn loot_roll(tables: &mut Tables, rng: &mut JavaRandom) -> i32 {
    let n = rng.next_int();
    let mut n2 = 1i32;
    loop {
        let item = tables.row(10, n2).map(|r| r[1]).unwrap_or(0);
        if item == 0 {
            return 0;
        }
        let row = tables.row_mut(10, n2).unwrap();
        if n % row[2] == 0 && row[3] > 0 {
            row[3] -= 1;
            return row[1];
        }
        n2 += 1;
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

    // ------------------------------------------------------------------
    // The b/e-layer actor API (M11 gameplay slice): the h.java methods the
    // script opcodes and the spawner call. All transcribed from the CFR
    // decompile (h.java line refs in each doc comment).
    // ------------------------------------------------------------------

    /// `h.a(String, byte)` (h.java:24) — the actor factory: a fresh `j` with the
    /// model resource name, the actor id (`var_byte_c` = slot + 1), armor slots
    /// cleared to -1, and the collision-box extents from the model's group-1
    /// frame width (`var_byte_a = g.a(d,1)`, `var_byte_b = a >> 1`) — passed in
    /// by the caller, which owns the loaded [`Anim`].
    pub fn create(model_name: &str, id: i8, frame_w: i32) -> Actor {
        Actor {
            model_name: model_name.to_string(),
            var_byte_c: id,
            var_byte_arr_a: [0, 0],
            var_int_arr_n: [-1; 8],
            var_byte_a: frame_w as i8,
            var_byte_b: (frame_w as i8) >> 1,
            ..Actor::default()
        }
    }

    /// `h.a(j, int[])` (h.java:1046) — apply an op15 spawn stat row (subtype 0).
    /// Non-players take the full attribute block, weapon/armor equips, and the
    /// spell row (`e.k[row[19]]`); everyone takes level, faction, and the
    /// health/fatigue re-derivation (+ the `E`/`F` = 300/200 defaults) + `h.f`.
    pub fn apply_stat_row(&mut self, row: &[i32], tables: &Tables) {
        self.var_int_arr_o = Some(row.to_vec());
        self.var_byte_o = row[2] as i8;
        if self.var_byte_c != 1 {
            self.var_short_s = row[3] as i16;
            self.var_short_t = row[4] as i16;
            self.var_short_u = row[5] as i16;
            self.var_short_v = row[6] as i16;
            self.var_short_w = row[7] as i16;
            self.var_short_x = row[8] as i16;
            self.var_short_y = row[9] as i16;
            self.e_field = row[14] as i16;
            self.f_field = row[15] as i16;
            self.var_byte_j = row[10] as i8;
            self.var_byte_y = row[18] as i8;
            let armor = row[11];
            self.var_int_arr_l = tables.row(8, row[19]).map(|r| r.to_vec());
            self.var_byte_t = i8::from(self.var_byte_y == 4);
            if row[20] > 0 {
                self.var_short_m = (row[20] * 1000) as i16;
            }
            if self.var_byte_t == 1 || self.var_byte_y == 0 {
                self.var_int_arr_l = None;
            }
            if self.var_byte_j > 0 {
                let w = tables
                    .row(4, i32::from(self.var_byte_j))
                    .map(<[i32]>::to_vec);
                if let Some(w) = w {
                    self.equip(0, &w, false, tables);
                }
            }
            if armor > 0 {
                let a = tables.row(1, armor).map(<[i32]>::to_vec);
                if let Some(a) = a {
                    self.equip(1, &a, false, tables);
                }
            }
        }
        self.var_byte_r = row[13] as i8;
        self.recompute_to_full();
        if self.e_field == 0 {
            self.e_field = 300; // h.var_short_a
        }
        if self.f_field == 0 {
            self.f_field = 200; // h.var_short_b
        }
        self.class_progression(tables);
    }

    /// `h.a(j, byte, boolean)` (h.java:1765) — the class init (the player on
    /// spawn / save load). Hangs the subtype-5 class row + its `e.j` aux row on
    /// the actor; unless `from_save`, equips the class weapon/armor rows and
    /// takes the attribute block from the class row. Ends in `h.f`.
    pub fn class_init(&mut self, class: i8, from_save: bool, tables: &Tables) {
        self.var_byte_f = class;
        if self.var_byte_f == 4 {
            self.var_byte_t = 1;
        }
        let row = tables
            .row(5, i32::from(class))
            .map(<[i32]>::to_vec)
            .expect("class row (subtype 5) must exist");
        self.var_int_arr_h = tables.class_aux_j(class).map(<[i32]>::to_vec);
        if !from_save {
            if let Some(w) = tables.row(4, row[4]).map(<[i32]>::to_vec) {
                self.equip(0, &w, false, tables);
            }
            if let Some(a) = tables.row(1, row[5]).map(<[i32]>::to_vec) {
                self.equip(1, &a, false, tables);
            }
            self.var_short_s = row[7] as i16;
            self.var_short_t = row[8] as i16;
            self.var_short_u = row[9] as i16;
            self.var_short_v = row[10] as i16;
            self.var_short_w = row[6] as i16;
            self.var_short_x = row[11] as i16;
            self.var_short_y = row[12] as i16;
            self.f_field = row[13] as i16;
            self.e_field = row[14] as i16;
        }
        self.var_int_arr_a = Some(row);
        self.class_progression(tables);
    }

    /// `h.boolean_a(j, int, int[])` (h.java:2122) — may this actor's class use
    /// the weapon (`kind == 0`) / armor (`kind == 1`) row? Resolved against the
    /// subtype-5 class-permission list (`e.boolean_a`).
    fn class_allows_item(&self, kind: i32, row: &[i32], tables: &Tables) -> bool {
        if self.var_byte_f == -1 {
            return false;
        }
        let tag = match (kind, row[2]) {
            (0, 1) => 5,
            (0, 2) => 6,
            (0, 3) => 7,
            (0, 4) => 8,
            (0, 0) => 14,
            (1, 2) => 4,
            (1, 1) => 3,
            (1, 0) => 1,
            _ => return true,
        };
        tables.class_allows(self.var_byte_f, tag)
    }

    /// `h.a(j, int, int[], boolean)` (h.java:1666) + `h.void_a` — add an item
    /// row to the inventory (`var_int_arr_k`, tag `kind << 8 | id`) and apply
    /// it: armor (`1`) equips into its slot if free (or `force`); consumables
    /// (`2`) with `[5] == 0` arm the health/fatigue regen rows; a weapon (`0`)
    /// becomes the equipped `var_byte_j` if the hand is free, the class allows
    /// it, or `force`.
    pub fn equip(&mut self, kind: i32, row: &[i32], force: bool, tables: &Tables) {
        let mut n2 = 0usize;
        while n2 < self.var_int_arr_k.len() && self.var_int_arr_k[n2] != 0 {
            n2 += 1;
        }
        if n2 >= self.var_int_arr_k.len() {
            return;
        }
        match kind {
            1 => {
                if self.var_int_arr_n[row[3] as usize] == -1 || force {
                    self.equip_armor(row, tables);
                }
                self.var_int_arr_k[n2] = 0x100 | row[0];
            }
            2 => {
                self.var_int_arr_k[n2] = 0x200 | row[0];
                if row[5] != 0 {
                    return;
                }
                if self.var_int_arr_f.is_none() && row[2] > 0 {
                    self.var_int_arr_f = Some(row.to_vec());
                    return;
                }
                if self.var_int_arr_g.is_none() && row[3] > 0 {
                    self.var_int_arr_g = Some(row.to_vec());
                }
            }
            0 => {
                self.var_int_arr_k[n2] = row[0];
                if (self.var_byte_j != 0
                    || self.var_int_arr_l.is_some()
                    || !self.class_allows_item(0, row, tables))
                    && !force
                {
                    return;
                }
                self.var_byte_j = row[0] as i8;
            }
            _ => {}
        }
    }

    /// `h.c(j, int[])` (h.java:1989) — equip an armor row into its slot
    /// (`var_int_arr_n[row[3]]`), class-permission-gated, then `h.f`.
    pub fn equip_armor(&mut self, row: &[i32], tables: &Tables) {
        if !self.class_allows_item(1, row, tables) {
            return;
        }
        self.var_int_arr_n[row[3] as usize] = row[0];
        self.class_progression(tables);
    }

    /// `h.b(j, int, int[])` (h.java:1714) — remove an item row from the
    /// inventory (shift-left over the first matching tag); un-equipping the held
    /// weapon re-picks the best allowed one (`h.h`). Ends in `h.f`.
    pub fn unequip(&mut self, kind: i32, row: &[i32], tables: &Tables) {
        let mut repick = false;
        let n4 = match kind {
            1 => 0x100 | row[0],
            2 => 0x200 | row[0],
            0 => {
                if self.var_byte_j == row[0] as i8 {
                    self.var_byte_j = 0;
                    self.var_byte_t = 0;
                    repick = true;
                }
                row[0]
            }
            _ => 0,
        };
        let mut n2 = 0usize;
        while n2 < self.var_int_arr_k.len() && self.var_int_arr_k[n2] != 0 {
            if self.var_int_arr_k[n2] == n4 {
                for n3 in n2..self.var_int_arr_k.len() - 1 {
                    self.var_int_arr_k[n3] = self.var_int_arr_k[n3 + 1];
                }
                break;
            }
            n2 += 1;
        }
        if repick {
            self.repick_weapon(tables);
        }
        self.class_progression(tables);
    }

    /// `h.h(j)` (h.java:1700) — re-pick the best allowed weapon from the
    /// inventory (highest `row[3]`), setting `var_byte_j` + the creature flag.
    fn repick_weapon(&mut self, tables: &Tables) {
        let mut best: Option<Vec<i32>> = None;
        let mut n = 0usize;
        while n < self.var_int_arr_k.len() && self.var_int_arr_k[n] != 0 {
            let entry = self.var_int_arr_k[n];
            n += 1;
            if entry > 255 {
                continue;
            }
            let Some(row) = tables.row(4, entry & 0xFF).map(<[i32]>::to_vec) else {
                continue;
            };
            if !self.class_allows_item(0, &row, tables) {
                continue;
            }
            if best.as_ref().is_some_and(|b| row[3] <= b[3]) {
                continue;
            }
            best = Some(row);
        }
        if let Some(b) = best {
            self.var_byte_j = b[0] as i8;
            self.var_byte_t = i8::from(b[2] == 4);
        }
    }

    /// `h.a(j, int, int, e)` (h.java:1573) — the op34 attribute write, followed
    /// by the health/fatigue re-derivation (clamping current values), the race
    /// row refresh from the equipped weapon, the `E`/`F` defaults, and `h.f`.
    pub fn attr_set(&mut self, attr: i32, value: i32, tables: &Tables) {
        match attr {
            2 => self.var_byte_o = value as i8,
            3 => self.var_short_s = value as i16,
            4 => self.var_short_t = value as i16,
            5 => self.var_short_u = value as i16,
            6 => self.var_short_v = value as i16,
            7 => self.var_short_w = value as i16,
            8 => self.var_short_x = value as i16,
            9 => self.var_short_y = value as i16,
            10 => self.var_byte_j = value as i8,
            13 => self.var_byte_r = value as i8,
            14 => self.e_field = value as i16,
            15 => self.f_field = value as i16,
            19 => self.var_int_arr_l = tables.row(8, value).map(<[i32]>::to_vec),
            18 => {
                self.var_byte_y = value as i8;
                self.var_byte_t = i8::from(self.var_byte_y == 4);
                if self.var_byte_t == 1 || self.var_byte_y == 0 {
                    self.var_int_arr_l = None;
                }
            }
            20 => self.var_short_m = (value * 1000) as i16,
            _ => {}
        }
        self.recompute();
        self.var_short_q = self.var_short_q.min(self.var_short_o);
        self.var_short_r = self.var_short_r.min(self.var_short_p);
        if self.var_byte_j > 0 {
            self.var_byte_i = tables
                .row(4, i32::from(self.var_byte_j))
                .expect("weapon row")[3] as i8;
        }
        if self.e_field == 0 {
            self.e_field = 300;
        }
        if self.f_field == 0 {
            self.f_field = 200;
        }
        self.class_progression(tables);
    }

    /// `h.b(j, byte)` (h.java:1788) — the op46/op53 facing write (an anim key,
    /// not the movement facing `var_byte_d`).
    pub fn set_facing(&mut self, by: i8) {
        self.var_byte_g = match by {
            2 => -52,
            1 => -53,
            3 => -51,
            4 => -2,
            0 => -1,
            _ => return,
        };
    }

    /// `h.a(j, byte)` (h.java:559, the void `g.a(Ld;I)V` overload) — set the
    /// animation state: state 6 marks the actor dead; a *changed* state resets
    /// the model's anim group for the current facing.
    pub fn set_anim(&mut self, by: i8, model: Option<&mut Anim>) {
        if by == 6 {
            self.var_byte_q = 1;
        } else if self.var_byte_e != by {
            if let Some(m) = model {
                m.reset(i32::from(self.var_byte_d) + i32::from(ANIM_STATE_OFFSETS[by as usize]));
            }
        }
        self.var_byte_e = by;
    }

    /// `h.b(j, int, int)` (h.java:345) — set the walk target (op17/41/42 and
    /// the cutscene sequencer): the tick's move-to-target consumes it.
    pub fn set_walk_target(&mut self, x: i32, y: i32) {
        self.var_int_arr_j = [x, y];
        self.var_byte_e = 1;
    }

    /// `h.b(j, int)` (h.java:2024) — the op65 level growth: +1 levels (all seven
    /// attributes + the class bonus + max/rate re-derivation — current health/
    /// fatigue are NOT refilled — + `h.f`) until `level >= n`.
    pub fn grow_level(&mut self, n: i32, tables: &Tables) {
        while i32::from(self.var_byte_o) < n {
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

    /// `h.void_a(j)` (h.java:89, the actor part) — the player re-init on level
    /// entry via the spawner's slot-0 reuse: clear the samples/dead/facing/
    /// text/anim/walk/DoT state, re-derive health/fatigue to full, `h.f` + `h.e`.
    /// (The array-wide `var_j_a = null` sweep is the world's job.)
    pub fn player_reset(&mut self, tables: &Tables) {
        self.var_byte_arr_a = [0, 0];
        self.var_byte_l = -1;
        self.var_byte_m = -1;
        self.var_byte_n = -1;
        self.var_byte_q = 0;
        self.var_byte_g = -1;
        self.var_j_a = -1;
        self.var_short_i = 0;
        self.floating_text = None;
        self.q_field = 0;
        self.var_int_c = 0xFF_0000;
        self.var_byte_e = 0;
        self.var_int_arr_j = [-1, -1];
        self.var_short_k = 0;
        self.var_short_l = 0;
        self.var_j_b = -1;
        self.recompute_to_full();
        self.class_progression(tables);
        crate::world::pick_primary(self);
    }

    /// `h.i(j)` (h.java:2078) — refresh the HUD status icon (`var_byte_v`)
    /// from the active weapon/spell row's type: buffs/bolts/poison/cure map to
    /// their icon keys; no row = the default -45.
    pub fn refresh_icon(&mut self) {
        let Some(w) = self.var_int_arr_l.as_ref() else {
            self.var_byte_v = -45;
            return;
        };
        self.var_byte_v = match w[2] {
            0 | 5 => -48,
            1 => -50,
            2 => -46,
            3 => {
                if w[1] == 61618 {
                    -44
                } else if w[1] == 61619 {
                    -43
                } else {
                    -50
                }
            }
            4 => -47,
            6 => -43,
            _ => return,
        };
    }

    /// `h.a(j, byte[])` (h.java:1433) — the FIRE/action dispatch: sample the
    /// action overlay under the three corners (a valid value in `0..255` sets
    /// `var_byte_n` and returns it — the caller pushes the action entry);
    /// otherwise an unarmed non-creature swings (windup + anim 4), and on the
    /// attack cooldown (`var_int_e >= var_short_m`) an armed/creature player
    /// casts (`h.c(j, true)`), else re-acquires a target (`h.boolean_b`),
    /// faces it (`h.b(j,j)`), and melees — a kill drops the target locks.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_action(
        &mut self,
        idx: usize,
        actors: &mut [Option<Actor>],
        action: &[i8],
        height: i32,
        tables: &mut Tables,
        effects: &mut Effects,
        events: &mut Vec<WorldEvent>,
        rng: &mut JavaRandom,
    ) -> i8 {
        self.var_byte_n = -1;
        if action.is_empty() {
            return self.var_byte_n; // Java: a null layer returns without the swing
        }
        {
            let cells = [
                i32::from(self.var_byte_arr_b[0]) * height + i32::from(self.var_byte_arr_b[1]),
                i32::from(self.var_byte_arr_c[0]) * height + i32::from(self.var_byte_arr_c[1]),
                i32::from(self.var_byte_arr_d[0]) * height + i32::from(self.var_byte_arr_d[1]),
            ];
            for &c in &cells {
                if c < 0 || c >= action.len() as i32 {
                    return -1;
                }
                let v = action[c as usize];
                // Java: `byArray[i] < 0 || byArray[i] >= 255` skips (a byte is
                // never >= 255; negatives = no action).
                if v < 0 {
                    continue;
                }
                self.var_byte_n = v;
                return v;
            }
        }
        if self.var_byte_t == 0 && self.var_int_arr_l.is_none() {
            self.var_short_a = 500;
            self.var_byte_e = 4;
        }
        if self.var_int_e >= i32::from(self.var_short_m) {
            self.var_int_e = 0;
            if self.var_int_arr_l.is_some() || self.var_byte_t == 1 {
                self.cast(idx, actors, effects, tables, rng, true, events);
            } else {
                self.attack_ai(actors);
                if self.var_j_a != -1 {
                    let tgt = self.var_j_a as usize;
                    debug_assert!(
                        tgt < actors.len() && actors[tgt].is_some(),
                        "fire target must be a live actor in the array"
                    );
                    let target_snapshot = actors[tgt].as_ref().unwrap().clone();
                    self.face_toward(&target_snapshot);
                    let mut target = actors[tgt].take().unwrap();
                    let (died, _) = crate::combat::melee_attack(
                        self,
                        idx,
                        &mut target,
                        actors,
                        true,
                        tables,
                        events,
                        rng,
                    );
                    actors[tgt] = Some(target);
                    if died {
                        self.var_j_a = -1;
                        self.var_j_b = -1;
                        self.var_byte_e = 0;
                    }
                }
            }
        }
        -1
    }

    /// `h.a(j, byte[], byte[])` (h.java:323) — sample the enter (`arr_j`) and
    /// leave (`arr_k`) overlay layers under the actor's three corner cells:
    /// the first cell with a value not in {0, -1} wins; sets `var_byte_l`
    /// (enter sample) + `var_byte_m` (leave sample) and returns the enter value
    /// (-1 = none). `height` is the column stride (`b.var_byte_g`).
    pub fn sample_overlay(&mut self, enter: &[i8], leave: &[i8], height: i32) -> i8 {
        self.var_byte_l = -1;
        self.var_byte_m = -1;
        let cells = [
            i32::from(self.var_byte_arr_b[0]) * height + i32::from(self.var_byte_arr_b[1]),
            i32::from(self.var_byte_arr_c[0]) * height + i32::from(self.var_byte_arr_c[1]),
            i32::from(self.var_byte_arr_d[0]) * height + i32::from(self.var_byte_arr_d[1]),
        ];
        for &c in &cells {
            if c < 0 || c >= enter.len() as i32 {
                return -1;
            }
            let v = enter[c as usize];
            if v == 0 || v == -1 {
                continue;
            }
            self.var_byte_l = v;
            self.var_byte_m = leave[c as usize];
            return v;
        }
        -1
    }
    ///
    /// **Ported subset (the rest of `h.a` is deferred):** timers, the animation
    /// advance gate, the move-to-target step (`var_int_arr_j` → `world::apply_delta`
    /// = `h.d`), the player attack-windup (`var_short_a`), the `var_short_k`
    /// damage-over-time, the player health/fatigue regen (every `var_short_d`/`_f`
    /// ms, recomputing via [`Actor::class_progression`]), the NPC attack AI
    /// (`h.boolean_b` + the strike at `h.a:457` — `bl` gates attacks on the
    /// player; armed/creature NPCs take the spell path, [`Actor::cast`] +
    /// the `var_byte_y` follow-ups), the `P`/`G` buff-expiry resets, the floating
    /// damage text, the `var_byte_y` status countdown, and the corpse
    /// timer/removal (`b.a`). The remaining branches — the summon cast (weapon
    /// type 2, needs the `b` actor spawner) and any death in combat — assert
    /// their gating preconditions so a caller that reaches one fails loudly
    /// rather than diverging silently.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        idx: usize,
        actors: &mut [Option<Actor>],
        rng: &mut JavaRandom,
        l: i64,
        bl: bool,
        model: Option<&mut Anim>,
        tables: &mut Tables,
        effects: &mut Effects,
        map: Option<&crate::world::MapRef>,
        events: &mut Vec<WorldEvent>,
    ) {
        // Pull self out of the array so the cross-actor branches (DoT dealer,
        // corpse removal, and the NPC AI) can borrow other slots freely,
        // mirroring `Effects::collision_hit`. The slot is `None` for the duration;
        // the actor-array scans already skip self (unique `var_byte_c` / empty).
        let Some(mut me) = actors[idx].take() else {
            return;
        };
        let keep = me.tick_inner(idx, actors, rng, l, bl, model, tables, effects, map, events);
        if keep {
            actors[idx] = Some(me);
        }
    }

    /// The body of [`Actor::tick`], run on `self` = the actor taken out of
    /// `actors[idx]`. Returns `true` to write the actor back, `false` to remove it
    /// (corpse removal leaves `actors[idx] = None`). `map` is only read by the
    /// summoner wander (`h.boolean_c`); `None` fences that branch.
    #[allow(clippy::too_many_arguments)]
    fn tick_inner(
        &mut self,
        idx: usize,
        actors: &mut [Option<Actor>],
        rng: &mut JavaRandom,
        l: i64,
        bl: bool,
        model: Option<&mut Anim>,
        tables: &mut Tables,
        effects: &mut Effects,
        map: Option<&crate::world::MapRef>,
        events: &mut Vec<WorldEvent>,
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
                    let dealer_idx = self.var_j_b as usize;
                    // Take the dealer out so a kill's death branch can mutate it
                    // (the swing-timer reset + XP), mirroring Java's object ref.
                    let mut dealer = actors[dealer_idx].take().unwrap();
                    // h.a(var_byte_x, j2, var_j_b, false, true).
                    let _ = crate::combat::dot_damage(
                        i32::from(self.var_byte_x),
                        self,
                        &mut dealer,
                        dealer_idx,
                        tables,
                        events,
                        rng,
                    );
                    actors[dealer_idx] = Some(dealer);
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
            } else if self.var_byte_z == 1
                && self.attack_ai(actors)
                && self.var_j_a != -1
                && self.var_int_e >= i32::from(self.var_short_m)
            {
                // The strike (h.a:457): fires only against a non-player target
                // unless `bl`; the cooldown resets either way. A `true` return
                // (target died) drops the target locks. Java's var_j_a is an
                // object ref; the index model needs the slot still live.
                debug_assert!(
                    (self.var_j_a as usize) < actors.len()
                        && actors[self.var_j_a as usize].is_some(),
                    "attack target must be a live actor in the array"
                );
                let tgt = self.var_j_a as usize;
                if bl || actors[tgt].as_ref().unwrap().var_byte_c != 1 {
                    // h.a(j2, var_j_a, true): an armed or creature NPC takes the
                    // spell branch (h.a:1204) — cast, then the var_byte_y==3
                    // weapon-drop / var_byte_y==2 teleport-wander follow-ups —
                    // and h.a returns false. Everyone else melees.
                    let died = if self.var_byte_c != 1
                        && (self.var_int_arr_l.is_some() || self.var_byte_t == 1)
                    {
                        self.cast(idx, actors, effects, tables, rng, false, events);
                        if self.var_byte_y == 3 {
                            self.var_int_arr_l = None;
                            self.f_field >>= 1;
                        } else if self.var_byte_y == 2 && self.var_short_n <= 0 {
                            let map = map
                                .expect("the summoner wander (h.boolean_c) needs the map layers");
                            if self.wander(actors, map, effects, rng) {
                                self.var_short_n = (rng.next_int().abs() % 2000 + 2000) as i16;
                            }
                        }
                        false
                    } else {
                        let mut target = actors[tgt].take().unwrap();
                        let (died, _) = crate::combat::melee_attack(
                            self,
                            idx,
                            &mut target,
                            actors,
                            true,
                            tables,
                            events,
                            rng,
                        );
                        actors[tgt] = Some(target);
                        died
                    };
                    if died {
                        self.var_j_a = -1;
                        self.var_j_b = -1;
                        self.var_byte_e = 0;
                    }
                }
                self.var_int_e = 0;
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

    /// `h.b(j, int n, int n2)` — set the move-to target and enter the walking
    /// animation state.
    fn set_move_target(&mut self, n: i32, n2: i32) {
        self.var_int_arr_j[0] = n;
        self.var_int_arr_j[1] = n2;
        self.var_byte_e = 1;
    }

    /// `h.a(j j2, j j3)` — step toward `target`: pick the axis with the larger
    /// world-position gap and issue a 20-unit move-to along it.
    fn move_toward(&mut self, target: &Actor) {
        let n = self.var_int_arr_b[0] - target.var_int_arr_b[0];
        let n2 = self.var_int_arr_b[1] - target.var_int_arr_b[1];
        if n.abs() > n2.abs() {
            if n > 0 {
                self.set_move_target(self.var_int_arr_b[0] - 20, self.var_int_arr_b[1]);
            } else {
                self.set_move_target(self.var_int_arr_b[0] + 20, self.var_int_arr_b[1]);
            }
        } else if n2 > 0 {
            self.set_move_target(self.var_int_arr_b[0], self.var_int_arr_b[1] - 20);
        } else {
            self.set_move_target(self.var_int_arr_b[0], self.var_int_arr_b[1] + 20);
        }
    }

    /// `h.b(j j2, j j3)` — face `target` by iso/screen position quadrant
    /// (`var_int_arr_i`). Facing is unchanged when either axis is equal.
    fn face_toward(&mut self, target: &Actor) {
        let (a, t) = (&self.var_int_arr_i, &target.var_int_arr_i);
        if a[0] < t[0] && a[1] > t[1] {
            self.var_byte_d = 2;
        } else if a[0] > t[0] && a[1] < t[1] {
            self.var_byte_d = 1;
        } else if a[0] < t[0] && a[1] < t[1] {
            self.var_byte_d = 3;
        } else if a[0] > t[0] && a[1] > t[1] {
            self.var_byte_d = 4;
        }
    }

    /// `h.boolean_b(j)` — the NPC attack-AI decision, run from the tick.
    /// Scan for the nearest valid enemy (`h.j_a`); if one is within the aggro
    /// range `E`: in attack range (`< F`) lock it as `var_j_a` + enter the attack
    /// state + face it; otherwise step toward it and return `false` (skipping the
    /// melee this frame). Out of range (or no target), a held `var_j_a` is
    /// dropped — unless `var_byte_y == 2` keeps it. Returns `true` to let the
    /// tick's melee gate run.
    fn attack_ai(&mut self, actors: &[Option<Actor>]) -> bool {
        if let Some(t) = crate::combat::nearest_target(actors, self) {
            let target = actors[t].as_ref().unwrap();
            let n = crate::combat::combat_distance(&self.var_int_arr_b, &target.var_int_arr_b);
            if n <= i32::from(self.e_field) {
                if n >= i32::from(self.f_field) {
                    if self.var_byte_c != 1 {
                        self.move_toward(target);
                        return false;
                    }
                } else {
                    self.var_int_arr_j[0] = -1;
                    self.var_j_a = t as i32;
                    self.var_byte_e = 4;
                    self.face_toward(target);
                }
            } else if self.var_j_a != -1 && self.var_byte_y != 2 {
                self.var_int_arr_j[0] = -1;
                self.var_j_a = -1;
                self.var_byte_e = 0;
            }
        } else if self.var_j_a != -1 {
            self.var_j_a = -1;
            self.var_byte_e = 0;
        }
        true
    }

    /// `h.c(j j2, boolean bl)` — the spell/cast path. A creature (`var_byte_t ==
    /// 1`) spawns the melee-swing effect (kind 11, remapped by facing) and
    /// returns. An armed caster pays the level-tier fatigue cost from its weapon
    /// row (`bl` gates on insufficient fatigue; the tick calls with `bl = false`,
    /// so fatigue can go **negative** — faithful) and dispatches on the row's
    /// type (`[2]`): `0`/`1`/`5` = timed L/N/H self-buffs (G duration, status
    /// icon, re-attached kind-9 effect); `2` = summon (replace the old summon,
    /// spawn a scamp at the caster via the `b` spawner — emitted as
    /// [`WorldEvent`]s, deferral-safe: the summon is same-faction so the
    /// fall-through AoE skips it either way) falling through into `4` = AoE
    /// poison ([`crate::combat::apply_poison`] on every enemy within `[14]`);
    /// `6` = cure own poison; `3` = by `[1]`: 61618 AoE direct damage
    /// ([`crate::combat::apply_spell_damage`]), 61619 self-heal, else a kind-0
    /// projectile in the facing direction. Ends with the `h.f` recompute.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cast(
        &mut self,
        idx: usize,
        actors: &mut [Option<Actor>],
        effects: &mut Effects,
        tables: &mut Tables,
        rng: &mut JavaRandom,
        bl: bool,
        events: &mut Vec<WorldEvent>,
    ) {
        if self.var_byte_t == 1 {
            if self.var_byte_c == 1 {
                self.var_short_a = 500;
                self.var_byte_e = 7;
            }
            effects.spawn_actor(11, i32::from(self.var_byte_d), self, 0);
            return;
        }
        // Java dereferences var_int_arr_l unconditionally past this point.
        let w = self
            .var_int_arr_l
            .clone()
            .expect("cast (h.c) requires an equipped weapon/spell row");
        let lvl = i32::from(self.var_byte_o);
        let n;
        if lvl >= w[10] {
            if i32::from(self.var_short_r) < w[13] && bl {
                return;
            }
            n = w[5];
            self.var_short_r = (i32::from(self.var_short_r) - w[13]) as i16;
        } else if lvl >= w[9] {
            if i32::from(self.var_short_r) < w[12] && bl {
                return;
            }
            n = w[4];
            self.var_short_r = (i32::from(self.var_short_r) - w[12]) as i16;
        } else {
            if i32::from(self.var_short_r) < w[11] && bl {
                return;
            }
            n = w[3];
            self.var_short_r = (i32::from(self.var_short_r) - w[11]) as i16;
        }
        match w[2] {
            0 => {
                self.g_field = w[6] as i16;
                self.l_bonus = n as i16;
                self.var_byte_w = -48;
                effects.clear(i32::from(self.var_byte_h));
                self.var_byte_h = effects.spawn_actor(9, 0, self, 5000) as i8;
            }
            1 => {
                self.g_field = w[6] as i16;
                self.n_bonus = n as i16;
                self.var_byte_w = -50;
                effects.clear(i32::from(self.var_byte_h));
                self.var_byte_h = effects.spawn_actor(9, 0, self, 5000) as i8;
            }
            5 => {
                self.g_field = w[6] as i16;
                self.h_field = (n + 100) as i16;
                self.var_byte_w = -48;
                effects.clear(i32::from(self.var_byte_h));
                self.var_byte_h = effects.spawn_actor(9, 0, self, 5000) as i8;
            }
            2 => {
                // The summon (h.c:1524): replace the previous summon (`b.a(slot)`
                // on `var_j_c.var_byte_c - 1`), spawn a scamp at the caster's
                // position from its own spawn row (`b.var_b_a.a("/oh_scamp.cml",
                // pos, var_int_arr_o)`), link master<->summon, clear its loot
                // flag (`h.b(j,false)` — done by the world's Summon handler) —
                // then FALL THROUGH into the type-4 AoE (no break in Java).
                if self.var_j_c != -1 {
                    // Java reads the old summon's var_byte_c - 1; our link
                    // already stores the slot.
                    events.push(WorldEvent::RemoveActor(self.var_j_c as usize));
                }
                events.push(WorldEvent::Summon {
                    caster: idx,
                    x: self.var_int_arr_b[0],
                    y: self.var_int_arr_b[1],
                });
                self.aoe_poison(idx, actors, effects, n, w[6], w[14], tables, events, rng);
            }
            4 => {
                self.aoe_poison(idx, actors, effects, n, w[6], w[14], tables, events, rng);
            }
            6 => {
                effects.spawn_actor(8, 0, self, 0);
                self.var_short_k = 0;
                self.var_short_l = 0;
                self.var_byte_w = -1;
            }
            3 => {
                if w[1] == 61618 {
                    // AoE direct damage on every enemy within range [14].
                    for i in 0..actors.len() {
                        let hit = match &actors[i] {
                            // Self is taken out of the array (the `== j2` skip).
                            Some(a) => {
                                a.var_byte_r != self.var_byte_r
                                    && crate::combat::combat_distance(
                                        &self.var_int_arr_b,
                                        &a.var_int_arr_b,
                                    ) <= w[14]
                            }
                            None => false,
                        };
                        if hit {
                            let mut victim = actors[i].take().unwrap();
                            crate::combat::apply_spell_damage(
                                self,
                                idx,
                                &mut victim,
                                actors,
                                n,
                                effects,
                                tables,
                                events,
                                rng,
                            );
                            actors[i] = Some(victim);
                        }
                    }
                } else if w[1] == 61619 {
                    effects.spawn_actor(8, 0, self, 0);
                    self.var_short_q = i32::from(self.var_short_o)
                        .min(i32::from(self.var_short_q) + n.abs())
                        as i16;
                } else {
                    effects.spawn_actor(0, i32::from(self.var_byte_d), self, 0);
                }
            }
            _ => {}
        }
        self.class_progression(tables);
    }

    /// The weapon-type-4 AoE body (shared with the type-2 fallthrough): poison
    /// every enemy within `range` (`h.a(j2, actor, n, row[6])` per victim).
    #[allow(clippy::too_many_arguments)]
    fn aoe_poison(
        &mut self,
        idx: usize,
        actors: &mut [Option<Actor>],
        effects: &mut Effects,
        n: i32,
        duration: i32,
        range: i32,
        tables: &mut Tables,
        events: &mut Vec<WorldEvent>,
        rng: &mut JavaRandom,
    ) {
        #[allow(clippy::needless_range_loop)]
        for slot_idx in 0..actors.len() {
            let hit = match &actors[slot_idx] {
                // Self is taken out of the array (the `== j2` skip).
                Some(a) => {
                    a.var_byte_r != self.var_byte_r
                        && crate::combat::combat_distance(&self.var_int_arr_b, &a.var_int_arr_b)
                            <= range
                }
                None => false,
            };
            if hit {
                let mut victim = actors[slot_idx].take().unwrap();
                crate::combat::apply_poison(
                    self,
                    idx,
                    &mut victim,
                    n,
                    duration,
                    effects,
                    tables,
                    events,
                    rng,
                );
                actors[slot_idx] = Some(victim);
            }
        }
    }

    /// `h.boolean_c(j)` — the summoner vanish/teleport-wander. Phase `A == 0`:
    /// if the player (slot 0) is alive, puff at the current position, cancel the
    /// move target, teleport to `(-10000, -10000)`, and enter phase 1. Phase
    /// `A == 1`: once `var_short_n <= -1000` (a beat after the y==2 countdown
    /// runs out) and the player is alive, roll up to 100 candidate positions
    /// near the player (2 RNG draws each; a candidate needs its 5-cell plus
    /// shape open on the collision layer and non-empty on the base layer),
    /// teleport there (even after 100 failures — faithful), puff at the new
    /// position, and on success clear the phase and return `true` (the caller
    /// then re-arms `var_short_n`).
    fn wander(
        &mut self,
        actors: &[Option<Actor>],
        map: &crate::world::MapRef,
        effects: &mut Effects,
        rng: &mut JavaRandom,
    ) -> bool {
        const OFFS: [[i32; 2]; 5] = [[-1, 0], [0, -1], [0, 0], [0, 1], [1, 0]];
        // Java reads b.var_j_arr_a[0] (the player) unconditionally.
        debug_assert!(
            !actors.is_empty() && actors[0].is_some(),
            "wander (h.boolean_c) needs the player at slot 0"
        );
        let player = actors[0].as_ref().unwrap();
        if self.a_phase == 1 {
            if self.var_short_n <= -1000 && player.var_byte_q == 0 {
                let (mut n, mut n2) = (0, 0);
                let mut ok = false;
                let mut k = 0;
                while k < 100 && !ok {
                    ok = true;
                    n = (player.var_int_arr_b[0] + rng.next_int() % 500).abs();
                    n2 = (player.var_int_arr_b[1] + rng.next_int() % 500).abs();
                    let n4 = n >> 7;
                    let n5 = n2 >> 7;
                    for off in OFFS {
                        let n6 = (n4 + off[0]) * map.height + n5 + off[1];
                        if n6 < 0 || n6 as usize >= map.base.len() {
                            ok = false;
                            continue; // out of bounds: keep checking offsets
                        }
                        if map.coll[n6 as usize] == 0 && map.base[n6 as usize] != 0 {
                            continue; // open cell: next offset
                        }
                        ok = false;
                        break; // blocked: next attempt (Java `continue block0`)
                    }
                    k += 1;
                }
                crate::world::set_position(self, n, n2);
                effects.spawn_world(8, self.var_int_arr_b[0], self.var_int_arr_b[1], 0);
                if ok {
                    self.a_phase = 0;
                    return true;
                }
            }
        } else if player.var_byte_q == 0 {
            effects.spawn_world(8, self.var_int_arr_b[0], self.var_int_arr_b[1], 0);
            self.var_int_arr_j[0] = -1;
            self.var_int_arr_j[1] = -1;
            crate::world::set_position(self, -10000, -10000);
            self.a_phase = 1;
        }
        false
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
        Actor::tick(
            0,
            &mut arr,
            &mut rng,
            l,
            false,
            model,
            &mut t.clone(),
            fx,
            None,
            &mut Vec::new(),
        );
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
            &mut tables.clone(),
            &mut Effects::new(),
            None,
            &mut Vec::new(),
        );
        let v = arr[1].as_ref().unwrap();
        assert!(v.var_short_q < before, "DoT should reduce HP");
        assert_eq!(v.var_short_l, 1000, "lap timer resets to 1000");
        assert!(v.var_short_k < 5000, "DoT duration counts down");
    }

    #[test]
    fn tick_npc_ai_approaches_engages_and_attacks() {
        let tables = Tables::default();
        // Aggressive NPC (slot 1) vs an enemy NPC (slot 2). Numeric parity with
        // the real h.boolean_b is established by `oracle_match::ai_matches_oracle`.
        let me = Actor {
            var_byte_c: 2, // slot 1
            var_byte_r: 2,
            var_short_q: 100,
            var_short_o: 100,
            var_short_w: 800, // walk speed
            var_short_s: 40,  // strength: base damage 20 (so a landed hit isn't
            var_byte_i: 10,   // absorbed to a no-text Miss)
            e_field: 500,     // aggro range
            f_field: 60,      // attack range
            var_int_e: 900,   // near the 1000ms attack period
            var_int_arr_b: [1000, 1000],
            var_int_arr_c: [1010, 1005],
            var_int_arr_d: [1005, 1010],
            ..Default::default()
        };
        let enemy = Actor {
            var_byte_c: 3, // slot 2
            var_byte_r: 1,
            var_short_q: 10_000, // survivable
            var_short_o: 10_000,
            var_int_arr_b: [1300, 1000], // distance 300: inside E, outside F
            ..Default::default()
        };
        let mut arr = vec![None, Some(me), Some(enemy)];
        let mut rng = JavaRandom::new(7);
        // Out of attack range: the AI issues a 20-unit move toward the enemy and
        // short-circuits the melee gate (boolean_b returns false; no cooldown reset).
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            200,
            false,
            None,
            &mut tables.clone(),
            &mut Effects::new(),
            None,
            &mut Vec::new(),
        );
        {
            let a = arr[1].as_ref().unwrap();
            assert_eq!(a.var_int_arr_j, [1020, 1000], "approach: +20 move target");
            assert_eq!(a.var_j_a, -1, "no target lock while approaching");
            assert!(a.var_int_e > 900, "cooldown keeps accumulating");
        }
        // Teleport into attack range (clearing the pending move so the move-to
        // step doesn't walk us back out first): the AI locks the target, enters
        // the attack state, and (cooldown elapsed) strikes — and the cooldown
        // resets either way.
        {
            let a = arr[1].as_mut().unwrap();
            a.var_int_arr_b = [1270, 1000];
            a.var_int_arr_j = [-1, -1];
        }
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            200,
            false,
            None,
            &mut tables.clone(),
            &mut Effects::new(),
            None,
            &mut Vec::new(),
        );
        let a = arr[1].as_ref().unwrap();
        assert_eq!(a.var_j_a, 2, "target locked to slot 2");
        assert_eq!(a.var_byte_e, 4, "attack animation state");
        assert_eq!(a.var_int_e, 0, "cooldown reset after the strike gate");
        let t = arr[2].as_ref().unwrap();
        assert_eq!(
            t.var_j_a, 1,
            "aggressor back-ref set to the attacker's slot"
        );
        assert!(t.floating_text.is_some(), "combat sets the floating text");
    }

    #[test]
    fn tick_creature_and_caster_attacks_take_the_spell_path() {
        let tables = Tables::default();
        // A creature (t=1) with a locked target and elapsed cooldown spawns the
        // facing-remapped melee-swing effect (kind 11 + facing 1 -> 12). Numeric
        // parity is established by `oracle_match::cast_matches_oracle`.
        let creature = Actor {
            var_byte_c: 2, // slot 1
            var_byte_r: 2,
            var_byte_t: 1,
            var_byte_d: 1, // facing
            var_short_q: 100,
            var_short_o: 100,
            e_field: 500,
            f_field: 60,
            var_int_e: 900,
            var_j_a: 2,
            var_int_arr_b: [1000, 1000],
            ..Default::default()
        };
        let enemy = Actor {
            var_byte_c: 3, // slot 2
            var_byte_r: 1,
            var_short_q: 10_000,
            var_short_o: 10_000,
            var_int_arr_b: [1030, 1000],
            ..Default::default()
        };
        let mut arr = vec![None, Some(creature), Some(enemy.clone())];
        let mut rng = JavaRandom::new(1);
        let mut fx = Effects::new();
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            200,
            false,
            None,
            &mut tables.clone(),
            &mut fx,
            None,
            &mut Vec::new(),
        );
        // Effect [+0] = 0xFFFFF000 | var_byte_c << 8 | 12 (kind 11 remapped by dir 1).
        assert_eq!(
            i32::from(fx.raw()[0]) & 0xFF,
            12,
            "swing effect kind remapped by facing"
        );
        assert_eq!(arr[1].as_ref().unwrap().var_int_e, 0, "cooldown reset");
        assert_eq!(
            arr[2].as_ref().unwrap().var_short_q,
            10_000,
            "spell branch returns false: no melee damage"
        );

        // An armed caster (weapon type 0 = L-buff): pays the tier-0 fatigue cost
        // (bl=false lets it go negative), sets G/L/w, and attaches the kind-9
        // effect into var_byte_h. The cast's trailing h.f needs a race row.
        let mut tables = Tables::default();
        tables.insert(4, vec![vec![0, 0, 0, 5]]);
        let caster = Actor {
            var_byte_c: 2,
            var_byte_r: 2,
            var_short_q: 100,
            var_short_o: 100,
            var_short_r: 3, // fatigue below the cost of 5 — still casts under bl=false
            var_byte_o: 1,  // level < row[9]=5 -> tier 0: power row[3], cost row[11]
            e_field: 500,
            f_field: 60,
            var_int_e: 900,
            var_j_a: 2,
            var_int_arr_b: [1000, 1000],
            var_int_arr_n: [-1; 8], // empty inventory (h.f skips the item rows)
            //                       [0][1][2][3] [4] [5] [6]  [7][8][9][10][11][12][13][14]
            var_int_arr_l: Some(vec![0, 0, 0, 7, 14, 21, 900, 0, 0, 5, 10, 5, 8, 12, 200]),
            ..Default::default()
        };
        let mut arr = vec![None, Some(caster), Some(enemy)];
        let mut fx = Effects::new();
        Actor::tick(
            1,
            &mut arr,
            &mut rng,
            200,
            false,
            None,
            &mut tables.clone(),
            &mut fx,
            None,
            &mut Vec::new(),
        );
        let a = arr[1].as_ref().unwrap();
        assert_eq!(
            a.var_short_r, -2,
            "fatigue 3 - cost 5 goes negative under bl=false"
        );
        // The cast sets G = row[6] = 900; the same tick's later G-buff branch
        // (h.a:478) then decrements it by this frame's l — as the original does.
        assert_eq!(
            a.g_field, 700,
            "buff duration from row[6], minus this frame"
        );
        assert_eq!(a.l_bonus, 7, "tier-0 power from row[3]");
        assert_eq!(a.var_byte_w, -48, "status icon");
        assert_eq!(a.var_byte_h, 0, "kind-9 effect attached at slot 0");
        assert_eq!(fx.raw()[7], 5000, "attached effect lifetime");
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
            &mut tables.clone(),
            &mut Effects::new(),
            None,
            &mut Vec::new(),
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
            &mut tables.clone(),
            &mut Effects::new(),
            None,
            &mut Vec::new(),
        );
        assert!(
            arr[1].is_none(),
            "dead NPC removed at the 250ms corpse threshold"
        );
    }
}
