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
                                melee_attack(&attacker, &mut target, true, &mut rng);
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
                let (died, outcome) = melee_attack(&attacker, &mut target, bl, &mut rng);
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
