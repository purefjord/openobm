//! `.scr` bytecode VM (skeleton).
//!
//! Faithful port of the instruction fetch + operand decoding in
//! `e.java::b(long)` (the per-tick execute step, ~line 447) and its operand
//! readers `int_b()` (u8), `d()` (u16 BE), `c()` (u24 BE), and
//! `java_lang_String_c(len)` (inline string). Execution is a call-stack of
//! program counters (`var_int_arr_a`), one opcode dispatched per tick; opcode 2
//! returns (pops), opcode 23 calls (pushes an entry), opcode 11 waits.
//!
//! Scope (M3): this is a *skeleton*. It decodes every opcode's operands exactly
//! as the original does (so the program counter advances identically) and models
//! the control flow needed to walk a script to its **first visible action**, but
//! it does not execute the opcodes' side effects (rendering, actor mutation,
//! resource loading) — those reach into `b.java`/`h.java` and belong to later
//! milestones. Because the path from a script's entry to its first visible
//! action is free of game-state-dependent branches (the language expresses
//! branching via event-callback entries, not data-dependent GOTOs in the
//! bytecode), the decode order equals the runtime execution order over that
//! prefix, and the resulting opcode trace is validated against the original
//! algorithm by the oracle.

use crate::reader::ParseError;
use crate::scr::ScrProgram;

/// What a decoded opcode does to control flow / the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// Ordinary opcode with no control-flow or visible effect.
    Normal,
    /// Opcode 11: set a wait timer (treated as immediately elapsed for tracing).
    Wait,
    /// Opcode 23: call another script entry (the operand is the entry id).
    Call(u8),
    /// Opcode 2: return from the current entry (pops the call stack).
    Return,
    /// A visible action: show text / dialog (opcodes 3, 15, 53).
    VisibleText,
    /// A visible action: load a level's map + model (opcode 8).
    VisibleLoad,
    /// Opcodes 0/1 ("never should have gotten here") or an out-of-range opcode.
    Unknown,
}

impl StepKind {
    /// True if this is a visible action (a natural place to stop a trace).
    pub fn is_visible(self) -> bool {
        matches!(self, StepKind::VisibleText | StepKind::VisibleLoad)
    }
}

/// One executed instruction in a trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// PC of the opcode byte (offset into the bytecode).
    pub pc: usize,
    /// The opcode.
    pub opcode: u8,
    /// Numeric operands read (an inline string's length appears as the operand
    /// that encoded it).
    pub operands: Vec<i32>,
    /// Inline strings the opcode read (Latin-1), in order. Empty for opcodes
    /// without inline strings, or when the string is a lang reference.
    pub strings: Vec<String>,
    /// Classification of the opcode.
    pub kind: StepKind,
}

/// A piece of text referenced by an opcode: either a `lang_*` id or an inline
/// string literal embedded in the bytecode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextRef {
    Lang(u16),
    Inline(String),
}

/// The semantic effect of executing an opcode — the "what it does" layer on top
/// of decoding. Only the opcodes with clear, runtime-independent meaning are
/// modeled; the rest carry their opcode number (their full side effects belong
/// to later milestones that build the actor/world/UI state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// op 11 / 64: wait for N ms.
    Wait(u32),
    /// op 23: call (push) another script entry.
    Call(u8),
    /// op 2: return from the current entry.
    Return,
    /// op 3 / 15 / 39 / 53: show dialog/menu text.
    ShowText(TextRef),
    /// op 8: load a level's map (`.jtm`) and model (`.cml`).
    LoadLevel { map: String, model: String },
    /// op 43: load a `.cml` model (and, transitively, its referenced images).
    /// Confirmed against the runtime: loading `/startup.cml` pulls in its frames'
    /// PNGs (`/1.png /2.png /3.png /5.png`; `/4.png` is the loader's skip case).
    LoadModel(String),
    /// op 72: free cached graphics whose key starts with the given prefix
    /// (`g`'s cache-clear). Confirmed against the runtime — these cause *no*
    /// resource load (which is how the runtime oracle corrected an earlier
    /// "LoadResource" misreading).
    FreeGraphics(String),
    /// op 9 / 29 / 56: a string-bearing UI/system action (set title, load lang,
    /// queue next script, etc.) — the string is captured; the precise effect is
    /// deferred.
    StringAction(String),
    /// Any other opcode: number + numeric operands (side effect deferred).
    Other(u8),
}

/// Highest opcode handled by the dispatch (`e.java` switch goes 0..=78).
pub const MAX_OPCODE: u8 = 78;

/// The `.scr` bytecode interpreter.
#[derive(Debug, Clone)]
pub struct ScriptVm {
    code: Vec<u8>,
    entry_offsets: Vec<i32>,
    /// Call stack of program counters (`var_int_arr_a` / `var_int_a`).
    stack: Vec<usize>,
}

impl ScriptVm {
    /// Build a VM over a loaded program.
    pub fn new(program: &ScrProgram) -> Self {
        Self {
            code: program.code.clone(),
            entry_offsets: program.entry_offsets.clone(),
            stack: Vec::new(),
        }
    }

    /// Push an entry onto the call stack (`e.void_a(id)`). Mirrors the original:
    /// the offset is pushed as-is (an unused entry is 0 / first code byte; the
    /// game never calls unused entries). Rejects only an out-of-range PC so the
    /// decoder can't index past the bytecode.
    pub fn start_entry(&mut self, id: u8) -> bool {
        let Some(&off) = self.entry_offsets.get(id as usize) else {
            return false;
        };
        let Ok(pc) = usize::try_from(off) else {
            return false;
        };
        if pc > self.code.len() {
            return false;
        }
        self.stack.push(pc);
        true
    }

    /// True while there is a frame to execute.
    pub fn running(&self) -> bool {
        !self.stack.is_empty()
    }

    fn pc(&self) -> usize {
        *self.stack.last().unwrap()
    }

    fn u8(&mut self) -> Result<i32, ParseError> {
        let top = self.stack.len() - 1;
        let pc = self.stack[top];
        let b = *self.code.get(pc).ok_or(ParseError::Eof)?;
        self.stack[top] = pc + 1;
        Ok(i32::from(b))
    }

    fn u16(&mut self) -> Result<i32, ParseError> {
        let hi = self.u8()?;
        let lo = self.u8()?;
        Ok((hi << 8) | lo)
    }

    fn u24(&mut self) -> Result<i32, ParseError> {
        let a = self.u8()?;
        let b = self.u16()?;
        Ok((a << 16) | b)
    }

    /// Read N u8 operands into `out`.
    fn read_u8s(&mut self, n: usize, out: &mut Vec<i32>) -> Result<(), ParseError> {
        for _ in 0..n {
            let v = self.u8()?;
            out.push(v);
        }
        Ok(())
    }

    /// Read `len` bytes as a Latin-1 string, advancing the PC.
    fn read_str(&mut self, len: i32) -> Result<String, ParseError> {
        let len = usize::try_from(len).map_err(|_| ParseError::Eof)?;
        let top = self.stack.len() - 1;
        let start = self.stack[top];
        let end = start.checked_add(len).ok_or(ParseError::Eof)?;
        let bytes = self.code.get(start..end).ok_or(ParseError::Eof)?;
        let s: String = bytes.iter().map(|&b| b as char).collect();
        self.stack[top] = end;
        Ok(s)
    }

    /// A lang-id reference packs `0xF___`; otherwise the value is an inline
    /// string length. Reads and returns the inline string when present.
    fn maybe_inline_string(&mut self, n16: i32) -> Result<Option<String>, ParseError> {
        if (n16 & 0xF000) != 0xF000 {
            Ok(Some(self.read_str(n16)?))
        } else {
            Ok(None)
        }
    }

    /// Execute one opcode, returning the decoded [`Step`]. Mirrors the operand
    /// consumption of every case in `e.java::b(long)`.
    pub fn step(&mut self) -> Result<Step, ParseError> {
        let pc = self.pc();
        let opcode = *self.code.get(pc).ok_or(ParseError::Eof)?;
        // Java advances the PC past the opcode byte before the switch.
        {
            let top = self.stack.len() - 1;
            self.stack[top] = pc + 1;
        }

        let mut ops: Vec<i32> = Vec::new();
        let mut strs: Vec<String> = Vec::new();
        let kind = match opcode {
            0 | 1 => StepKind::Unknown,
            2 => StepKind::Return,
            3 => {
                let n16 = self.u16()?;
                ops.push(n16);
                if let Some(s) = self.maybe_inline_string(n16)? {
                    strs.push(s);
                }
                StepKind::VisibleText
            }
            4 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            5..=7 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            8 => {
                let l1 = self.u8()?;
                ops.push(l1);
                strs.push(self.read_str(l1)?);
                let l2 = self.u8()?;
                ops.push(l2);
                strs.push(self.read_str(l2)?);
                StepKind::VisibleLoad
            }
            9 => {
                let l = self.u8()?;
                ops.push(l);
                strs.push(self.read_str(l)?);
                StepKind::Normal
            }
            10 => {
                ops.push(self.u8()?);
                ops.push(self.u24()?);
                StepKind::Normal
            }
            11 => {
                ops.push(self.u16()?);
                StepKind::Wait
            }
            12 => StepKind::Normal,
            13 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            14 => {
                let sel = self.u8()?;
                ops.push(sel);
                if (0..=4).contains(&sel) {
                    ops.push(self.u8()?);
                }
                StepKind::Normal
            }
            15 => {
                let n16 = self.u16()?;
                ops.push(n16);
                if n16 != 0 {
                    if let Some(s) = self.maybe_inline_string(n16)? {
                        strs.push(s);
                    }
                }
                ops.push(self.u8()?);
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::VisibleText
            }
            16 => {
                self.read_u8s(5, &mut ops)?;
                StepKind::Normal
            }
            17 => {
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            18 => {
                self.read_u8s(4, &mut ops)?;
                StepKind::Normal
            }
            19 | 20 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            21 => {
                let cnt = self.u8()?;
                ops.push(cnt);
                let cnt = usize::try_from(cnt).map_err(|_| ParseError::Eof)?;
                self.read_u8s(cnt, &mut ops)?;
                StepKind::Normal
            }
            22 => {
                self.read_u8s(3, &mut ops)?;
                StepKind::Normal
            }
            23 => {
                let id = self.u8()?;
                ops.push(id);
                StepKind::Call(id as u8)
            }
            24 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            25 => {
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            26 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            27 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            28 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            29 => {
                let l = self.u8()?;
                ops.push(l);
                strs.push(self.read_str(l)?);
                StepKind::Normal
            }
            32 => {
                self.read_u8s(3, &mut ops)?;
                StepKind::Normal
            }
            33 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            34 => {
                let n13 = self.u8()?;
                let n17 = self.u8()?;
                ops.push(n13);
                ops.push(n17);
                match n17 {
                    2..=6 | 8..=13 | 18..=20 => ops.push(self.u8()?),
                    7 | 14 | 15 => ops.push(self.u16()?),
                    _ => {}
                }
                StepKind::Normal
            }
            35 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            36 => {
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            37 | 38 => {
                self.read_u8s(3, &mut ops)?;
                StepKind::Normal
            }
            39 => {
                let n16 = self.u16()?;
                ops.push(n16);
                if let Some(s) = self.maybe_inline_string(n16)? {
                    strs.push(s);
                }
                self.read_u8s(3, &mut ops)?;
                StepKind::VisibleText
            }
            40 => StepKind::Normal,
            41 | 42 => {
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            43 => {
                let l = self.u8()?;
                ops.push(l);
                strs.push(self.read_str(l)?);
                StepKind::Normal
            }
            44 | 45 => StepKind::Normal,
            46 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            47 => {
                self.read_u8s(3, &mut ops)?;
                StepKind::Normal
            }
            48 => StepKind::Normal,
            49 => {
                self.read_u8s(3, &mut ops)?;
                StepKind::Normal
            }
            50 => {
                self.read_u8s(7, &mut ops)?;
                StepKind::Normal
            }
            51 => {
                self.read_u8s(4, &mut ops)?;
                StepKind::Normal
            }
            52 => {
                ops.push(self.u8()?);
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            53 => {
                ops.push(self.u8()?);
                ops.push(self.u8()?);
                let n16 = self.u16()?;
                ops.push(n16);
                if let Some(s) = self.maybe_inline_string(n16)? {
                    strs.push(s);
                }
                StepKind::VisibleText
            }
            54 | 55 => StepKind::Normal,
            56 => {
                let l = self.u8()?;
                ops.push(l);
                strs.push(self.read_str(l)?);
                ops.push(self.u8()?);
                StepKind::Normal
            }
            57 => StepKind::Normal,
            58 | 59 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            60..=63 => StepKind::Normal,
            64 => {
                ops.push(self.u24()?);
                StepKind::Normal
            }
            65 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            66 => {
                let n16 = self.u16()?;
                ops.push(n16);
                if let Some(s) = self.maybe_inline_string(n16)? {
                    strs.push(s);
                }
                StepKind::Normal
            }
            67 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            68 => {
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            69 => {
                ops.push(self.u8()?);
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                ops.push(self.u8()?);
                StepKind::Normal
            }
            70 | 71 => {
                ops.push(self.u16()?);
                ops.push(self.u16()?);
                StepKind::Normal
            }
            72 => {
                let len = self.u16()?;
                ops.push(len);
                strs.push(self.read_str(len)?);
                StepKind::Normal
            }
            73 | 74 => StepKind::Normal,
            75 | 76 => {
                self.read_u8s(1, &mut ops)?;
                StepKind::Normal
            }
            77 => StepKind::Normal,
            78 => {
                self.read_u8s(2, &mut ops)?;
                StepKind::Normal
            }
            _ => StepKind::Unknown,
        };

        // Apply control-flow effects.
        match kind {
            StepKind::Return => {
                self.stack.pop();
            }
            StepKind::Call(id) => {
                self.start_entry(id);
            }
            _ => {}
        }

        Ok(Step {
            pc,
            opcode,
            operands: ops,
            strings: strs,
            kind,
        })
    }

    /// Trace from `entry` until the first visible action, an empty call stack, an
    /// unknown opcode, or `max_steps` — whichever comes first. Returns the steps
    /// executed (the last one is the visible action, if reached).
    pub fn trace_entry(&mut self, entry: u8, max_steps: usize) -> Result<Vec<Step>, ParseError> {
        self.trace_entry_opts(entry, max_steps, true)
    }

    /// Like [`Self::trace_entry`], but `stop_at_visible` controls whether a
    /// visible action ends the trace. With `false`, the trace runs to an empty
    /// call stack / unknown opcode / `max_steps` — used to disassemble a whole
    /// entry for the coverage report.
    pub fn trace_entry_opts(
        &mut self,
        entry: u8,
        max_steps: usize,
        stop_at_visible: bool,
    ) -> Result<Vec<Step>, ParseError> {
        let mut steps = Vec::new();
        if !self.start_entry(entry) {
            return Ok(steps);
        }
        while self.running() && steps.len() < max_steps {
            let step = self.step()?;
            let visible = step.kind.is_visible();
            let unknown = matches!(step.kind, StepKind::Unknown);
            steps.push(step);
            if (visible && stop_at_visible) || unknown {
                break;
            }
        }
        Ok(steps)
    }

    /// Execute an entry to completion (empty call stack / unknown opcode /
    /// `max_steps`), returning each step. Unlike a decode trace, the steps carry
    /// captured inline strings, so [`Step::effect`] yields full semantic effects.
    /// Per-entry execution is deterministic (no data-dependent branches in the
    /// bytecode), so this is the script's actual run for the entry.
    pub fn run_entry(&mut self, entry: u8, max_steps: usize) -> Result<Vec<Step>, ParseError> {
        self.trace_entry_opts(entry, max_steps, false)
    }
}

impl Step {
    /// The semantic [`Effect`] of this executed step. If `lang` is given, lang
    /// text references are resolved to their string; otherwise they stay
    /// [`TextRef::Lang`].
    pub fn effect(&self, lang: Option<&crate::lang::Lang>) -> Effect {
        let s = |i: usize| self.strings.get(i).cloned().unwrap_or_default();
        let op = |i: usize| self.operands.get(i).copied().unwrap_or(0);
        let text_ref = |n16: i32, inline_idx: usize| -> TextRef {
            if (n16 & 0xF000) == 0xF000 {
                let id = (n16 & 0xFFF) as u16;
                match lang {
                    Some(l) => TextRef::Inline(l.get(id).to_string()),
                    None => TextRef::Lang(id),
                }
            } else {
                TextRef::Inline(self.strings.get(inline_idx).cloned().unwrap_or_default())
            }
        };
        match self.opcode {
            2 => Effect::Return,
            11 => Effect::Wait(op(0).max(0) as u32),
            23 => Effect::Call(op(0) as u8),
            3 | 15 | 39 => Effect::ShowText(text_ref(op(0), 0)),
            53 => Effect::ShowText(text_ref(op(2), 0)),
            8 => Effect::LoadLevel {
                map: s(0),
                model: s(1),
            },
            43 => Effect::LoadModel(s(0)),
            72 => Effect::FreeGraphics(s(0)),
            9 | 29 | 56 => Effect::StringAction(s(0)),
            other => Effect::Other(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scr::ScrProgram;

    fn vm_with_code(code: Vec<u8>, entry1: i32) -> ScriptVm {
        let mut entry_offsets = vec![0i32; 256];
        entry_offsets[1] = entry1;
        ScriptVm::new(&ScrProgram {
            entry_count: 1,
            entry_offsets,
            code_start: 0,
            code,
            sections: vec![],
            string_count: 0,
            global_e_count: 0,
        })
    }

    #[test]
    fn decodes_wait_then_text() {
        // opcode 11 (wait, u16=0x0064), opcode 3 (text, lang ref 0xF001)
        let mut vm = vm_with_code(vec![11, 0x00, 0x64, 3, 0xF0, 0x01], 0);
        let steps = vm.trace_entry(1, 16).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].opcode, 11);
        assert_eq!(steps[0].kind, StepKind::Wait);
        assert_eq!(steps[0].operands, vec![0x64]);
        assert_eq!(steps[1].opcode, 3);
        assert_eq!(steps[1].kind, StepKind::VisibleText);
        assert_eq!(steps[1].operands, vec![0xF001]);
    }

    #[test]
    fn inline_string_is_skipped() {
        // opcode 3 with inline string length 3 ("abc"), then opcode 8 load.
        let mut vm = vm_with_code(
            vec![3, 0x00, 0x03, b'a', b'b', b'c', /* never reached */ 2],
            0,
        );
        let steps = vm.trace_entry(1, 16).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::VisibleText);
        // PC after the text op should sit at the trailing opcode (index 6).
        assert_eq!(vm.pc(), 6);
    }

    #[test]
    fn call_and_return_follow_control_flow() {
        // entry1 at 0: opcode 23 call entry-as-id... we instead test pop/return.
        // code: [12 (normal), 2 (return)] -> after return, stack empty.
        let mut vm = vm_with_code(vec![12, 2], 0);
        let steps = vm.trace_entry(1, 16).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].kind, StepKind::Return);
        assert!(!vm.running());
    }

    #[test]
    fn truncated_code_is_error_not_panic() {
        let mut vm = vm_with_code(vec![3, 0x00], 0); // u16 operand truncated
        assert_eq!(vm.trace_entry(1, 16).err(), Some(ParseError::Eof));
    }

    #[test]
    fn execute_resolves_effects_and_strings() {
        // op 72 free "/x.png" (len 6); op 8 load map "/m.jtm"(6)+model "/o.cml"(6);
        // op 11 wait 0x0064; op 3 lang ref 0xF002; op 2 return.
        let mut code = vec![72, 0x00, 0x06];
        code.extend_from_slice(b"/x.png");
        code.push(8);
        code.push(6);
        code.extend_from_slice(b"/m.jtm");
        code.push(6);
        code.extend_from_slice(b"/o.cml");
        code.extend_from_slice(&[11, 0x00, 0x64, 3, 0xF0, 0x02, 2]);
        let mut vm = vm_with_code(code, 0);
        let steps = vm.run_entry(1, 64).unwrap();
        let effects: Vec<Effect> = steps.iter().map(|s| s.effect(None)).collect();
        assert_eq!(
            effects,
            vec![
                Effect::FreeGraphics("/x.png".into()),
                Effect::LoadLevel {
                    map: "/m.jtm".into(),
                    model: "/o.cml".into()
                },
                Effect::Wait(100),
                Effect::ShowText(TextRef::Lang(2)),
                Effect::Return,
            ]
        );
    }
}
