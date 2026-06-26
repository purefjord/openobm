# Travels Oblivion — compatibility-first Rust port

A compatibility-first Rust rewrite of *The Elder Scrolls Travels: Oblivion*
(Superscape, Java ME / MIDP-1.0). Correctness is anchored to the **original
binary's computation**, captured into byte-comparable fixtures and frozen into
automated tests — never to hand judgement. See `../GOAL.md` and `../spec.txt`.

This repository delivers milestones **M0 → M3** of that plan.

## Workspace layout

```
crates/
  formats/     pure, graphics-free parsers (fuzzable in isolation)
    reader.rs    one big-endian / unsigned-byte reader all parsers use
    iso.rs       world<->screen isometric transforms (verified shifts)
    lang.rs      lang_*.txt string tables
    jtm.rs       .jtm RLE tile maps
    cml.rs       .cml model/animation/sprite tables
    scr.rs       .scr script loader (entry table + data sections + bytecode)
    vm.rs        .scr bytecode VM skeleton (79-opcode decoder + control flow)
  eso-tools/   `eso-dump` — byte-comparable canonical dumps + scr-coverage
  render/      debug isometric map renderer behind a `Renderer` trait
oracle/        OracleDump.java — original loader algorithms, as ground truth
tests/fixtures/  rust/ and oracle/ canonical dumps (the mechanical diff)
artifacts/     rendered PNG screenshots
```

## The oracle (how correctness is established)

The reference project (RECON 2010 Syndicate Wars port) kept the original binary
continuously runnable as an in-process oracle. JVM bytecode and Rust can't
co-execute, so we rebuild that baseline **out of process**: `oracle/OracleDump.java`
is a *verbatim transcription* of the original game's loader algorithms
(`b.java` for `.jtm`/lang, `e.java` for `.scr`), run on the JVM, emitting the
exact same canonical text format as the Rust `eso-dump`. Validation is then a
mechanical `diff`. Two independent faithful ports (Rust + Java) agreeing
byte-for-byte across all real assets is the cross-check.

Current oracle agreement (`cargo test -p eso-tools --test oracle_match`):

| Artifact | Scope | Result |
|---|---|---|
| `.jtm` grids | all 17 maps, all layers | byte-identical |
| `lang_*.txt` | all 13 tables, 546 entries | byte-identical |
| `.scr` loader + stat tables | all 32 scripts (entries, code, sections w/ full row values + aux lists) | byte-identical |
| `.scr` opcode trace | `startup.scr` entry 1, full | byte-identical |
| `.cml` models | all 21 (records, flags, boxes, anim groups/frames) | byte-identical |

> For the data formats the algorithm is fully self-contained, so the JVM
> transcription *is* equivalent ground truth. For runtime-coupled behavior
> (M7+), a second oracle runs the **actual `Oblivion.jar` on FreeJ2ME, headless**
> — verified booting through the `startup.scr` splash to the title screen from
> the original bytecode (see `oracle/README.md` and `artifacts/real_20s.png`).
> This is the foundation for byte-diffing runtime behavior (opcode execution
> traces, save blobs) and for screenshot parity.

## Verified corrections to `spec.txt`

Checked against the decompiled source / bytecode (the spec said to trust but verify):

1. **Lang lookup: base wins over overlay.** `b.java::java_lang_String_a` checks
   the base table (`lang_0`) first and only falls back to the secondary table for
   ids missing from the base. `spec.txt` claimed overlay wins; the bytecode says
   the opposite. (`lang.rs`)
2. **Lang text is Latin-1, not UTF-8.** The Java widens each byte with `(char)n`
   (ISO-8859-1). Decoding as UTF-8 (as the spec sketch did) would corrupt the
   German/French translation files. (`lang.rs`)
3. **`.jtm` confirmed:** tile index is `x*height+y` (not row-major), y-outer /
   x-inner; RLE `0xFF count value`; a `count==0` run still writes one cell. (`jtm.rs`)
4. **Iso transform round-trip:** `world->screen` is lossy; `screen->world->screen`
   is the exact identity (the invariant the renderer relies on). (`iso.rs`)

## Milestones delivered

- **M0** — workspace + quality gates (clippy `-D warnings`, rustfmt, dev
  `overflow-checks`, `insta`, `proptest`, miri); `Reader`, `AssetStore`, iso
  transforms with exact-shift and round-trip tests.
- **M1** — `lang_*.txt` + every `.jtm` parsed; golden `insta` snapshots over all
  real assets; `proptest` fuzz; **oracle byte-match**.
- **M2** — debug isometric renderer behind a `Renderer` trait, with a headless
  CPU/PNG backend (verifiable in CI, see `artifacts/`) and a macroquad
  interactive window (`--features interactive`: arrow/WASD pan, tile-under-cursor).
- **M3** — `.scr` loader + a 79-opcode VM **skeleton**: every opcode's operands
  are decoded exactly as the original (PC advances identically), with call/return/
  wait control flow modeled. The `startup.scr` opcode trace matches the oracle
  opcode-for-opcode; `scr-coverage` disassembles entry 1 of all 32 scripts and
  reaches 54/79 opcodes with **zero unknown-opcode hits**. Opcode *side effects*
  (rendering, actor mutation, resource loading) are deferred to later milestones.
- **M4** — `.cml` model/animation parser (`g.java`/`d.java`): path prefix, frame
  records, bit-flag blocks, bounding boxes, animation groups/frames. All 21 files
  consume exactly to EOF and **match the oracle byte-for-byte**. A decompiler trap
  in the path read was resolved against `g.class` bytecode (`javap -c`). Sprite
  *rendering* (decoding the referenced PNGs + frame placement) is deferred to the
  renderer milestone.
- **M5** — `.scr` data tables materialized: the section sub-parsers now capture
  full row values (actor/item/spell/etc. stats) with each subtype's exact
  signedness, inline-string-pool indexing, and the two global lists (subtype 7's
  flat list, subtype 9's slot list). All 32 scripts' tables **match the oracle
  byte-for-byte**. (Executing opcode side effects remains future work.)

## Build & test

```sh
cargo test --workspace          # unit + golden + fuzz + oracle-match
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly miri test -p formats --lib         # UB / OOB check on parsers*

# regenerate oracle fixtures (only if the original algorithm reading changes)
cd oracle && javac OracleDump.java
java OracleDump jtm  ../assets > ../tests/fixtures/oracle/jtm_canonical.txt
java OracleDump lang ../assets > ../tests/fixtures/oracle/lang_canonical.txt
java OracleDump scr  ../assets > ../tests/fixtures/oracle/scr_canonical.txt
java OracleDump scr-trace ../assets /startup.scr 1 > ../tests/fixtures/oracle/scr_trace_startup.txt

# tools
cargo run -p eso-tools -- jtm|lang|cml|scr|scr-trace|scr-coverage ./assets
cargo run -p render --bin map-shot -- ./assets l01_1.jtm artifacts/l01_1.png
cargo run -p render --features interactive --bin map-view -- ./assets l01_1.jtm
```

> \* miri did not run to completion in the CI sandbox (it hangs on its first-run
> instrumented-sysroot build). It is low-value for this code regardless: the
> `formats` crate contains **no `unsafe`**, so an out-of-bounds access is a clean
> panic, not silent UB. The intended coverage — "catch OOB/UB the moment
> `x*height+y` is fumbled" — is already provided by `overflow-checks = true`
> (dev/test) plus the proptest fuzzers, which exercise every parser and the VM on
> thousands of random inputs without panicking.
