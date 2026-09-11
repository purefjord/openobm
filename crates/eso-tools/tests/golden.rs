//! Golden-master tests over the *real* game assets (GOAL.md section 9):
//!  - every `.jtm` and every `lang_*.txt` decodes with no EOF/overflow;
//!  - their parsed form is pinned by an `insta` snapshot so any drift surfaces
//!    as a reviewable diff. The `.jtm` snapshot is a compact per-layer-hash
//!    summary (full grids go to fixtures for oracle diffing); the lang
//!    snapshot is likewise a per-table entry-count + hash summary, because the
//!    full dump is the game's own writing and is never committed.
//!
//! These snapshots are *self-consistency* anchors. Validating them against the
//! original binary is the oracle's job (see crates/eso-tools + oracle/); once
//! oracle fixtures exist, `tests/oracle_match.rs` diffs the canonical dumps.

use eso_tools::{scr_coverage, summarize_cml, summarize_jtm, summarize_lang};
use formats::AssetStore;

fn assets() -> AssetStore {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    AssetStore::new(dir)
}

#[test]
fn all_jtm_decode_and_match_snapshot() {
    let summary = summarize_jtm(&assets()).expect("all .jtm maps decode");
    // 17 maps expected in the asset set.
    assert_eq!(
        summary.lines().count(),
        17,
        "unexpected number of .jtm maps"
    );
    insta::assert_snapshot!("jtm_summary", summary);
}

#[test]
fn all_lang_decode_and_match_snapshot() {
    let summary = summarize_lang(&assets()).expect("all lang files decode");
    // 13 tables expected in the asset set.
    assert_eq!(
        summary.lines().count(),
        13,
        "unexpected number of lang tables"
    );
    insta::assert_snapshot!("lang_summary", summary);
}

#[test]
fn all_cml_decode_and_match_snapshot() {
    let summary = summarize_cml(&assets()).expect("all .cml models decode");
    assert_eq!(
        summary.lines().count(),
        21,
        "unexpected number of .cml models"
    );
    insta::assert_snapshot!("cml_summary", summary);
}

#[test]
fn scr_coverage_snapshot() {
    let report = scr_coverage(&assets()).expect("all .scr scripts disassemble");
    insta::assert_snapshot!("scr_coverage", report);
}
