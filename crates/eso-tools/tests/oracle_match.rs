//! Oracle agreement (the load-bearing test, per GOAL.md section 2).
//!
//! The fixtures under `tests/fixtures/oracle/` were produced by
//! `oracle/OracleDump.java` — a verbatim transcription of the original game's
//! `.jtm`/lang loader algorithms, run on the JVM. This test regenerates the
//! Rust port's canonical dump and asserts it is **byte-identical** to the
//! oracle. Correctness here comes from the original algorithm, not judgement; a
//! flipped tile index or a wrong shift fails this mechanically.
//!
//! To refresh the oracle fixtures (only when the original algorithm's reading
//! changes, which it never should):
//!   cd oracle && javac OracleDump.java
//!   java OracleDump jtm  ../assets > ../tests/fixtures/oracle/jtm_canonical.txt
//!   java OracleDump lang ../assets > ../tests/fixtures/oracle/lang_canonical.txt

use eso_tools::{dump_cml, dump_hf_sweep, dump_jtm, dump_lang, dump_scr, dump_scr_trace};
use formats::AssetStore;

fn assets() -> AssetStore {
    AssetStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

fn fixture_path(name: &str) -> String {
    format!(
        "{}/../../tests/fixtures/oracle/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn oracle(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing oracle fixture {path}: {e}"))
        // Normalize CRLF in case git touched line endings on checkout; the Rust
        // dumper emits LF, so compare on LF.
        .replace("\r\n", "\n")
}

/// Find the first differing line for a readable failure message.
fn assert_identical(rust: &str, oracle: &str, what: &str) {
    if rust == oracle {
        return;
    }
    let mut r = rust.lines();
    let mut o = oracle.lines();
    let mut line = 0;
    loop {
        line += 1;
        match (r.next(), o.next()) {
            (Some(a), Some(b)) if a == b => continue,
            (a, b) => panic!("{what} mismatch at line {line}:\n  rust:   {a:?}\n  oracle: {b:?}"),
        }
    }
}

#[test]
fn jtm_matches_oracle() {
    let rust = dump_jtm(&assets(), &[]).expect("rust jtm dump");
    assert_identical(&rust, &oracle("jtm_canonical.txt"), "jtm");
}

#[test]
fn lang_matches_oracle() {
    let rust = dump_lang(&assets(), &[]).expect("rust lang dump");
    assert_identical(&rust, &oracle("lang_canonical.txt"), "lang");
}

#[test]
fn cml_matches_oracle() {
    let rust = dump_cml(&assets(), &[]).expect("rust cml dump");
    assert_identical(&rust, &oracle("cml_canonical.txt"), "cml");
}

#[test]
fn scr_loader_matches_oracle() {
    let rust = dump_scr(&assets(), &[]).expect("rust scr dump");
    assert_identical(&rust, &oracle("scr_canonical.txt"), "scr");
}

#[test]
fn scr_trace_startup_matches_oracle() {
    let rust = dump_scr_trace(&assets(), "/startup.scr", 1, 4096).expect("rust scr trace");
    assert_identical(&rust, &oracle("scr_trace_startup.txt"), "scr-trace");
}

/// `h.f` (class/level/race progression). The Rust port runs the same synthetic
/// actor sweep over the *same* live stat tables (`hf_tables.txt`, captured from
/// the running game's `b.var_e_a`) that the FreeJ2ME oracle drove through the
/// real `h.f` bytecode (`hf_sweep.txt`). Byte-identical = the port reproduces the
/// real method exactly — including its redundant double-writes and level
/// breakpoints. This is real-bytecode ground truth, not a transcription.
#[test]
fn hf_matches_oracle() {
    let rust = dump_hf_sweep(&fixture_path("hf_tables.txt")).expect("rust h.f sweep");
    assert_identical(&rust, &oracle("hf_sweep.txt"), "hf");
}
