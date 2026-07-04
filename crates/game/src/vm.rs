//! The `e.java` script runtime as the shell sees it: the per-frame execute
//! gate around the M7 opcode decoder (`formats::vm::ScriptVm`), transcribed
//! from `e.b(long)`'s prologue and the load path `e.void_a(String)` (CFR).
//!
//! Per frame (`run()` step 4 calls `e.a(J)` = one `b(J)` execute):
//! - nothing runs with an empty call stack (`var_int_a == 0`);
//! - the **key gate** (`var_boolean_a`, set by op60) halts execution until a
//!   key is fed via `e.b(char)` (the `run()` input tail) — which CLEARS the
//!   gate and CONSUMES the key (`b.a:I = sentinel`);
//! - the **wait timer** (op11: `var_int_c = ms; var_int_b = 0`) accumulates dt
//!   and halts until elapsed;
//! - then exactly ONE opcode executes.
//!
//! Loading (`b.a(String)` -> `e.void_a(String)`): parse the `.scr`, MERGE its
//! subtype-5 sections into the persistent 9x15 class table (`e.h:[[I` — the
//! reset `e.void_a()` does NOT clear the section tables, so startup.scr's
//! classes survive startup2.scr's load and feed the class-select menu), reset
//! the execution state, push entry 1. op29 re-enters this loader mid-frame,
//! discarding the rest of the calling script (the old stack is reset — the
//! trailing ops after op29 in startup.scr never run).
//!
//! The `b`-coupled side effects (op10/op12/op43/op44/op56/op61/...) are NOT
//! applied here — [`GameVm::tick`] returns the decoded step and the shell
//! (which owns the `b` state) applies them; unhandled opcodes are a loud
//! error, never skipped.
//!
//! Out-of-slice fences (explicit): the actor-wait prologue guards
//! (`var_int_arr_e` anim-waits, `var_int_arr_h` scripted moves,
//! `b.var_boolean_g`, `b.boolean_a()` dialog-open) never trigger on the
//! front-end path and are not modeled; event-callback entries
//! (`var_int_arr_c`, fed by `e.b(char)` when un-gated) are asserted unarmed.

use formats::scr::{parse_scr, ScrProgram};
use formats::vm::{ScriptVm, Step};

/// `b.a:I` idles at this sentinel (recon: -286331154 = 0xEEEEEEEE).
pub const KEY_SENTINEL: i32 = -286331154;

pub struct GameVm {
    exec: Option<ScriptVm>,
    /// `e.var_boolean_a` — the op60 wait-for-key gate.
    pub key_gate: bool,
    /// `e.var_int_c` — wait target ms (-1 = no wait pending).
    wait_target: i32,
    /// `e.var_int_b` — accumulated wait ms.
    wait_acc: i32,
    /// `e.h:[[I` — the 9x15 class table, persistent across script loads.
    class_table: [[i32; 15]; 9],
}

impl Default for GameVm {
    fn default() -> Self {
        Self::new()
    }
}

impl GameVm {
    pub fn new() -> Self {
        Self {
            exec: None,
            key_gate: false,
            wait_target: -1,
            wait_acc: 0,
            class_table: [[0; 15]; 9],
        }
    }

    /// `e.void_a(String)` body (after `b.int_a` read the bytes): parse, merge
    /// section tables, reset exec state, push entry 1.
    pub fn load(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let program: ScrProgram = parse_scr(bytes)?;
        for sec in &program.sections {
            if sec.subtype == 5 {
                let row = self
                    .class_table
                    .get_mut(sec.index)
                    .ok_or_else(|| anyhow::anyhow!("class row {} out of range", sec.index))?;
                for (dst, src) in row.iter_mut().zip(&sec.fields) {
                    *dst = *src;
                }
            }
        }
        // e.void_a() reset: exec state only; section tables persist.
        self.key_gate = false;
        self.wait_target = -1;
        self.wait_acc = 0;
        let mut exec = ScriptVm::new(&program);
        anyhow::ensure!(exec.start_entry(1), "script has no entry 1");
        self.exec = Some(exec);
        Ok(())
    }

    /// Class-select rows: `l()` walks `e.h:[[I` taking rows with col0 > 0 and
    /// resolves col1 (`e.a(int)` — 0xF000-marked = lang id). Local-string
    /// indices are out of slice (startup.scr's classes are all lang refs).
    pub fn class_name_ids(&self) -> Vec<u16> {
        self.class_table
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

    /// `e.b(char)` (CFR `void_b(int)`) — the `run()` input tail's key feed.
    /// Returns `true` if the key released the op60 gate (the caller must then
    /// consume the latched key: `b.a:I = sentinel`).
    pub fn feed_key(&mut self, _remapped: i32) -> bool {
        if self.key_gate {
            self.key_gate = false;
            return true;
        }
        // event-callback entries (var_int_arr_c[3..=7]) are never armed by the
        // front-end scripts; modeled as a no-op (fence documented above).
        false
    }

    /// One `e.a(J)` tick: returns the executed step, or `None` when halted
    /// (empty stack / key gate / wait pending). op11 waits and op2 returns are
    /// applied internally and still surfaced for tracing.
    pub fn tick(&mut self, dt_ms: i32) -> Option<Step> {
        let exec = self.exec.as_mut()?;
        if !exec.running() {
            return None;
        }
        if self.key_gate {
            return None;
        }
        if self.wait_target >= 0 {
            self.wait_acc += dt_ms;
            if self.wait_acc >= self.wait_target {
                self.wait_target = -1;
            } else {
                return None;
            }
        }
        let step = exec.step().expect("script decode error");
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
}
