//! Dumping/ground-truth helpers shared by the `eso-dump` binary and the golden
//! snapshot tests.
//!
//! Two output shapes:
//!  - **canonical** (`dump_jtm`, `dump_lang`): the rigid, byte-comparable text
//!    the FreeJ2ME oracle is instrumented to reproduce, for a mechanical diff.
//!  - **summary** (`summarize_jtm`): a compact, review-friendly digest (dims +
//!    per-layer FNV-1a hash) for an `insta` snapshot, so multi-thousand-cell
//!    maps don't bloat the repo while still failing on any drift.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use formats::vm::StepKind;
use formats::{parse_jtm, parse_lang_file, parse_scr, AssetStore, ScriptVm};

/// FNV-1a 64-bit hash — small, dependency-free, deterministic across platforms.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Canonical `.jtm` dump (byte-comparable with the oracle).
pub fn dump_jtm(store: &AssetStore, names: &[String]) -> Result<String> {
    let resources = pick(store, names, "jtm")?;
    let mut out = String::new();
    for res in resources {
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let map = parse_jtm(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
        writeln!(out, "# jtm {res}")?;
        writeln!(
            out,
            "width={} height={} layers={}",
            map.width,
            map.height,
            map.layers.len()
        )?;
        for li in 0..map.layers.len() {
            writeln!(out, "layer {li}")?;
            for y in 0..map.height {
                let mut row = String::new();
                for x in 0..map.width {
                    if x > 0 {
                        row.push(' ');
                    }
                    write!(row, "{}", map.tile(li, x, y).expect("in-range"))?;
                }
                writeln!(out, "{row}")?;
            }
        }
    }
    Ok(out)
}

/// Compact `.jtm` summary for snapshotting: dims + per-layer hash.
pub fn summarize_jtm(store: &AssetStore) -> Result<String> {
    let mut out = String::new();
    for res in store.list("jtm")? {
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let map = parse_jtm(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
        write!(
            out,
            "{res}: {}x{} layers={}",
            map.width,
            map.height,
            map.layers.len()
        )?;
        for layer in &map.layers {
            write!(out, " {:016x}", fnv1a(layer))?;
        }
        out.push('\n');
    }
    Ok(out)
}

/// Canonical `lang_*.txt` dump (byte-comparable with the oracle).
pub fn dump_lang(store: &AssetStore, ids: &[u8]) -> Result<String> {
    let ids: Vec<u8> = if ids.is_empty() {
        (0u8..=12).collect()
    } else {
        ids.to_vec()
    };
    let mut out = String::new();
    for id in ids {
        let res = format!("/lang_{id}.txt");
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let table = parse_lang_file(&bytes, id).with_context(|| format!("unknown lang id {id}"))?;
        writeln!(out, "# lang {res} entries={}", table.len())?;
        for (lang_id, text) in &table {
            writeln!(out, "{lang_id}\t{}", escape(text))?;
        }
    }
    Ok(out)
}

/// Canonical structural dump of `.scr` loaders (byte-comparable with the oracle):
/// entry table + code-start + code hash + section headers.
pub fn dump_scr(store: &AssetStore, names: &[String]) -> Result<String> {
    let resources = pick(store, names, "scr")?;
    let mut out = String::new();
    for res in resources {
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let p = parse_scr(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
        writeln!(out, "# scr {res}")?;
        writeln!(
            out,
            "entry_count={} code_start={} code_len={} sections={} strings={}",
            p.entry_count,
            p.code_start,
            p.code.len(),
            p.sections.len(),
            p.string_count
        )?;
        let mut entries = String::from("entries:");
        for (id, off) in p.entry_offsets.iter().enumerate() {
            if *off != 0 {
                write!(entries, " {id}={off}")?;
            }
        }
        writeln!(out, "{entries}")?;
        for s in &p.sections {
            writeln!(out, "section subtype={} index={}", s.subtype, s.index)?;
        }
        writeln!(out, "code_fnv={:016x}", fnv1a(&p.code))?;
    }
    Ok(out)
}

/// `scr-coverage` report: which opcodes the VM decodes, and which are actually
/// exercised by the real scripts (by disassembling entry 1 of every `.scr`,
/// following calls/returns). The decoder covers the full `0..=78` opcode space;
/// this shows how much of it the shipped content reaches and flags any opcode a
/// real script uses that the decoder would not recognize (there should be none).
pub fn scr_coverage(store: &AssetStore) -> Result<String> {
    use formats::vm::MAX_OPCODE;

    let mut seen = [0u64; 256];
    let mut scripts = 0u32;
    let mut unknown_hits = 0u64;
    for res in store.list("scr")? {
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let program = parse_scr(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
        scripts += 1;
        let mut vm = ScriptVm::new(&program);
        // Disassemble entry 1 fully (don't stop at the first visible action).
        let steps = vm
            .trace_entry_opts(1, 100_000, false)
            .map_err(|e| anyhow::anyhow!("tracing {res}: {e}"))?;
        for s in steps {
            seen[s.opcode as usize] += 1;
            if matches!(s.kind, StepKind::Unknown) {
                unknown_hits += 1;
            }
        }
    }

    let total = usize::from(MAX_OPCODE) + 1;
    let exercised = (0..total).filter(|&op| seen[op] > 0).count();
    let mut out = String::new();
    writeln!(
        out,
        "# scr-coverage  scripts={scripts}  opcodes_decoded=0..={MAX_OPCODE}  exercised={exercised}/{total}  unknown_opcode_hits={unknown_hits}"
    )?;
    let missing: Vec<String> = (0..total)
        .filter(|&op| seen[op] == 0)
        .map(|op| op.to_string())
        .collect();
    writeln!(out, "not_exercised_by_entry1: {}", missing.join(" "))?;
    for (op, &count) in seen.iter().enumerate().take(total) {
        if count > 0 {
            writeln!(out, "op {op}: {count} uses")?;
        }
    }
    writeln!(
        out,
        "note: operand decoding is complete for all opcodes; opcode *side effects* are deferred past M3 (skeleton)."
    )?;
    Ok(out)
}

fn kind_str(k: StepKind) -> String {
    match k {
        StepKind::Normal => "Normal".into(),
        StepKind::Wait => "Wait".into(),
        StepKind::Call(id) => format!("Call({id})"),
        StepKind::Return => "Return".into(),
        StepKind::VisibleText => "VisibleText".into(),
        StepKind::VisibleLoad => "VisibleLoad".into(),
        StepKind::Unknown => "Unknown".into(),
    }
}

/// Canonical opcode trace of a script from `entry` to its first visible action
/// (byte-comparable with the oracle).
pub fn dump_scr_trace(
    store: &AssetStore,
    res: &str,
    entry: u8,
    max_steps: usize,
) -> Result<String> {
    let bytes = store.load(res).with_context(|| format!("loading {res}"))?;
    let program = parse_scr(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
    let mut vm = ScriptVm::new(&program);
    let steps = vm
        .trace_entry(entry, max_steps)
        .map_err(|e| anyhow::anyhow!("tracing {res}: {e}"))?;

    let mut out = String::new();
    writeln!(out, "# trace {res} entry={entry}")?;
    for (i, s) in steps.iter().enumerate() {
        let ops = s
            .operands
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            out,
            "{i} pc={} op={} kind={} ops=[{ops}]",
            s.pc,
            s.opcode,
            kind_str(s.kind)
        )?;
    }
    let result = match steps.last() {
        Some(s) if s.kind.is_visible() => "visible_action",
        Some(s) if matches!(s.kind, StepKind::Unknown) => "unknown_opcode",
        _ if steps.len() >= max_steps => "step_cap",
        _ => "stack_empty",
    };
    writeln!(out, "result={result} steps={}", steps.len())?;
    Ok(out)
}

fn pick(store: &AssetStore, names: &[String], ext: &str) -> Result<Vec<String>> {
    if names.is_empty() {
        Ok(store.list(ext)?)
    } else {
        Ok(names.to_vec())
    }
}

/// Escape a Latin-1 string to byte-stable ASCII: printable ASCII passes through
/// (backslash doubled), everything else becomes `\xHH` of its code point.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => {
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
        }
    }
    out
}
