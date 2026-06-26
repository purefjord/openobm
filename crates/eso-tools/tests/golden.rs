//! Golden-master tests over the *real* game assets (GOAL.md section 9):
//!  - every `.jtm` and every `lang_*.txt` decodes with no EOF/overflow;
//!  - their parsed form is pinned by an `insta` snapshot so any drift surfaces
//!    as a reviewable diff. The `.jtm` snapshot is a compact per-layer-hash
//!    summary (full grids go to fixtures for oracle diffing); the lang snapshot
//!    is the full canonical dump (small enough to read).
//!
//! These snapshots are *self-consistency* anchors. Validating them against the
//! original binary is the oracle's job (see crates/eso-tools + oracle/); once
//! oracle fixtures exist, `tests/oracle_match.rs` diffs the canonical dumps.

use eso_tools::{dump_lang, summarize_jtm};
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
    let dump = dump_lang(&assets(), &[]).expect("all lang files decode");
    insta::assert_snapshot!("lang_dump", dump);
}
