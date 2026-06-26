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
use formats::{parse_jtm, parse_lang_file, AssetStore};

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
