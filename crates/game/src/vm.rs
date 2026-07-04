//! The `e.java` script runtime as the shell sees it: the per-frame execute
//! gate around the M7 opcode decoder (`formats::vm::ScriptVm`), the section
//! tables, and the `e.b(long)` prologue guards. Transcribed from the CFR
//! decompile of `e.java`.
//!
//! Per frame (`run()` step 4 calls `e.a(J)` = one `b(J)` execute), the
//! prologue in order:
//! - nothing runs with an empty call stack (`var_int_a == 0`);
//! - the **key gate** (`e.var_boolean_a`, set by op60) halts execution until a
//!   key is fed via `e.b(char)` — which CLEARS the gate and CONSUMES the key;
//! - an **open dialogue** (`b.var_boolean_g`) halts execution;
//! - a pending **post-dialogue facing reset** (`var_byte_d >= 0`) fires
//!   (`h.b(actor, 0)`) once executable again;
//! - the **shop block** (`b.boolean_a()` = mode 1) halts;
//! - the **wait timer** (op11) accumulates dt and halts until elapsed;
//! - the **actor wait-list** (op21, `var_int_arr_e`) halts until every listed
//!   actor's walk target is cleared (or its slot empty);
//! - the **cutscene walk sequencer** (op52, `var_int_arr_h` + phase
//!   `var_byte_c`) runs its own three-phase state machine and halts;
//! - then exactly ONE opcode executes.
//!
//! Loading (`b.a(String)` -> `e.void_a(String)`): parse the `.scr`, merge its
//! sections into the persistent tables (`e.void_a()` clears only the exec
//! state and the subtype-10 loot list; the other tables and the class table
//! persist — startup.scr's rows survive every later load), take the one-time
//! pristine snapshot of the subtype-0 stat rows (`var_boolean_b`), overwrite
//! the string pool prefix, reset the execution state, push entry 1.
//!
//! The `b`-coupled side effects are NOT applied here — [`GameVm::tick`]
//! returns the decoded step and the shell (which owns the `b` state) applies
//! them; unhandled opcodes are a loud error, never skipped.

use crate::world::{ModelCache, World};
use formats::scr::{parse_scr, ScrProgram};
use formats::vm::{ScriptVm, Step};
use formats::Tables;

/// `b.a:I` idles at this sentinel (recon: -286331154 = 0xEEEEEEEE).
pub const KEY_SENTINEL: i32 = -286331154;

/// The e-table geometry (`e.java` field initializers): rows × cols per
/// subtype, materialized zero-filled so section rows land at their index.
const TABLE_DIMS: &[(u8, usize, usize)] = &[
    (0, 25, 21), // var_int_arr_arr_a (actor stat rows; working copy)
    (1, 42, 10), // var_int_arr_arr_d (armor)
    (2, 11, 14), // var_int_arr_arr_e (consumables)
    (4, 37, 8),  // var_int_arr_arr_c (weapons)
    (5, 9, 15),  // var_int_arr_arr_h (classes)
    (6, 25, 7),  // var_int_arr_arr_g
    (8, 10, 15), // k (spells)
    (9, 10, 21), // var_int_arr_arr_f (spawn rows)
    (10, 30, 4), // l (loot; cleared per load)
];

/// The op58 growth table (`e.var_byte_arr_arr_a`, the static initializer).
const GROWTH: [[i32; 7]; 3] = [
    [3, 1, 1, 2, 0, 1, 1],
    [2, 2, 2, 1, 0, 1, 1],
    [1, 3, 3, 1, 0, 2, 3],
];

/// The op52 cutscene walk sequencer state (`var_int_arr_h` + `var_byte_a/b/c`).
#[derive(Debug, Clone, Copy)]
struct Seq {
    target: [i32; 2], // var_int_arr_h
    actor: i32,       // var_byte_b
    axis: i32,        // var_byte_a (0/1 = first-axis lock via the actor's x/y)
    phase: u8,        // var_byte_c
}

pub struct GameVm {
    exec: Option<ScriptVm>,
    /// `e.var_boolean_a` — the op60 wait-for-key gate.
    pub key_gate: bool,
    /// `e.var_int_c` / `e.var_int_b` — wait target ms (-1 = none) + accumulator.
    wait_target: i32,
    wait_acc: i32,

    /// The working section tables (subtype-keyed, index-addressed) + the
    /// subtype-5 aux lists — shared with the actor layer as [`Tables`].
    pub tables: Tables,
    /// `var_int_arr_arr_b` — the pristine subtype-0 snapshot (op67 restores).
    pristine0: Vec<Vec<i32>>,
    /// `var_boolean_b` — the snapshot is taken once, after the FIRST load.
    snapshot_pending: bool,
    /// `var_java_lang_String_arr_a` — the inline-string pool. Loads overwrite
    /// the prefix; stale tails persist (faithful).
    pub strings: Vec<String>,
    /// `var_int_d` — the CURRENT script's string count (the reverse-lookup
    /// scan bound; stale tails beyond it are invisible to `int_a(String)`).
    pool_fill: usize,
    /// `var_int_arr_f` — the subtype-7 flat list (`[0] = -1` on reset).
    pub flat7: Vec<i32>,
    /// `var_int_arr_g` — the subtype-9 global slot list.
    pub slots9: Vec<i32>,

    /// `var_int_arr_c[3..=7]` — the op14 key/event handler entries (-1 unset).
    handlers: [i32; 8],
    /// `var_int_arr_e` — the op21 actor wait-list.
    wait_actors: Option<Vec<i32>>,
    /// The op52 sequencer.
    seq: Option<Seq>,
    /// `e.var_byte_d` — the post-dialogue facing reset actor (op53).
    face_reset: i8,
}

impl Default for GameVm {
    fn default() -> Self {
        Self::new()
    }
}

impl GameVm {
    pub fn new() -> Self {
        let mut tables = Tables::default();
        for &(subtype, rows, cols) in TABLE_DIMS {
            tables.insert(subtype, vec![vec![0i32; cols]; rows]);
        }
        tables.insert_class_aux(vec![vec![0; 15]; 9], vec![vec![0; 15]; 9]);
        Self {
            exec: None,
            key_gate: false,
            wait_target: -1,
            wait_acc: 0,
            tables,
            pristine0: vec![vec![0i32; 21]; 25],
            snapshot_pending: true,
            strings: Vec::new(),
            pool_fill: 0,
            flat7: {
                let mut v = vec![0i32; 100];
                v[0] = -1;
                v
            },
            slots9: vec![0i32; 10],
            handlers: [-1; 8],
            wait_actors: None,
            seq: None,
            face_reset: -1,
        }
    }

    /// `e.void_a(String)` body (after `b.int_a` read the bytes): the exec-state
    /// reset (`e.void_a()`), section merge, one-time pristine snapshot, string
    /// pool prefix overwrite, push entry 1.
    pub fn load(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let program: ScrProgram = parse_scr(bytes)?;
        // e.void_a() reset: exec state, handlers, flat7[0], the subtype-10
        // table; the other section tables persist.
        self.key_gate = false;
        self.wait_target = -1;
        self.wait_acc = 0;
        self.handlers = [-1; 8];
        self.flat7 = {
            let mut v = vec![0i32; 100];
            v[0] = -1;
            v
        };
        self.slots9 = vec![0i32; 10];
        self.wait_actors = None;
        self.seq = None;
        self.face_reset = -1;
        for row in self
            .tables
            .row_iter_mut(10)
            .expect("subtype-10 table always materialized")
        {
            row.fill(0);
        }
        // String pool: overwrite the prefix, keep any stale tail (faithful —
        // Java only writes var_java_lang_String_arr_a[0..new_count]).
        for (i, s) in program.strings.iter().enumerate() {
            if i < self.strings.len() {
                self.strings[i] = s.clone();
            } else {
                self.strings.push(s.clone());
            }
        }
        self.pool_fill = program.strings.len();
        // Section merge (arraycopy row -> table[index]).
        let mut aux_i: Option<Vec<(usize, Vec<i32>)>> = None;
        let mut aux_j: Option<Vec<(usize, Vec<i32>)>> = None;
        for sec in &program.sections {
            match sec.subtype {
                7 => {
                    // walk_g: the flat pair list, -1 terminated.
                    for (i, v) in sec.aux_a.iter().enumerate() {
                        self.flat7[i] = *v;
                    }
                }
                9 => {
                    self.merge_row(9, sec.index, &sec.fields)?;
                    // tag-20 globals append to var_int_arr_g (var_int_e resets
                    // per load, so indexes restart at 0).
                    for (i, v) in sec.aux_a.iter().enumerate() {
                        self.slots9[i] = *v;
                    }
                }
                5 => {
                    self.merge_row(5, sec.index, &sec.fields)?;
                    aux_i
                        .get_or_insert_with(Vec::new)
                        .push((sec.index, sec.aux_a.clone()));
                    aux_j
                        .get_or_insert_with(Vec::new)
                        .push((sec.index, sec.aux_b.clone()));
                }
                s => self.merge_row(s, sec.index, &sec.fields)?,
            }
        }
        // Subtype-5 aux lists land in e.i / e.j at the class index, -1
        // terminated inside a 15-wide row.
        if aux_i.is_some() || aux_j.is_some() {
            let mut i_rows = self.tables.class_aux_i_rows();
            let mut j_rows = self.tables.class_aux_j_rows();
            for (idx, list) in aux_i.unwrap_or_default() {
                let row = &mut i_rows[idx];
                *row = vec![0; 15];
                for (n, v) in list.iter().enumerate() {
                    row[n] = *v;
                }
            }
            for (idx, list) in aux_j.unwrap_or_default() {
                let row = &mut j_rows[idx];
                *row = vec![0; 15];
                for (n, v) in list.iter().enumerate() {
                    row[n] = *v;
                }
            }
            self.tables.insert_class_aux(i_rows, j_rows);
        }
        // One-time pristine snapshot of the subtype-0 working table.
        if self.snapshot_pending {
            self.pristine0 = self.tables.rows(0).to_vec();
            self.snapshot_pending = false;
        }
        let mut exec = ScriptVm::new(&program);
        anyhow::ensure!(exec.start_entry(1), "script has no entry 1");
        self.exec = Some(exec);
        Ok(())
    }

    fn merge_row(&mut self, subtype: u8, index: usize, fields: &[i32]) -> anyhow::Result<()> {
        let row = self
            .tables
            .row_mut(subtype, index as i32)
            .ok_or_else(|| anyhow::anyhow!("subtype {subtype} row {index} out of range"))?;
        for (dst, src) in row.iter_mut().zip(fields) {
            *dst = *src;
        }
        Ok(())
    }

    /// `e.java_lang_String_a(int)` — resolve a text value: `0xF___` = lang id
    /// (the caller resolves), else a string-pool index.
    pub fn pool_string(&self, idx: i32) -> &str {
        self.strings
            .get(idx as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// The pool half of `e.int_a(String)` — an exact-match scan over the
    /// CURRENT script's pool entries (`0..var_int_d`); the caller falls back
    /// to the lang reverse lookup (`0xF000 | id`).
    pub fn pool_reverse(&self, s: &str) -> Option<i32> {
        self.strings[..self.pool_fill.min(self.strings.len())]
            .iter()
            .position(|v| v == s)
            .map(|i| i as i32)
    }

    /// Class-select rows: `l()` walks `e.h:[[I` taking rows with col0 > 0 and
    /// resolves col1 (0xF000-marked = lang id). Local-string indices are out
    /// of slice (startup.scr's classes are all lang refs).
    pub fn class_name_ids(&self) -> Vec<u16> {
        self.tables
            .rows(5)
            .iter()
            .filter(|row| row[0] > 0)
            .map(|row| {
                let v = row[1];
                assert!(
                    (v & 0xF000) == 0xF000,
                    "class name via local script string not ported (out of slice): {v}"
                );
                (v & 0xFFF) as u16
            })
            .collect()
    }

    /// op67 — restore a subtype-0 working row from the pristine snapshot.
    pub fn restore_row(&mut self, idx: i32) {
        let pristine = self.pristine0[idx as usize].clone();
        *self.tables.row_mut(0, idx).expect("subtype-0 row") = pristine;
    }

    /// op58 — level-scale a subtype-0 working row: `row[2] = scale` and
    /// `row[3..=9] += scale * GROWTH[row[17]][col-3]`.
    pub fn scale_row(&mut self, idx: i32, scale: i32) {
        let row = self.tables.row_mut(0, idx).expect("subtype-0 row");
        row[2] = scale;
        let kind = row[17] as usize;
        for col in 3..=9 {
            row[col] += scale * GROWTH[kind][col - 3];
        }
    }

    /// op14 / op28 — set / clear a key-event handler entry (`var_int_arr_c`,
    /// selector 0..=4 maps to actions 3..=7).
    pub fn set_handler(&mut self, selector: i32, entry: Option<i32>) {
        self.handlers[(selector + 3) as usize] = entry.unwrap_or(-1);
    }

    /// op21 — arm the actor wait-list.
    pub fn set_wait_actors(&mut self, list: Vec<i32>) {
        self.wait_actors = Some(list);
    }

    /// op52 — arm the cutscene walk sequencer.
    pub fn set_sequencer(&mut self, actor: i32, axis: i32, target: [i32; 2]) {
        self.seq = Some(Seq {
            target,
            actor,
            axis,
            phase: 0,
        });
    }

    /// op53 — remember the actor whose facing resets after the dialogue.
    pub fn set_face_reset(&mut self, actor: i32) {
        self.face_reset = actor as i8;
    }

    /// `e.void_a(int)` — push a script entry (op23 nesting, event handlers,
    /// death triggers).
    pub fn push_entry(&mut self, entry: u8) {
        if let Some(exec) = self.exec.as_mut() {
            exec.start_entry(entry);
        }
    }

    /// `e.b(char)` (CFR `void_b(int)`) — the `run()` input tail's key feed.
    /// Returns `true` if the key released the op60 gate (the caller must then
    /// consume the latched key: `b.a:I = sentinel`). Otherwise a *remapped*
    /// action (3..=7) with an armed op14 handler pushes that entry (one-shot).
    pub fn feed_key(&mut self, remapped: i32) -> bool {
        if self.key_gate {
            self.key_gate = false;
            return true;
        }
        if (3..=7).contains(&remapped) && self.handlers[remapped as usize] >= 0 {
            let entry = self.handlers[remapped as usize];
            self.handlers[remapped as usize] = -1;
            self.push_entry(entry as u8);
        }
        false
    }

    /// One `e.a(J)` tick: run the prologue guards against the world, then
    /// execute one opcode and return the decoded step (`None` when halted).
    /// op11 waits, op60, op2 returns, and the local e-state opcodes (14/21/28/
    /// 52/58/67) are applied internally and still surfaced for tracing; the
    /// `b`-side opcodes are the caller's to apply.
    pub fn tick(
        &mut self,
        dt_ms: i32,
        world: &mut World,
        models: &mut ModelCache,
        shop_block: bool,
    ) -> Option<Step> {
        let exec = self.exec.as_mut()?;
        if !exec.running() {
            return None;
        }
        if self.key_gate {
            return None;
        }
        // b.var_boolean_g — an open dialogue halts the VM.
        if world.dialogue.is_some() {
            return None;
        }
        // The post-dialogue facing reset (h.b(actor, 0)).
        if self.face_reset >= 0 {
            if let Some(a) = world.actors[self.face_reset as usize].as_mut() {
                a.set_facing(0);
            }
            self.face_reset = -1;
        }
        // b.boolean_a() — the shop (mode 1) halts the VM.
        if shop_block {
            return None;
        }
        // op11 wait.
        if self.wait_target >= 0 {
            self.wait_acc += dt_ms;
            if self.wait_acc >= self.wait_target {
                self.wait_target = -1;
            } else {
                return None;
            }
        }
        // op21 actor wait-list: halt while any listed actor still has a walk
        // target (slot empty or target cleared passes).
        if let Some(list) = &self.wait_actors {
            for &slot in list {
                if let Some(a) = world.actors[slot as usize].as_ref() {
                    if a.var_int_arr_j[0] != -1 {
                        return None;
                    }
                }
            }
            self.wait_actors = None;
        }
        // op52 sequencer: three phases, each ending the tick.
        if self.seq.is_some() {
            self.seq_step(world, models);
            return None;
        }

        let step = self
            .exec
            .as_mut()
            .unwrap()
            .step()
            .expect("script decode error");
        match step.opcode {
            11 => {
                self.wait_target = step.operands[0];
                self.wait_acc = 0;
            }
            60 => self.key_gate = true,
            _ => {}
        }
        Some(step)
    }

    /// The op52 sequencer body (`e.b(long)` at the `var_int_arr_h` guard):
    /// phase 0 locks input, follows the actor, walks the first axis at anim 2 /
    /// speed 900; phase 1 waits for arrival then walks to the target at anim 3 /
    /// speed 400; phase 2 waits for arrival then unlocks input and disarms.
    fn seq_step(&mut self, world: &mut World, _models: &mut ModelCache) {
        let mut seq = self.seq.unwrap();
        let slot = seq.actor as usize;
        match seq.phase {
            0 => {
                world.input_unlocked = false; // b.a(false)
                world.camera_follow(seq.actor); // b.b(var_byte_b)
                let crate::world::World {
                    actors,
                    actor_anims: anims,
                    ..
                } = world;
                let a = actors[slot].as_mut().expect("sequencer actor");
                if seq.axis == 0 || seq.axis == 1 {
                    let x = a.var_int_arr_b[0];
                    a.set_walk_target(x, seq.target[1]);
                } else {
                    let y = a.var_int_arr_b[1];
                    a.set_walk_target(seq.target[0], y);
                }
                a.set_anim(2, anims[slot].as_mut());
                a.attr_set(7, 900, &self.tables);
                seq.phase = 1;
                self.seq = Some(seq);
            }
            1 => {
                let crate::world::World {
                    actors,
                    actor_anims: anims,
                    ..
                } = world;
                let a = actors[slot].as_mut().expect("sequencer actor");
                if a.var_int_arr_j[0] != -1 {
                    return;
                }
                a.set_anim(3, anims[slot].as_mut());
                a.attr_set(7, 400, &self.tables);
                a.set_walk_target(seq.target[0], seq.target[1]);
                seq.phase = 2;
                self.seq = Some(seq);
            }
            2 => {
                let a = world.actors[slot].as_ref().expect("sequencer actor");
                if a.var_int_arr_j[0] != -1 {
                    return;
                }
                world.input_unlocked = true; // b.a(true) (+ key consume)
                world.consume_key = true;
                self.seq = None;
            }
            _ => unreachable!(),
        }
    }
}
