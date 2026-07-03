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
use formats::{parse_cml, parse_jtm, parse_lang_file, parse_scr, AssetStore, ScriptVm, Tables};

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

/// Compact `.cml` summary for snapshotting: per-file record/group/frame counts,
/// bytes consumed, and a hash of the full canonical dump.
pub fn summarize_cml(store: &AssetStore) -> Result<String> {
    let mut out = String::new();
    for res in store.list("cml")? {
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let cml = parse_cml(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
        let groups: usize = cml.records.iter().map(|r| r.anim_groups.len()).sum();
        let frames: usize = cml
            .records
            .iter()
            .flat_map(|r| r.anim_groups.iter())
            .map(|g| g.frames.len())
            .sum();
        let one = dump_cml(store, std::slice::from_ref(&res))?;
        writeln!(
            out,
            "{res}: records={} groups={groups} frames={frames} consumed={}/{} {:016x}",
            cml.records.len(),
            cml.consumed,
            bytes.len(),
            fnv1a(one.as_bytes())
        )?;
    }
    Ok(out)
}

/// Flat `.jtm` layer dump matching the FreeJ2ME `Instrument.dumpJtm` format:
/// `layers=N` then each layer's bytes as space-joined unsigned decimals, in
/// storage order (base layer first). This is the orientation-independent form
/// used to diff our parse against the *live* runtime's in-memory grid.
pub fn dump_jtm_flat(store: &AssetStore, name: &str) -> Result<String> {
    let res = if name.starts_with('/') {
        name.to_string()
    } else {
        format!("/{name}")
    };
    let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
    let map = parse_jtm(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
    let mut out = String::new();
    writeln!(out, "layers={}", map.layers.len())?;
    for layer in &map.layers {
        let row = layer
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(out, "{row}")?;
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

fn flags_str(f: &[i32; 10]) -> String {
    ints(f)
}

fn ints(f: &[i32]) -> String {
    f.iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Canonical dump of `.cml` models (byte-comparable with the oracle).
pub fn dump_cml(store: &AssetStore, names: &[String]) -> Result<String> {
    let resources = pick(store, names, "cml")?;
    let mut out = String::new();
    for res in resources {
        let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
        let cml = parse_cml(&bytes).map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
        writeln!(out, "# cml {res}")?;
        writeln!(
            out,
            "prefix={} records={} consumed={}",
            escape(&cml.prefix),
            cml.records.len(),
            cml.consumed
        )?;
        for r in &cml.records {
            writeln!(
                out,
                "rec id={} eid={} path={} static={} skipped={} flags={} boxes={} anim={}",
                r.frame_id,
                r.effective_id,
                escape(&r.path),
                r.is_static,
                r.skipped,
                flags_str(&r.flags),
                r.boxes.len(),
                r.anim_groups.len()
            )?;
            for (a, b) in &r.boxes {
                writeln!(out, "box {a} {b}")?;
            }
            for g in &r.anim_groups {
                writeln!(out, "grp {} frames={}", flags_str(&g.flags), g.frames.len())?;
                for f in &g.frames {
                    writeln!(out, "frm {}", flags_str(f))?;
                }
            }
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
            "entry_count={} code_start={} code_len={} sections={} strings={} global_e={}",
            p.entry_count,
            p.code_start,
            p.code.len(),
            p.sections.len(),
            p.string_count,
            p.global_e_count
        )?;
        let mut entries = String::from("entries:");
        for (id, off) in p.entry_offsets.iter().enumerate() {
            if *off != 0 {
                write!(entries, " {id}={off}")?;
            }
        }
        writeln!(out, "{entries}")?;
        for s in &p.sections {
            write!(
                out,
                "section subtype={} index={} fields=[{}]",
                s.subtype,
                s.index,
                ints(&s.fields)
            )?;
            if !s.aux_a.is_empty() {
                write!(out, " aux_a=[{}]", ints(&s.aux_a))?;
            }
            if !s.aux_b.is_empty() {
                write!(out, " aux_b=[{}]", ints(&s.aux_b))?;
            }
            out.push('\n');
        }
        writeln!(out, "code_fnv={:016x}", fnv1a(&p.code))?;
    }
    Ok(out)
}

/// Round-trip an `ESO` save blob: parse it, re-serialize, and report whether the
/// bytes are identical (the key validation against a real, game-written blob),
/// plus a decoded summary. `path` is a file containing the raw record bytes
/// (captured via the FreeJ2ME `-Doracle.savelog` hook).
pub fn save_roundtrip(path: &str) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    let save = formats::parse_save(&bytes).map_err(|e| anyhow::anyhow!("parsing {path}: {e}"))?;
    let reser = formats::serialize_save(&save);
    let identical = reser == bytes;
    let mut out = String::new();
    writeln!(out, "# save {path} ({} bytes)", bytes.len())?;
    writeln!(out, "round_trip_identical={identical}")?;
    writeln!(
        out,
        "flags={:?} bool_o={} player={}",
        save.flags,
        save.bool_o,
        save.player.is_some()
    )?;
    if let Some(p) = &save.player {
        writeln!(out, "player_name={:?}", String::from_utf8_lossy(&p.name))?;
        writeln!(
            out,
            "actor model={:?} items={}",
            String::from_utf8_lossy(&p.actor.model_name),
            p.actor.items.len()
        )?;
    }
    if !identical {
        let n = reser.iter().zip(&bytes).take_while(|(a, b)| a == b).count();
        writeln!(out, "first_diff_at_byte={n} (reser_len={})", reser.len())?;
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

fn effect_str(e: &formats::Effect) -> String {
    use formats::{Effect, TextRef};
    let tr = |t: &TextRef| match t {
        TextRef::Lang(id) => format!("lang#{id}"),
        TextRef::Inline(s) => format!("{:?}", s),
    };
    match e {
        Effect::Wait(ms) => format!("Wait {ms}ms"),
        Effect::Call(id) => format!("Call entry {id}"),
        Effect::Return => "Return".into(),
        Effect::ShowText(t) => format!("ShowText {}", tr(t)),
        Effect::LoadLevel { map, model } => format!("LoadLevel map={map:?} model={model:?}"),
        Effect::LoadModel(m) => format!("LoadModel {m:?}"),
        Effect::FreeGraphics(p) => format!("FreeGraphics {p:?}"),
        Effect::StringAction(s) => format!("StringAction {s:?}"),
        Effect::Other(op) => format!("op{op}"),
    }
}

/// Execute a script entry and dump the resolved semantic effects (load/show/wait
/// /call/return), with lang references resolved against `lang_0`.
pub fn dump_scr_exec(store: &AssetStore, res: &str, entry: u8, max_steps: usize) -> Result<String> {
    let program = parse_scr(&store.load(res).with_context(|| format!("loading {res}"))?)
        .map_err(|e| anyhow::anyhow!("parsing {res}: {e}"))?;
    let lang = store
        .load("/lang_0.txt")
        .ok()
        .map(|b| formats::Lang::load_base(&b));
    let mut vm = ScriptVm::new(&program);
    let steps = vm
        .run_entry(entry, max_steps)
        .map_err(|e| anyhow::anyhow!("executing {res}: {e}"))?;

    let mut out = String::new();
    writeln!(out, "# exec {res} entry={entry}")?;
    for (i, s) in steps.iter().enumerate() {
        let eff = s.effect(lang.as_ref());
        writeln!(out, "{i} pc={} op={} {}", s.pc, s.opcode, effect_str(&eff))?;
    }
    let last = steps.last();
    let result = match last {
        Some(s) if matches!(s.kind, StepKind::Unknown) => "unknown_opcode",
        _ if steps.len() >= max_steps => "step_cap",
        _ => "stack_empty",
    };
    writeln!(out, "result={result} steps={}", steps.len())?;
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

/// Parse the oracle's live-store table dump (`Instrument.dumpTables`) into a
/// [`Tables`]. Format: `subtype <s> rows <r> cols <c>` headers, then rows
/// `<s> <rowidx> <v0> <v1> ...` (the leading subtype + row index are stripped).
fn parse_tables(text: &str) -> Result<Tables> {
    let mut tables = Tables::default();
    let mut cur: Option<(u8, Vec<Vec<i32>>)> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("subtype ") {
            if let Some((st, rows)) = cur.take() {
                tables.insert(st, rows);
            }
            let st: u8 = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .context("table header missing subtype")?;
            cur = Some((st, Vec::new()));
        } else {
            let toks = line
                .split_whitespace()
                .map(|s| s.parse::<i32>())
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| anyhow::anyhow!("non-integer in table row: {line:?}"))?;
            let (_, rows) = cur
                .as_mut()
                .context("table row before any subtype header")?;
            // Drop the leading [subtype, rowindex]; the rest is the row.
            rows.push(toks.get(2..).unwrap_or(&[]).to_vec());
        }
    }
    if let Some((st, rows)) = cur.take() {
        tables.insert(st, rows);
    }
    Ok(tables)
}

/// Run the Rust port of `h.f` (`Actor::class_progression`) over the same synthetic
/// actor sweep the FreeJ2ME oracle (`Instrument.dumpHf`) drives through the *real*
/// `h.f`, emitting the byte-identical canonical text for a mechanical diff. Reads
/// the ground-truth stat tables from the oracle's `dumptables` capture so both
/// sides operate on provably-identical table input.
pub fn dump_hf_sweep(tables_path: &str) -> Result<String> {
    let text = std::fs::read_to_string(tables_path)
        .with_context(|| format!("reading tables fixture {tables_path}"))?;
    let tables = parse_tables(&text)?;
    let r4 = tables.rows(4).len();

    let empty = [-1i32; 8];
    let full = [0i32, 1, 2, 3, 4, 5, 6, 7];

    let mut out = String::new();
    writeln!(out, "# h.f sweep: f o j inv0..inv7 | i z A B C D")?;
    writeln!(out, "races {r4}")?;
    // Phase A: empty inventory, full class x level x race.
    for fv in 1..=8u8 {
        for ov in 1..=20u8 {
            for jv in 0..r4 {
                hf_line(&mut out, &tables, fv, ov, jv, &empty)?;
            }
        }
    }
    // Phase B: full inventory at class 1 / level 1, all races (exercises var_short_z).
    for jv in 0..r4 {
        hf_line(&mut out, &tables, 1, 1, jv, &full)?;
    }
    Ok(out)
}

fn hf_line(
    out: &mut String,
    tables: &Tables,
    fv: u8,
    ov: u8,
    jv: usize,
    inv: &[i32; 8],
) -> Result<()> {
    let mut a = formats::Actor {
        var_byte_f: fv as i8,
        var_byte_o: ov as i8,
        var_byte_j: jv as i8,
        var_int_arr_n: *inv,
        ..Default::default()
    };
    a.class_progression(tables);
    write!(out, "{fv} {ov} {jv}")?;
    for v in inv {
        write!(out, " {v}")?;
    }
    write!(
        out,
        " | {} {} {} {} {} {}",
        a.var_byte_i, a.var_short_z, a.prog_a, a.prog_b, a.prog_c, a.prog_d
    )?;
    out.push('\n');
    Ok(())
}

/// Run the Rust melee port (`formats::melee_attack`) over the same synthetic
/// attacker/target + seed sweep the FreeJ2ME oracle (`Instrument.dumpCombat`)
/// drives through the *real* `h.a` bytecode, emitting byte-identical canonical
/// text. The per-case `probe` is one extra `nextInt()` after the attack: it pins
/// that combat consumed the identical number of RNG draws. Self-contained (no
/// table input): the actors' combat fields are set directly, as in the oracle.
pub fn dump_combat_sweep() -> Result<String> {
    use formats::{melee_attack, Actor, JavaRandom};

    let atk_s = [10i32, 40];
    let atk_d = [60i32, 120];
    let tgt_a = [0i32, 50];
    let tgt_v = [0i32, 20];
    let tgt_b = [0i32, 40];

    let mut out = String::new();
    writeln!(
        out,
        "# combat sweep: seed s D A v B | qAfter byteQ died outcome probe"
    )?;
    for seed in 0..20i64 {
        for &s in &atk_s {
            for &d in &atk_d {
                for &a in &tgt_a {
                    for &v in &tgt_v {
                        for &bb in &tgt_b {
                            let attacker = Actor {
                                var_byte_c: 0,
                                var_byte_o: 5,
                                var_byte_t: 0,
                                var_byte_u: 0,
                                var_short_s: s as i16,
                                o_bonus: 5,
                                var_byte_i: 10,
                                k_bonus: 2,
                                n_bonus: 3,
                                prog_d: d as i16,
                                var_int_arr_l: None,
                                ..Default::default()
                            };
                            let mut target = Actor {
                                var_byte_c: 1,
                                var_byte_u: 0,
                                var_byte_q: 0,
                                prog_a: a as i16,
                                prog_b: bb as i16,
                                h_field: 100,
                                var_short_v: v as i16,
                                var_short_z: 0,
                                l_bonus: 0,
                                m_bonus: 0,
                                var_short_q: 10_000,
                                ..Default::default()
                            };
                            let mut rng = JavaRandom::new(seed);
                            let (died, outcome) =
                                melee_attack(&attacker, 0, &mut target, &[], true, &mut rng);
                            let probe = rng.next_int();
                            writeln!(
                                out,
                                "{seed} {s} {d} {a} {v} {bb} | {} {} {} {} {probe}",
                                target.var_short_q,
                                target.var_byte_q,
                                i32::from(died),
                                outcome as i32
                            )?;
                        }
                    }
                }
            }
        }
    }

    // Phase B: equipped-weapon damage override (see oracle Phase B comment).
    writeln!(
        out,
        "# phase B (weapons): W seed c bl wtype lvl | qAfter byteQ died outcome probe"
    )?;
    let weps = [
        vec![0i32, 0, 1, 10, 20, 30, 0, 0, 0, 5, 10],
        vec![0i32, 0, 4, 10, 20, 30, 0, 0, 0, 5, 10],
    ];
    let lvls = [3i32, 5, 10];
    let cfgs = [(1i32, true, 0usize), (0, false, 0), (0, false, 1)]; // (c, bl, wtype)
    for seed in 0..10i64 {
        for &(c, bl, wi) in &cfgs {
            for &lvl in &lvls {
                let attacker = Actor {
                    var_byte_c: c as i8,
                    var_byte_o: lvl as i8,
                    var_byte_t: 0,
                    var_byte_u: 0,
                    var_short_s: 20,
                    o_bonus: 5,
                    var_byte_i: 10,
                    k_bonus: 2,
                    n_bonus: 3,
                    prog_d: 100,
                    var_int_arr_l: Some(weps[wi].clone()),
                    ..Default::default()
                };
                let mut target = Actor {
                    var_byte_c: 1,
                    var_byte_u: 0,
                    var_byte_q: 0,
                    prog_a: 0,
                    prog_b: 0,
                    h_field: 100,
                    var_short_v: 0,
                    var_short_z: 0,
                    l_bonus: 0,
                    m_bonus: 0,
                    var_short_q: 10_000,
                    ..Default::default()
                };
                let mut rng = JavaRandom::new(seed);
                let (died, outcome) = melee_attack(&attacker, 0, &mut target, &[], bl, &mut rng);
                let probe = rng.next_int();
                writeln!(
                    out,
                    "W {seed} {c} {} {wi} {lvl} | {} {} {} {} {probe}",
                    i32::from(bl),
                    target.var_short_q,
                    target.var_byte_q,
                    i32::from(died),
                    outcome as i32
                )?;
            }
        }
    }

    // Phase C: non-player target — the E-update runs on a landed hit.
    writeln!(
        out,
        "# phase C (non-player E-update): C seed tx ty | qAfter byteQ died outcome E probe"
    )?;
    let tpos = [
        [1000i32, 500],
        [1100, 500],
        [1000, 800],
        [700, 200],
        [2000, 1500],
        [1000, 500],
    ];
    for seed in 0..5i64 {
        for tp in tpos {
            let attacker = Actor {
                var_byte_c: 0,
                var_byte_o: 5,
                var_byte_t: 0,
                var_byte_u: 0,
                var_short_s: 20,
                o_bonus: 5,
                var_byte_i: 10,
                k_bonus: 2,
                n_bonus: 3,
                prog_d: 100,
                var_int_arr_l: None,
                var_int_arr_b: [1000, 500],
                ..Default::default()
            };
            let mut target = Actor {
                var_byte_c: 0,
                var_byte_u: 0,
                var_byte_q: 0,
                prog_a: 0,
                prog_b: 0,
                h_field: 100,
                var_short_v: 0,
                var_short_z: 0,
                l_bonus: 0,
                m_bonus: 0,
                var_short_q: 10_000,
                var_int_arr_b: tp,
                e_field: 0,
                ..Default::default()
            };
            let mut rng = JavaRandom::new(seed);
            let (died, outcome) = melee_attack(&attacker, 0, &mut target, &[], true, &mut rng);
            let probe = rng.next_int();
            writeln!(
                out,
                "C {seed} {} {} | {} {} {} {} {} {probe}",
                tp[0],
                tp[1],
                target.var_short_q,
                target.var_byte_q,
                i32::from(died),
                outcome as i32,
                target.e_field
            )?;
        }
    }
    Ok(out)
}

/// Run the Rust `h.a(int[],int[])` distance port (`formats::combat_distance`) over
/// the same grid the oracle (`Instrument.dumpDist`) drives through the real method.
pub fn dump_dist_sweep() -> Result<String> {
    use formats::combat_distance;
    let coords = [
        -300i32, -130, -65, -33, -16, -7, -1, 0, 1, 7, 16, 33, 65, 130, 300,
    ];
    let mut out = String::new();
    writeln!(out, "# dist: ax ay bx by | d")?;
    for &bx in &coords {
        for &by in &coords {
            let d = combat_distance(&[0, 0], &[bx, by]);
            writeln!(out, "0 0 {bx} {by} | {d}")?;
        }
    }
    let pairs = [
        [100, 50, 130, 90],
        [-40, 20, 60, -33],
        [500, 500, 500, 500],
        [7, 7, 300, 1],
    ];
    for p in pairs {
        let d = combat_distance(&[p[0], p[1]], &[p[2], p[3]]);
        writeln!(out, "{} {} {} {} | {d}", p[0], p[1], p[2], p[3])?;
    }
    Ok(out)
}

/// Run the Rust targeting port (`formats::nearest_target` = `h.j_a`) over the same
/// synthetic actor array + querier sweep the oracle installs into `b.var_j_arr_a`
/// and drives through the real method. Emits the chosen slot index per query.
pub fn dump_targeting_sweep() -> Result<String> {
    use formats::{nearest_target, Actor};

    // A synthetic actor slot: (var_byte_c, var_byte_r, var_byte_q, x, y), or empty.
    type Slot = Option<(i8, i8, i8, i32, i32)>;
    // Synthetic `b.var_j_arr_a`.
    let spec: [Slot; 8] = [
        None,
        Some((0, 1, 0, 100, 100)),
        Some((0, 2, 0, 50, 50)),
        Some((0, 2, 1, 10, 10)), // dead
        Some((1, 2, 0, 20, 20)),
        Some((0, 2, 0, 200, 200)),
        None,
        Some((0, 3, 0, 60, 60)),
    ];
    let actors: Vec<Option<Actor>> = spec
        .iter()
        .map(|s| {
            s.map(|(c, r, qf, x, y)| Actor {
                var_byte_c: c,
                var_byte_r: r,
                var_byte_q: qf,
                var_int_arr_b: [x, y],
                ..Default::default()
            })
        })
        .collect();

    let cs = [0i8, 1, 2];
    let rs = [1i8, 2, 3, 9];
    let poss = [[0i32, 0], [55, 55], [150, 150], [1000, 1000]];

    let mut out = String::new();
    writeln!(out, "# targeting: qc qr qx qy | idx")?;
    for &qc in &cs {
        for &qr in &rs {
            for p in poss {
                let q = Actor {
                    var_byte_c: qc,
                    var_byte_r: qr,
                    var_int_arr_b: p,
                    ..Default::default()
                };
                let idx = nearest_target(&actors, &q).map_or(-1, |i| i as i32);
                writeln!(out, "{qc} {qr} {} {} | {idx}", p[0], p[1])?;
            }
        }
    }
    Ok(out)
}

/// Run the Rust map-collision port (`formats::collides` = `h.boolean_a`) over the
/// same synthetic collision layer + crafted-sample cases the oracle swaps into
/// `b.var_byte_arr_a`/dims and drives through the real method. Emits per-case blocks.
pub fn dump_collision_sweep() -> Result<String> {
    use formats::{collides, Actor};

    // 8x8 collision layer matching the oracle's: 1=solid, 2..=5 = slope tiles.
    let mut map = [0i8; 64];
    map[2 * 8 + 3] = 1;
    map[3 * 8 + 3] = 2;
    map[3 * 8 + 4] = 3;
    map[4 * 8 + 3] = 4;
    map[4 * 8 + 4] = 5;
    let (w, h) = (8i32, 8i32);

    // {p, bRow,bCol,bWx,bWy, cRow,cCol,cWx,cWy, dRow,dCol,dWx,dWy}
    let cases: [[i32; 13]; 19] = [
        [0, 2, 3, 0, 0, 2, 3, 0, 0, 2, 3, 0, 0],
        [1, -1, 3, 0, 0, -1, 3, 0, 0, -1, 3, 0, 0],
        [1, 2, 8, 0, 0, 2, 8, 0, 0, 2, 8, 0, 0],
        [1, 8, 3, 0, 0, 8, 3, 0, 0, 8, 3, 0, 0],
        [1, 2, -1, 0, 0, 2, -1, 0, 0, 2, -1, 0, 0],
        [1, 2, 3, 0, 0, 2, 3, 0, 0, 2, 3, 0, 0],
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [1, 3, 3, 10, 20, 3, 3, 10, 20, 3, 3, 10, 20],
        [1, 3, 3, 20, 10, 3, 3, 20, 10, 3, 3, 20, 10],
        [1, 3, 3, 15, 15, 3, 3, 15, 15, 3, 3, 15, 15],
        [1, 3, 4, 10, 20, 3, 4, 10, 20, 3, 4, 10, 20],
        [1, 3, 4, 20, 10, 3, 4, 20, 10, 3, 4, 20, 10],
        [1, 4, 3, 10, 20, 4, 3, 10, 20, 4, 3, 10, 20],
        [1, 4, 3, 20, 10, 4, 3, 20, 10, 4, 3, 20, 10],
        [1, 4, 4, 10, 20, 4, 4, 10, 20, 4, 4, 10, 20],
        [1, 4, 4, 20, 10, 4, 4, 20, 10, 4, 4, 20, 10],
        [1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 0, 0],
        [1, 0, 0, 0, 0, 3, 3, 10, 20, 0, 0, 0, 0],
    ];

    let mut out = String::new();
    writeln!(out, "# collision: case | blocks")?;
    for (i, cc) in cases.iter().enumerate() {
        let a = Actor {
            var_byte_p: cc[0] as i8,
            var_byte_arr_b: [cc[1] as i8, cc[2] as i8],
            var_int_arr_b: [cc[3], cc[4]],
            var_byte_arr_c: [cc[5] as i8, cc[6] as i8],
            var_int_arr_c: [cc[7], cc[8]],
            var_byte_arr_d: [cc[9] as i8, cc[10] as i8],
            var_int_arr_d: [cc[11], cc[12]],
            ..Default::default()
        };
        let blocks = i32::from(collides(&a, &map, w, h));
        writeln!(out, "{i} | {blocks}")?;
    }
    Ok(out)
}

/// The animation-trace fixture: synthetic `d`-graphs (full branch coverage) plus
/// two real models (`oh_pc`, `oh_magic`), each driven through a fixed op-script
/// and diffed against the **real `g` bytecode** invoked on the equivalent graphs.
///
/// The op-script is deterministic and identical on both sides (see `AnimOracle.java`):
/// dump the node list, probe a missing key, then for each distinct key reset +
/// 20 advances (`g.boolean a(d,int)`), reset + a seek sweep `-2..=12`
/// (`g.boolean a(d,int,int)`), recording each call's return and resulting cursor.
const ANIM_ADV_ITERS: i32 = 20;
const ANIM_SEEK_LO: i32 = -2;
const ANIM_SEEK_HI: i32 = 12;
const ANIM_MISS_KEY: i32 = 1000; // outside any i8 key, so lookup always misses

/// Synthetic model specs: `(name, &[(key, looping, frame_count)])`. Cover the
/// branches the real models may not: single-frame loop/once, multi-frame
/// loop/once, frame count beyond the seek sweep, multiple groups, duplicate keys
/// (lookup resolves to the first), and extreme `i8` keys (sign extension).
#[allow(clippy::type_complexity)]
const ANIM_SYNTH: &[(&str, &[(i32, bool, usize)])] = &[
    ("loop1", &[(0, true, 1)]),
    ("once1", &[(0, false, 1)]),
    ("loop3", &[(7, true, 3)]),
    ("once3", &[(7, false, 3)]),
    ("once5", &[(3, false, 5)]),
    ("loop7", &[(9, true, 7)]),
    (
        "multi",
        &[(0, true, 2), (1, false, 4), (-56, false, 1), (2, true, 5)],
    ),
    ("dupkey", &[(5, false, 3), (5, true, 2)]),
    (
        "extremes",
        &[(-128, false, 2), (127, true, 4), (-1, false, 1)],
    ),
];

/// Real models built from their `.cml` via `from_cml` (validates the extraction
/// of `key`/`loop`/`frame_count` on real data against the transcribed loader).
const ANIM_REAL: &[&str] = &["/oh_pc.cml", "/oh_magic.cml"];

fn anim_cur(anim: &formats::Anim, key: i32) -> i64 {
    anim.current_frame(key).map(|c| c as i64).unwrap_or(-1)
}

/// Emit the canonical op-script trace for one model (must match `AnimOracle.java`).
fn anim_trace_one(out: &mut String, name: &str, anim: &mut formats::Anim) -> Result<()> {
    let nodes = anim.nodes().to_vec();
    writeln!(out, "== {name} ==")?;
    writeln!(out, "nodes={}", nodes.len())?;
    for (i, nd) in nodes.iter().enumerate() {
        writeln!(
            out,
            "node {i} key={} loop={} frames={}",
            nd.key, nd.looping as i32, nd.frame_count
        )?;
    }
    // Missing-key probe: both primitives report "done" (true) on an absent group.
    let madv = anim.advance(ANIM_MISS_KEY) as i32;
    let mseek = anim.seek(ANIM_MISS_KEY, 0) as i32;
    writeln!(out, "miss adv={madv} seek={mseek}")?;

    let mut keys: Vec<i32> = Vec::new();
    for nd in &nodes {
        let k = i32::from(nd.key);
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    for k in keys {
        writeln!(out, "key {k}")?;
        anim.reset(k);
        writeln!(out, "  reset cur={}", anim_cur(anim, k))?;
        for t in 0..ANIM_ADV_ITERS {
            let r = anim.advance(k) as i32;
            writeln!(out, "  adv {t} r={r} cur={}", anim_cur(anim, k))?;
        }
        anim.reset(k);
        writeln!(out, "  reset cur={}", anim_cur(anim, k))?;
        for f in ANIM_SEEK_LO..=ANIM_SEEK_HI {
            let r = anim.seek(k, f) as i32;
            writeln!(out, "  seek {f} r={r} cur={}", anim_cur(anim, k))?;
        }
    }
    Ok(())
}

/// Drive every synthetic + real model through the op-script (the Rust side of
/// `anim_matches_oracle`).
pub fn dump_anim_trace(store: &AssetStore) -> Result<String> {
    use formats::{Anim, AnimNode};
    let mut out = String::new();
    writeln!(out, "# anim trace (g.java playback primitives)")?;
    for (name, spec) in ANIM_SYNTH {
        let nodes: Vec<AnimNode> = spec
            .iter()
            .map(|&(key, looping, frame_count)| AnimNode {
                key: key as i8,
                looping,
                frame_count,
            })
            .collect();
        let mut anim = Anim::from_nodes(nodes);
        anim_trace_one(&mut out, name, &mut anim)?;
    }
    for res in ANIM_REAL {
        let bytes = store.load(res).with_context(|| format!("loading {res}"))?;
        let cml = parse_cml(&bytes).with_context(|| format!("parsing {res}"))?;
        let mut anim = Anim::from_cml(&cml);
        anim_trace_one(&mut out, res, &mut anim)?;
    }
    Ok(out)
}

/// `i.a(long)` effect-pool sweep. Crafted pool images × frame sequences are run
/// through the Rust [`Effects::update`] over the real `/oh_magic.cml` model and an
/// identical synthetic 25-actor array; `Instrument.dumpEffects` installs the same
/// images into the **live `i.var_short_arr_a`** and drives the **real `i.a(long)`
/// bytecode**, dumping the 99-`short` pool after each frame. Scenarios are built so
/// the projectile hit test (`collision_hit`) never fires melee (firer + same-faction
/// actors), isolating the timer / frame-step (`g.seek`) / movement / homing /
/// lifetime logic. Must match `dump_effects_sweep` line-for-line.
const EFFECTS_FIRER_C: i32 = 1; // player at slot 0
const EFFECTS_ANCHOR_C: i32 = 2; // homing anchor at slot 1

/// `(slot_offset, [9 fields])` — fields are cast to `i16` (matching the pool).
type EffectSlot = (usize, [i32; 9]);

/// `0xFFFFF000 | c<<8 | kind`: the actor-homing `+0` form (truncated to `short`
/// when written into the pool image, matching `i.a`'s `(short)` cast).
fn effects_homing(c: i32, kind: i32) -> i32 {
    (0xFFFF_F000u32 as i32) | (c << 8) | kind
}

fn dump_effects_pool(out: &mut String, l: i64, pool: &[i16; formats::effects::POOL_LEN]) {
    use std::fmt::Write as _;
    write!(out, "frame l={l}:").ok();
    for v in pool.iter() {
        write!(out, " {v}").ok();
    }
    out.push('\n');
}

pub fn dump_effects_sweep(store: &AssetStore) -> Result<String> {
    use formats::effects::POOL_LEN;
    use formats::{Actor, Anim, Effects, JavaRandom};

    // The real /oh_magic.cml model the live `i.var_d_a` holds.
    let bytes = store
        .load("/oh_magic.cml")
        .context("loading /oh_magic.cml")?;
    let cml = parse_cml(&bytes).map_err(|e| anyhow::anyhow!("parsing oh_magic: {e}"))?;

    // Synthetic 25-actor array, all faction 0 so projectiles never hit (no melee).
    let firer = Actor {
        var_byte_c: EFFECTS_FIRER_C as i8,
        var_byte_r: 0,
        var_byte_q: 0,
        var_int_arr_b: [1000, 1000],
        ..Default::default()
    };
    let anchor = Actor {
        var_byte_c: EFFECTS_ANCHOR_C as i8,
        var_byte_r: 0,
        var_byte_q: 0,
        var_int_arr_b: [3000, 1500],
        ..Default::default()
    };

    let h1 = effects_homing(EFFECTS_FIRER_C, 0);
    let h_right = effects_homing(EFFECTS_FIRER_C, 4);
    let h_swing = effects_homing(EFFECTS_FIRER_C, 11);
    let h_anchor8 = effects_homing(EFFECTS_ANCHOR_C, 8);
    let h_down = effects_homing(EFFECTS_FIRER_C, 2);

    // (name, slots, frames). Fields: [kind/+0, x, y, stepT, frameC, ox, oy, life, lifeT].
    let f200_4: Vec<i64> = vec![200; 4];
    let f200_6: Vec<i64> = vec![200; 6];
    let f200_8: Vec<i64> = vec![200; 8];
    let f200_14: Vec<i64> = vec![200; 14];
    let f200_16: Vec<i64> = vec![200; 16];
    let f_world9: Vec<i64> = vec![60, 60, 200, 200, 200, 200, 200, 200, 200, 200];
    let scenarios: Vec<(&str, Vec<EffectSlot>, &Vec<i64>)> = vec![
        (
            "world9_life",
            vec![(0, [9, 100, 200, 0, 0, 100, 200, 600, 0])],
            &f_world9,
        ),
        (
            "world9_nolife",
            vec![(0, [9, 0, 0, 0, 0, 0, 0, 0, 0])],
            &f200_4,
        ),
        (
            "proj_up",
            vec![(0, [h1, 1000, 1000, 0, 0, 1000, 1000, 0, 0])],
            &f200_16,
        ),
        (
            "proj_right",
            vec![(0, [h_right, 1000, 1000, 0, 0, 1000, 1000, 0, 0])],
            &f200_16,
        ),
        (
            "swing_up",
            vec![(0, [h_swing, 1000, 1000, 0, 0, 1000, 1000, 0, 0])],
            &f200_8,
        ),
        (
            "homing8",
            vec![(0, [h_anchor8, 0, 0, 0, 0, 0, 0, 0, 0])],
            &f200_6,
        ),
        (
            "multi",
            vec![
                (0, [9, 100, 200, 0, 0, 100, 200, 600, 0]),
                (9, [h_down, 1000, 1000, 0, 0, 1000, 1000, 0, 0]),
            ],
            &f200_14,
        ),
    ];

    let mut out = String::new();
    writeln!(
        out,
        "# effects trace (i.a(long) over the /oh_magic.cml model)"
    )?;
    for (name, slots, frames) in &scenarios {
        writeln!(out, "# scenario {name}")?;
        let mut model = Anim::from_cml(&cml);
        let mut actors: Vec<Option<Actor>> = (0..25).map(|_| None).collect();
        actors[0] = Some(firer.clone());
        actors[1] = Some(anchor.clone());
        let mut rng = JavaRandom::new(0x00C0_FFEE);

        let mut raw = [-1i16; POOL_LEN];
        for (off, fields) in slots {
            for (k, v) in fields.iter().enumerate() {
                raw[off + k] = *v as i16;
            }
        }
        let mut e = Effects::from_raw(raw);
        for &l in frames.iter() {
            e.update(l, &mut model, &mut actors, &mut rng);
            dump_effects_pool(&mut out, l, e.raw());
        }
    }
    Ok(out)
}

/// Per-actor tick (`h.a(j,long,boolean)`) sweep — the ported subset. Synthetic
/// actors × frame sequences are run through the Rust [`Actor::tick`] (no model →
/// the animation advance is a no-op, exactly like `g.advance(null,…)`);
/// `Instrument.dumpTick` drives the same actors through the **real `h.a` bytecode**
/// and dumps the touched fields after each frame. Scenarios stay inside the ported
/// branches (timers, anim-advance gate, attack-windup, player health/fatigue regen
/// via `h.f`, the `var_byte_y` countdown, the dead-corpse timer) and avoid the
/// deferred ones (movement, DoT, NPC AI, buff-expiry, corpse removal).
///
/// `tables_path` is the captured `hf_tables.txt` (the regen recompute calls `h.f`).
fn tick_fields(out: &mut String, a: &formats::Actor) {
    use std::fmt::Write as _;
    write!(
        out,
        " {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
        a.var_short_b,
        a.var_int_a,
        a.var_int_e,
        a.var_byte_e,
        a.var_short_a,
        a.var_short_q,
        a.var_short_r,
        a.var_short_c,
        a.var_short_e,
        a.var_short_i,
        a.var_byte_w,
        a.var_short_n,
        a.var_byte_i,
        a.var_short_z,
        a.prog_a,
        a.prog_b,
        a.prog_c,
        a.prog_d
    )
    .ok();
    // Movement fields (constant in the non-movement scenarios): position, derived
    // iso + draw-tile, one tile corner, facing, step timer, target, prev position.
    write!(
        out,
        " {} {} {} {} {} {} {} {} {} {} {} {} {}",
        a.var_int_arr_b[0],
        a.var_int_arr_b[1],
        a.var_int_arr_i[0],
        a.var_int_arr_i[1],
        a.var_byte_arr_a[0],
        a.var_byte_arr_a[1],
        a.var_byte_arr_b[0],
        a.var_byte_arr_b[1],
        a.var_byte_d,
        a.var_short_g,
        a.var_int_arr_j[0],
        a.var_int_arr_e[0],
        a.var_int_arr_e[1]
    )
    .ok();
    // Buff-block fields (constant outside the P/G scenarios): the J..P bonus block
    // + H/O, the P-buff timer, recomputed max-health + rate, the G-buff duration.
    write!(
        out,
        " {} {} {} {} {} {} {} {} {} {} {} {}",
        a.p_bonus,
        a.j_bonus,
        a.k_bonus,
        a.l_bonus,
        a.m_bonus,
        a.n_bonus,
        a.h_field,
        a.o_bonus,
        a.var_short_j,
        a.var_short_o,
        a.var_short_d,
        a.g_field
    )
    .ok();
    // Floating-text fields: fade timer, rise position Q/R, color + decrement, and
    // whether the text is still active (1/0).
    write!(
        out,
        " {} {} {} {} {} {}",
        a.var_short_h,
        a.q_field,
        a.r_field,
        a.var_int_c,
        a.var_int_d,
        i32::from(a.floating_text.is_some())
    )
    .ok();
}

pub fn dump_tick_sweep(tables_path: &str) -> Result<String> {
    use formats::Actor;

    let text = std::fs::read_to_string(tables_path)
        .with_context(|| format!("reading tables fixture {tables_path}"))?;
    let tables = parse_tables(&text)?;

    // (name, actor, frames). Each actor stays in the ported subset.
    let scenarios: Vec<(&str, Actor, Vec<i64>)> = vec![
        // Timers + anim-advance gate: player at full health, no windup/move target.
        (
            "timers",
            Actor {
                var_byte_c: 1,
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                var_short_b: 100,
                var_byte_e: 2,
                ..Default::default()
            },
            vec![200, 200, 200],
        ),
        // Player attack-windup: var_short_a counts down, then var_byte_e -> 0.
        (
            "windup",
            Actor {
                var_byte_c: 1,
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                var_short_a: 500,
                var_byte_e: 3,
                ..Default::default()
            },
            vec![200, 200, 200],
        ),
        // Player health + fatigue regen (ticks via h.f every var_short_d/_f ms).
        (
            "regen",
            Actor {
                var_byte_c: 1,
                var_byte_f: 1,
                var_byte_o: 5,
                var_byte_j: 1,
                var_int_arr_n: [-1; 8],
                var_short_q: 10,
                var_short_o: 100,
                var_short_d: 300,
                var_short_r: 10,
                var_short_p: 100,
                var_short_f: 300,
                ..Default::default()
            },
            vec![200, 200, 200, 200],
        ),
        // var_byte_y == 2 status countdown.
        (
            "vy_timer",
            Actor {
                var_byte_c: 1,
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                var_byte_y: 2,
                var_short_n: 1000,
                ..Default::default()
            },
            vec![200, 200],
        ),
        // Dead-actor corpse timer (kept < 250 so corpse removal isn't reached).
        (
            "dead",
            Actor {
                var_byte_c: 1,
                var_byte_q: 1,
                ..Default::default()
            },
            vec![60, 60, 60],
        ),
        // Move toward a target (var_int_arr_j): a full-health player (so regen is
        // skipped) walks +x to the target, then arrival clears it. Exercises
        // apply_delta (h.d): position, iso/tile recompute, facing, walk-anim timer.
        (
            "move",
            Actor {
                var_byte_c: 1,
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                var_short_w: 800, // speed
                var_int_arr_b: [1000, 1000],
                var_int_arr_c: [1010, 1005],
                var_int_arr_d: [1005, 1010],
                var_int_arr_j: [1200, 1000],
                ..Default::default()
            },
            vec![200, 200, 200, 200, 200],
        ),
        // P-buff expiry: a full-health player (regen skipped) whose buff timer laps
        // the duration (P) -> strip the J..P/H block, recompute (O was set), h.f.
        (
            "pbuff",
            Actor {
                var_byte_c: 1,
                var_byte_f: 1,
                var_byte_o: 5,
                var_byte_j: 1,
                var_int_arr_n: [-1; 8],
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                var_short_s: 30,
                var_short_x: 30,
                p_bonus: 100,
                j_bonus: 5,
                k_bonus: 3,
                l_bonus: 2,
                m_bonus: 4,
                n_bonus: 1,
                o_bonus: 10,
                ..Default::default()
            },
            vec![200, 200, 200],
        ),
        // G-buff expiry: countdown that strips the same block on reaching 0.
        (
            "gbuff",
            Actor {
                var_byte_c: 2, // any actor (G is not player-gated)
                var_byte_z: 0, // non-aggressive: skip the (deferred) NPC AI branch
                var_byte_f: 1,
                var_byte_o: 5,
                var_byte_j: 1,
                var_int_arr_n: [-1; 8],
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                var_short_s: 30,
                var_short_x: 30,
                g_field: 300,
                j_bonus: 5,
                k_bonus: 3,
                l_bonus: 2,
                m_bonus: 4,
                n_bonus: 1,
                o_bonus: 10,
                ..Default::default()
            },
            vec![200, 200, 200],
        ),
        // Floating damage text fade: raise Q + fade var_int_c each >50ms step, then
        // clear once the color reaches 0.
        (
            "text",
            Actor {
                var_byte_c: 1,
                var_short_q: 100,
                var_short_o: 100,
                var_short_r: 100,
                var_short_p: 100,
                floating_text: Some("42".to_string()),
                q_field: 100,
                r_field: 100,
                var_int_c: 10,
                var_int_d: 4,
                ..Default::default()
            },
            vec![200, 200, 200, 200],
        ),
    ];

    let mut out = String::new();
    writeln!(
        out,
        "# tick sweep: scenario frame | b int_a int_e e short_a q r c e_acc i w n bi z A B C D \
         bx by ix iy ax ay bbx bby fd sg jx ex ey P J K L M N H O sj so sd G \
         sh Q R ic id txt"
    )?;
    for (name, base, frames) in &scenarios {
        let mut arr = vec![Some(base.clone())];
        let mut rng = formats::JavaRandom::new(0);
        let mut fx = formats::Effects::new();
        for (fi, &l) in frames.iter().enumerate() {
            // Array form (these scenarios never remove the actor and draw no RNG, so
            // the seed is irrelevant and the trace stays byte-identical).
            formats::Actor::tick(
                0, &mut arr, &mut rng, l, false, None, &tables, &mut fx, None,
            );
            let a = arr[0].as_ref().expect("tick scenario removed its actor");
            write!(out, "{name} {fi} |")?;
            tick_fields(&mut out, a);
            out.push('\n');
        }
    }
    Ok(out)
}

/// `var_short_k` damage-over-time sweep (the DoT lap of `h.a(j,long,boolean)`,
/// `h.java:396-403`). A non-aggressive NPC victim (slot 1) carries an active DoT
/// dealt by the actor at slot 0; [`Actor::tick`] decrements both timers and, on
/// each lap, spawns the poison effect (`i.a(8,j2)`) and applies `var_byte_x`
/// defense-bypassing damage. Per frame we dump the victim's HP/timers/dead-flag +
/// the 99-`short` effect pool; one end-of-scenario RNG probe pins the cumulative
/// draw count (the per-frame stream is sequential, so it can't carry a probe).
/// Crossing `dealer.var_byte_t` ∈ {0,1} makes that probe validate the extra-draw
/// fork (`h.java:1127`). `Instrument.dumpDoT` drives the same actors through the
/// real `h.a`; must match line-for-line.
pub fn dump_dot_sweep() -> Result<String> {
    use formats::{Actor, Effects, JavaRandom, Tables};

    // (name, dealer.var_byte_t, frames).
    let scenarios: Vec<(&str, i8, Vec<i64>)> = vec![
        // One lap (var_short_l 100 fires on frame 0); creature dealer (t=1, no extra draw).
        ("t1_1lap", 1, vec![200, 200, 200]),
        // Same, non-creature dealer (t=0, one extra var_byte_t draw per lap).
        ("t0_1lap", 0, vec![200, 200, 200]),
        // Two laps (frame 0 fires; reset to 1000; the 900ms frames re-cross), t=0
        // so the cumulative extra draws shift the end probe.
        ("t0_2lap", 0, vec![200, 900, 900]),
    ];

    let tables = Tables::default();
    let mut out = String::new();
    writeln!(
        out,
        "# dot sweep: scenario frame | q k l dead | pool[99]   (then: scenario probe <n>)"
    )?;
    for (name, dealer_t, frames) in &scenarios {
        let dealer = Actor {
            var_byte_c: 1, // slot 0
            var_byte_t: *dealer_t,
            var_int_arr_b: [4000, 4000],
            ..Default::default()
        };
        let victim = Actor {
            var_byte_c: 2,     // slot 1 (idx + 1)
            var_byte_z: 0,     // non-aggressive: skip the deferred NPC AI branch
            var_short_q: 1000, // survivable
            var_short_o: 1000,
            var_short_k: 30_000, // long DoT duration (stays > 0 across the sweep)
            var_short_l: 100,    // first lap fires on frame 0
            var_byte_x: 10,      // 10 damage per lap
            var_j_b: 0,          // dealer is at slot 0
            var_int_arr_b: [2000, 3000],
            ..Default::default()
        };
        let mut actors: Vec<Option<Actor>> = vec![Some(dealer), Some(victim)];
        let mut rng = JavaRandom::new(0x00C0_FFEE);
        let mut fx = Effects::new();
        for (fi, &l) in frames.iter().enumerate() {
            Actor::tick(
                1,
                &mut actors,
                &mut rng,
                l,
                false,
                None,
                &tables,
                &mut fx,
                None,
            );
            let v = actors[1]
                .as_ref()
                .expect("DoT victim is survivable; never removed");
            write!(
                out,
                "{name} {fi} | {} {} {} {} |",
                v.var_short_q, v.var_short_k, v.var_short_l, v.var_byte_q
            )?;
            for p in fx.raw().iter() {
                write!(out, " {p}")?;
            }
            out.push('\n');
        }
        // End probe: total draws consumed distinguishes the var_byte_t fork.
        writeln!(out, "{name} probe {}", rng.next_int())?;
    }
    Ok(out)
}

/// Corpse-removal sweep (the dead branch of `h.a`, `h.java:504-508`): a dead NPC
/// at slot 1 accumulates its corpse timer (`var_short_i`); at `>= 250`ms
/// [`Actor::tick`] removes it from the array (`actors[1] = None`). Per frame we
/// dump `present` (1/0) + `var_short_i` (`-1` once removed). `Instrument.dumpCorpse`
/// installs the same dead NPC at `b.var_j_arr_a[1]` and drives the real `h.a`
/// (which calls `b.a(1)` → nulls the slot); must match line-for-line.
pub fn dump_corpse_sweep() -> Result<String> {
    use formats::{Actor, Effects, JavaRandom, Tables};

    let tables = Tables::default();
    let mut out = String::new();
    writeln!(out, "# corpse sweep: scenario frame present var_short_i")?;
    // (name, starting var_short_i, frames).
    let scenarios: Vec<(&str, i16, Vec<i64>)> = vec![
        // Accumulate 100 -> 160 -> 220 -> 280; the 4th frame enters >= 250 -> remove.
        ("accumulate", 100, vec![60, 60, 60, 60]),
        // Already at the threshold: frame 0 enters at 250 >= 250 -> removed at once.
        ("at_threshold", 250, vec![60]),
    ];
    for (name, start_i, frames) in &scenarios {
        let npc = Actor {
            var_byte_c: 2, // slot 1 (idx + 1)
            var_byte_q: 1, // dead
            var_short_i: *start_i,
            ..Default::default()
        };
        let mut actors: Vec<Option<Actor>> = vec![None, Some(npc)];
        let mut rng = JavaRandom::new(0);
        let mut fx = Effects::new();
        for (fi, &l) in frames.iter().enumerate() {
            Actor::tick(
                1,
                &mut actors,
                &mut rng,
                l,
                false,
                None,
                &tables,
                &mut fx,
                None,
            );
            match actors[1].as_ref() {
                Some(v) => writeln!(out, "{name} {fi} 1 {}", v.var_short_i)?,
                None => writeln!(out, "{name} {fi} 0 -1")?,
            }
        }
    }
    Ok(out)
}

/// NPC attack-AI sweep (`h.boolean_b` + the melee at `h.a:457`, run from
/// [`formats::Actor::tick`]). An aggressive NPC at slot 1 of a 25-slot actor
/// array is ticked per frame; the AI scans for the nearest enemy (`h.j_a`),
/// range-checks `E`/`F`, and either steps toward it (`h.a(j,j)`), locks + faces
/// it (`h.b(j,j)`), or drops it; at `var_int_e >= var_short_m` the melee fires
/// (only vs non-players under `bl = false`). Per frame we dump the AI-owned
/// fields + the watched target's HP/`E`/text-presence; one end-of-scenario RNG
/// probe pins the melee draw count. `Instrument.dumpAI` drives the same
/// scenarios through the real `h.a`; must match line-for-line.
pub fn dump_ai_sweep() -> Result<String> {
    use formats::{Actor, Effects, JavaRandom, Tables};

    // The aggressive NPC under test (slot 1): unarmed, non-creature, with the
    // combat stats of the unarmed sweep (s=40, i=10, K=2, N=3).
    let me = || Actor {
        var_byte_c: 2,
        var_byte_r: 2,
        var_short_q: 100,
        var_short_o: 100,
        var_short_w: 800,
        e_field: 500,
        f_field: 60,
        var_short_s: 40,
        var_byte_i: 10,
        k_bonus: 2,
        n_bonus: 3,
        var_int_arr_b: [1000, 1000],
        var_int_arr_c: [1010, 1005],
        var_int_arr_d: [1005, 1010],
        var_int_arr_i: [100, 100],
        ..Default::default()
    };
    let player = |pos: [i32; 2], iso: [i32; 2]| Actor {
        var_byte_c: 1,
        var_byte_r: 1,
        var_short_q: 10_000,
        var_short_o: 10_000,
        var_int_arr_b: pos,
        var_int_arr_i: iso,
        ..Default::default()
    };
    #[allow(clippy::too_many_arguments)]
    let foe = |c: i8,
               r: i8,
               q: i16,
               o: i16,
               a: i16,
               bb: i16,
               pos: [i32; 2],
               iso: [i32; 2],
               dead: bool| {
        Actor {
            var_byte_c: c,
            var_byte_r: r,
            var_short_q: q,
            var_short_o: o,
            prog_a: a,
            prog_b: bb,
            var_int_arr_b: pos,
            var_int_arr_i: iso,
            var_byte_q: i8::from(dead),
            ..Default::default()
        }
    };
    // {slot0, me@1, slot2, slot3} in a 25-slot array (b.var_j_arr_a).
    let install = |s0: Option<Actor>, me: Actor, s2: Option<Actor>, s3: Option<Actor>| {
        let mut arr: Vec<Option<Actor>> = (0..25).map(|_| None).collect();
        arr[0] = s0;
        arr[1] = Some(me);
        arr[2] = s2;
        arr[3] = s3;
        arr
    };

    // (name, actors, watch slot (-1 = none), frames).
    let scenarios: Vec<(&str, Vec<Option<Actor>>, i32, usize)> = vec![
        // No valid target (friendly + dead foe only).
        (
            "idle",
            install(
                None,
                me(),
                Some(foe(3, 2, 100, 100, 0, 0, [1100, 1000], [110, 100], false)),
                Some(foe(4, 1, 0, 0, 0, 0, [1050, 1000], [105, 100], true)),
            ),
            -1,
            3,
        ),
        // Player inside E, outside F -> 20-unit steps toward it.
        (
            "approach",
            install(Some(player([1300, 1000], [130, 100])), me(), None, None),
            0,
            6,
        ),
        // In attack range; bl=false gates the strike but the cooldown saw-tooths.
        (
            "engage_player",
            install(Some(player([1030, 1000], [120, 80])), me(), None, None),
            0,
            12,
        ),
        // An enemy NPC in range -> the melee fires each cooldown lap.
        (
            "engage_npc",
            install(
                None,
                Actor {
                    var_int_e: 900,
                    ..me()
                },
                Some(foe(
                    3,
                    1,
                    10_000,
                    10_000,
                    10,
                    10,
                    [1030, 1000],
                    [80, 120],
                    false,
                )),
                None,
            ),
            2,
            12,
        ),
        // Held target out of aggro range -> dropped (y != 2).
        (
            "disengage",
            install(
                Some(player([3000, 3000], [300, 300])),
                Actor {
                    var_byte_e: 4,
                    var_j_a: 0,
                    ..me()
                },
                None,
                None,
            ),
            0,
            2,
        ),
        // Same, but var_byte_y == 2 keeps the target locked.
        (
            "y2_hold",
            install(
                Some(player([3000, 3000], [300, 300])),
                Actor {
                    var_byte_e: 4,
                    var_j_a: 0,
                    var_byte_y: 2,
                    var_short_n: 5000,
                    ..me()
                },
                None,
                None,
            ),
            0,
            2,
        ),
        // Facing quadrants (engage_player covers 2; engage_npc covers 1).
        (
            "face_q3",
            install(Some(player([1030, 1000], [120, 120])), me(), None, None),
            0,
            1,
        ),
        (
            "face_q4",
            install(Some(player([1030, 1000], [80, 80])), me(), None, None),
            0,
            1,
        ),
        // Equal iso x: none of the four quadrant arms match -> facing unchanged.
        (
            "face_eq",
            install(
                Some(player([1030, 1000], [100, 80])),
                Actor {
                    var_byte_d: 1,
                    ..me()
                },
                None,
                None,
            ),
            0,
            1,
        ),
    ];

    let tables = Tables::default();
    let mut out = String::new();
    writeln!(
        out,
        "# ai sweep: scenario frame | jx jy e fd inte ja bx by | tq tE ttxt   (then: scenario probe <n>)"
    )?;
    for (name, mut actors, watch, frames) in scenarios {
        let mut rng = JavaRandom::new(0xA11CE);
        let mut fx = Effects::new();
        for fi in 0..frames {
            Actor::tick(
                1,
                &mut actors,
                &mut rng,
                200,
                false,
                None,
                &tables,
                &mut fx,
                None,
            );
            let a = actors[1].as_ref().expect("AI actor is never removed");
            let (tq, te, ttxt) = if watch >= 0 {
                let t = actors[watch as usize].as_ref().unwrap();
                (
                    i64::from(t.var_short_q),
                    i64::from(t.e_field),
                    i64::from(t.floating_text.is_some()),
                )
            } else {
                (0, 0, 0)
            };
            writeln!(
                out,
                "{name} {fi} | {} {} {} {} {} {} {} {} | {tq} {te} {ttxt}",
                a.var_int_arr_j[0],
                a.var_int_arr_j[1],
                a.var_byte_e,
                a.var_byte_d,
                a.var_int_e,
                a.var_j_a,
                a.var_int_arr_b[0],
                a.var_int_arr_b[1],
            )?;
        }
        writeln!(out, "{name} probe {}", rng.next_int())?;
    }
    Ok(out)
}

/// Spell/cast-path sweep (`h.c` + the armed/creature branch of `h.a:1204`, run
/// from [`formats::Actor::tick`]). An aggressive armed/creature NPC at slot 1
/// locks the slot-2 enemy and, when the cooldown elapses, takes the spell
/// branch: `h.c(j,false)` (fatigue may go negative), the weapon-type dispatch
/// (buffs `0`/`1`/`5` across the level tiers, AoE poison `4`, cure `6`, type
/// `3`'s AoE-damage/self-heal/bolt), then the `var_byte_y == 3` weapon-drop or
/// the `y == 2` `boolean_c` vanish/teleport-wander (on a synthetic open map).
/// Per frame we dump the caster's cast-owned fields + the slot-2 target's
/// poison/damage state + the effect pool; per-scenario RNG probes pin the
/// draws. Needs `hf_tables.txt` (the cast ends in `h.f`).
/// `Instrument.dumpCast` drives the same scenarios through the real `h.a`;
/// must match line-for-line.
pub fn dump_cast_sweep(tables_path: &str) -> Result<String> {
    use formats::{Actor, Effects, JavaRandom, MapRef};

    let text = std::fs::read_to_string(tables_path)
        .with_context(|| format!("reading tables fixture {tables_path}"))?;
    let tables = parse_tables(&text)?;

    // The armed/creature caster under test (slot 1): dumpAI's NPC, plus full
    // fatigue rails and an empty inventory (so the trailing h.f skips item rows).
    let me = || Actor {
        var_byte_c: 2,
        var_byte_r: 2,
        var_short_q: 100,
        var_short_o: 100,
        var_short_r: 100,
        var_short_p: 100,
        var_short_w: 800,
        e_field: 500,
        f_field: 60,
        var_short_s: 40,
        var_byte_i: 10,
        k_bonus: 2,
        n_bonus: 3,
        var_int_e: 900,
        var_int_arr_b: [1000, 1000],
        var_int_arr_c: [1010, 1005],
        var_int_arr_d: [1005, 1010],
        var_int_arr_i: [100, 100],
        var_int_arr_n: [-1; 8],
        ..Default::default()
    };
    // {player@0 (alive, out of AoE range), me@1, enemy@2 (in F + AoE range)}.
    let install = |me: Actor| {
        let mut arr: Vec<Option<Actor>> = (0..25).map(|_| None).collect();
        arr[0] = Some(Actor {
            var_byte_c: 1,
            var_byte_r: 1,
            var_short_q: 10_000,
            var_short_o: 10_000,
            var_int_arr_b: [1200, 1200],
            var_int_arr_i: [120, 120],
            ..Default::default()
        });
        arr[1] = Some(me);
        arr[2] = Some(Actor {
            var_byte_c: 3,
            var_byte_r: 1,
            var_short_q: 10_000,
            var_short_o: 10_000,
            prog_a: 10,
            prog_b: 10,
            var_int_arr_b: [1030, 1000],
            var_int_arr_i: [80, 120],
            ..Default::default()
        });
        arr
    };
    // Shared weapon row template; [2] (type) and [1] (61618/61619 id) vary.
    //              [0][1][2][3][4] [5] [6]  [7][8][9][10][11][12][13][14]
    let w_base = [0, 0, 0, 7, 14, 21, 900, 0, 0, 5, 10, 5, 8, 12, 200];
    let w_row = |ty: i32, id: i32| {
        let mut w = w_base.to_vec();
        w[2] = ty;
        w[1] = id;
        Some(w)
    };

    // (name, me, frames).
    let scenarios: Vec<(&str, Actor, usize)> = vec![
        // creature (t=1): kind-11 swing remapped by facing; no weapon needed.
        (
            "creature",
            Actor {
                var_byte_t: 1,
                ..me()
            },
            2,
        ),
        // The three buff types across the three level tiers.
        (
            "buff_l",
            Actor {
                var_int_arr_l: w_row(0, 0),
                var_byte_o: 1,
                ..me()
            },
            1,
        ),
        (
            "buff_n",
            Actor {
                var_int_arr_l: w_row(1, 0),
                var_byte_o: 7,
                ..me()
            },
            1,
        ),
        (
            "buff_h",
            Actor {
                var_int_arr_l: w_row(5, 0),
                var_byte_o: 12,
                ..me()
            },
            1,
        ),
        // aoe_poison (type 4): the slot-2 enemy is inside range [14].
        (
            "aoe_poison",
            Actor {
                var_int_arr_l: w_row(4, 0),
                var_byte_o: 1,
                ..me()
            },
            1,
        ),
        // cure (type 6): the caster's own active DoT is cleared.
        (
            "cure",
            Actor {
                var_int_arr_l: w_row(6, 0),
                var_byte_o: 1,
                var_short_k: 5000,
                var_short_l: 500,
                var_byte_w: -47,
                ..me()
            },
            1,
        ),
        // aoe_damage (type 3, id 61618): full-defense damage on the enemy.
        (
            "aoe_damage",
            Actor {
                var_int_arr_l: w_row(3, 61618),
                var_byte_o: 1,
                ..me()
            },
            1,
        ),
        // heal (type 3, id 61619): self-heal clamped to max.
        (
            "heal",
            Actor {
                var_int_arr_l: w_row(3, 61619),
                var_byte_o: 1,
                var_short_q: 50,
                ..me()
            },
            1,
        ),
        // bolt (type 3, other id): kind-0 projectile remapped by facing.
        (
            "bolt",
            Actor {
                var_int_arr_l: w_row(3, 0),
                var_byte_o: 1,
                ..me()
            },
            1,
        ),
        // weapon_drop (var_byte_y == 3): the row is dropped and F halves.
        (
            "weapon_drop",
            Actor {
                var_int_arr_l: w_row(0, 0),
                var_byte_o: 1,
                var_byte_y: 3,
                ..me()
            },
            1,
        ),
        // wander (var_byte_y == 2): vanish on the first strike, return once
        // var_short_n <= -1000, on a fully-open 20x20 map.
        (
            "wander",
            Actor {
                var_int_arr_l: w_row(0, 0),
                var_byte_o: 1,
                var_byte_y: 2,
                var_short_n: 0,
                ..me()
            },
            6,
        ),
    ];

    let base = vec![1i8; 400];
    let coll = vec![0i8; 400];
    let map = MapRef {
        base: &base,
        coll: &coll,
        height: 20,
    };

    let mut out = String::new();
    writeln!(
        out,
        "# cast sweep: scenario frame | r q k l w h G L N Hf A n F hasW inte ja bx by | \
         tq tk tx tw tjb tE ttxt | pool[99]   (then: scenario probe <n>)"
    )?;
    for (name, caster, frames) in scenarios {
        let mut actors = install(caster);
        let mut rng = JavaRandom::new(0xCA57E);
        let mut fx = Effects::new();
        for fi in 0..frames {
            Actor::tick(
                1,
                &mut actors,
                &mut rng,
                200,
                false,
                None,
                &tables,
                &mut fx,
                Some(&map),
            );
            let a = actors[1].as_ref().expect("caster is never removed");
            let t = actors[2].as_ref().expect("target is never removed");
            write!(
                out,
                "{name} {fi} | {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} | \
                 {} {} {} {} {} {} {} |",
                a.var_short_r,
                a.var_short_q,
                a.var_short_k,
                a.var_short_l,
                a.var_byte_w,
                a.var_byte_h,
                a.g_field,
                a.l_bonus,
                a.n_bonus,
                a.h_field,
                a.a_phase,
                a.var_short_n,
                a.f_field,
                i32::from(a.var_int_arr_l.is_some()),
                a.var_int_e,
                a.var_j_a,
                a.var_int_arr_b[0],
                a.var_int_arr_b[1],
                t.var_short_q,
                t.var_short_k,
                t.var_byte_x,
                t.var_byte_w,
                t.var_j_b,
                t.e_field,
                i32::from(t.floating_text.is_some()),
            )?;
            for p in fx.raw().iter() {
                write!(out, " {p}")?;
            }
            out.push('\n');
        }
        writeln!(out, "{name} probe {}", rng.next_int())?;
    }
    Ok(out)
}

/// Run the Rust movement port (`world::move_in_world` = `h.void_a`) over the same
/// scripted (direction, dt) sequence the oracle drives through the real method, on
/// an identical synthetic collision map — a position trace, diffed step-by-step.
pub fn dump_move_sweep() -> Result<String> {
    use formats::{move_in_world, set_position, Actor};

    let mut map = [0i8; 64];
    map[3 * 8 + 4] = 1; // wall at col 4 of the start tile
    let (w, h) = (8i32, 8i32);

    let script: [(i32, i64); 22] = [
        (3, 30),
        (3, 30),
        (3, 60),
        (3, 60),
        (3, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (1, 60),
        (2, 60),
        (2, 60),
        (4, 60),
        (4, 60),
        (4, 60),
    ];

    let mut a = Actor {
        var_short_w: 200,
        var_byte_a: 0,
        var_byte_b: 0,
        var_byte_p: 1,
        ..Default::default()
    };
    set_position(&mut a, 384, 384);

    let mut out = String::new();
    writeln!(
        out,
        "# move: step dir dt | bx by ix iy tbx tby facing g anim"
    )?;
    for (i, &(dir, dt)) in script.iter().enumerate() {
        move_in_world(&mut a, dir, dt, &map, w, h);
        writeln!(
            out,
            "{i} {dir} {dt} | {} {} {} {} {} {} {} {} {}",
            a.var_int_arr_b[0],
            a.var_int_arr_b[1],
            a.var_int_arr_i[0],
            a.var_int_arr_i[1],
            a.var_byte_arr_b[0],
            a.var_byte_arr_b[1],
            a.var_byte_d,
            a.var_short_g,
            a.var_short_a
        )?;
    }
    Ok(out)
}

/// Run the Rust XP/level-up port (`Actor::award_xp`) over the same sweep the
/// oracle (`Instrument.dumpXp`) drives through the real `h.c`. First emits the XP
/// tables from the Rust constants (the oracle emits the real `h.var_short_arr_a/b`,
/// so the diff directly validates those 52 numbers), then the per-case results.
/// Reads the stat-table fixture because a level-up runs `h.f`.
pub fn dump_xp_sweep(tables_path: &str) -> Result<String> {
    use formats::actor::{XP_REWARD, XP_THRESHOLD};
    use formats::Actor;

    let text = std::fs::read_to_string(tables_path)
        .with_context(|| format!("reading tables fixture {tables_path}"))?;
    let tables = parse_tables(&text)?;

    let levels = [1i32, 4, 5, 9, 14, 19, 24];
    let races = [1i32, 7, 13, 19, 25];
    let cfgs = [(30_000i32, 5usize), (0, 1)]; // (startXP, n): 0=levelup, 1=no-levelup

    let mut out = String::new();
    write!(out, "threshold")?;
    for v in XP_THRESHOLD {
        write!(out, " {v}")?;
    }
    write!(out, "\nreward")?;
    for v in XP_REWARD {
        write!(out, " {v}")?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "# xp sweep: class lvl race cfg | int_b o s t u v w x y maxH maxF rH rF i z A B C D"
    )?;
    for cls in 1..=8u8 {
        for &lvl in &levels {
            for &race in &races {
                for (cfg, &(start_xp, n)) in cfgs.iter().enumerate() {
                    let mut a = Actor {
                        var_byte_c: 1,
                        var_byte_f: cls as i8,
                        var_byte_j: race as i8,
                        var_byte_o: lvl as i8,
                        var_int_b: start_xp,
                        var_int_arr_n: [-1; 8],
                        var_short_s: 30,
                        var_short_t: 30,
                        var_short_u: 30,
                        var_short_v: 30,
                        var_short_w: 30,
                        var_short_x: 30,
                        var_short_y: 30,
                        o_bonus: 0,
                        i_bonus: 0,
                        j_bonus: 0,
                        ..Default::default()
                    };
                    a.award_xp(n, &tables);
                    writeln!(
                        out,
                        "{cls} {lvl} {race} {cfg} | {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                        a.var_int_b, a.var_byte_o,
                        a.var_short_s, a.var_short_t, a.var_short_u, a.var_short_v,
                        a.var_short_w, a.var_short_x, a.var_short_y,
                        a.var_short_o, a.var_short_p, a.var_short_d, a.var_short_f,
                        a.var_byte_i, a.var_short_z,
                        a.prog_a, a.prog_b, a.prog_c, a.prog_d
                    )?;
                }
            }
        }
    }
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
