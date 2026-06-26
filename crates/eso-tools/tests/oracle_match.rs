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

use eso_tools::{dump_jtm, dump_lang};
use formats::AssetStore;

fn assets() -> AssetStore {
    AssetStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

fn oracle(name: &str) -> String {
    let path = format!(
        "{}/../../tests/fixtures/oracle/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
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
