# OpenOBM

**An open reimplementation of *The Elder Scrolls Travels: Oblivion* (2006, Java ME)
— in Rust, validated byte-for-byte against the original bytecode.**

*Oblivion Mobile* was an isometric action-RPG released for J2ME feature phones in
2006 by Vir2L Studios. OpenOBM is a compatibility-first rewrite of its engine:
the whole game, from the prison cell to the end credits, ported and **per-beat
byte-validated** against the original.

No prior reimplementation of this title is known. The closest comparable work in
the *Travels* series is [Shadowkey-RE][sk] — a different game, and data
extraction rather than a playable port.

> **OpenOBM ships no game data.** Running it requires your own legally obtained
> copy of the original. See [Supplying the game data](#supplying-the-game-data).

[sk]: https://github.com/minexew/Shadowkey-RE

## What "byte-validated" means here


Correctness is anchored to the **original binary's computation** — captured into
byte-comparable fixtures and frozen into automated tests, never to hand
judgement. Two independent faithful ports (this Rust one and a JVM transcription)
agreeing byte-for-byte across every real asset is the cross-check; where behavior
is runtime-coupled, the original `.jar` itself runs headless on FreeJ2ME and its
LCD frames are diffed pixel-exact.

This repository delivers milestones **M0 → M13** — the whole game, end to end.
Current state, and what is left, live in `HANDOFF.md`.

## What is proved


Every shipped format parses byte-exact against the original, and the interactive
loop is diffed frame-by-frame against the real game running on FreeJ2ME:

| Surface | Scope | Result |
|---|---|---|
| Data formats | `.jtm` maps, `lang` tables, `.cml` models, `.scr` scripts (loader, all 79 opcodes, stat tables), `ESO` saves | byte-identical |
| Game logic | 15 sweeps against real bytecode — progression, melee, XP, targeting, collision, movement, animation, effects, per-actor tick, AI, spellcasting | byte-identical |
| The shell | 70 pixel-parity frames — boot, menus, gameplay, shop, save/load, death, credits | byte-identical |
| Every level | l01 through l12 + the ending, per-beat validated; 10 sequential loads in one session | byte-identical |

**→ [`docs/validation.md`](docs/validation.md)** for the full table, every row.
**→ [`docs/milestones.md`](docs/milestones.md)** for how it was built, M0 to M13.

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
  game/        the ported `b.java` shell — modes, menus, gameplay, save/load
oracle/        OracleDump.java — original loader algorithms, as ground truth
               patches/ — instrumented FreeJ2ME sources (GPLv3, see NOTICE.md)
docs/          how it was built, what to play-test, what remains
tools/         extract-assets.{ps1,sh} — unpack your jar into assets/

# NOT in this repo — you create these locally (see below):
assets/          your extracted game data
tests/fixtures/  regenerated oracle dumps + captured frames
artifacts/       rendered screenshots
```

## Supplying the game data


OpenOBM is an engine. It contains no assets, no string tables, no sprites and no
disassembly — `.gitignore` blocks all of them, and nothing of the sort exists
anywhere in this repository's history.

To get to a working tree you need:

1. **The original MIDlet** (`Oblivion.jar`), from your own copy of the game.
2. **Its resources unpacked into `assets/`.** A `.jar` is a zip and the game's
   resources sit at its root, so this is just an unzip with the Java classes
   dropped. There is a script for it:

   ```sh
   pwsh tools/extract-assets.ps1 path/to/Oblivion.jar   # Windows
   sh   tools/extract-assets.sh  path/to/Oblivion.jar   # Linux/macOS (needs unzip)
   ```

   Both write to `assets/` and expect ~112 resource files. No installer, no
   wizard, no config file — the engine reads whichever directory you point it at.
3. **The fixtures regenerated** from those assets, via the oracle:

   ```sh
   mkdir -p tests/fixtures/oracle
   cd oracle && javac OracleDump.java
   java OracleDump jtm  ../assets > ../tests/fixtures/oracle/jtm_canonical.txt
   java OracleDump lang ../assets > ../tests/fixtures/oracle/lang_canonical.txt
   java OracleDump scr  ../assets > ../tests/fixtures/oracle/scr_canonical.txt
   ```

   `oracle/README.md` documents the full set, including the FreeJ2ME runtime
   oracle behind frame parity.

   The `lang` golden snapshot is deliberately absent — it would be a verbatim
   copy of the game's writing. On your first `cargo test` with assets present,
   `insta` generates it locally and asks you to accept it (`cargo insta
   accept`); it stays ignored by git.

Without these the crates still build and the pure-logic tests still pass; the
fixture-gated suites fail for want of their inputs.

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
cargo run -p eso-tools -- jtm|lang|cml|scr|scr-trace|scr-exec|scr-coverage ./assets
cargo run -p eso-tools -- save-roundtrip path/to/eso_blob.bin
cargo run -p render --bin map-shot -- ./assets l01_1.jtm artifacts/l01_1.png
cargo run -p render --bin sprite-shot -- ./assets oh_pc.cml c1.png l01_1.jtm artifacts/pc
cargo run -p render --features interactive --bin map-view -- ./assets l01_1.jtm
```

> \* miri did not run to completion in the CI sandbox (it hangs on its first-run
> instrumented-sysroot build). It is low-value for this code regardless: the
> `formats` crate contains **no `unsafe`**, so an out-of-bounds access is a clean
> panic, not silent UB. The intended coverage — "catch OOB/UB the moment
> `x*height+y` is fumbled" — is already provided by `overflow-checks = true`
> (dev/test) plus the proptest fuzzers, which exercise every parser and the VM on
> thousands of random inputs without panicking.

## Beyond the port


Two things here are **not** part of the faithful port. They are built on top of
the validated engine, they are gated by nothing, and they have no tests. Demos,
not guarantees:

- **`mapforge`** — a custom-map generator. It writes a `.jtm` map and a `.scr`
  script from scratch and boots them in the engine. Everything else in this
  repository proves the original's formats can be *read* byte-exactly; mapforge
  is the proof they can be *written* too, which is what makes a level editor
  plausible (`docs/editor-feasibility.html`). It touches no shipped file and no
  fixture — custom content only.

  ```sh
  cargo run -p game --bin mapforge --release -- world    # a walking-sim level
  cargo run -p game --bin mapforge --release -- palette  # a tile contact sheet
  ```

- **widescreen** (`play wide` / `wide10`) — a viewport wider than the original's
  240x320. Good for looking around; deliberately outside the parity gates,
  because the original's framing is part of what gets validated.

The port itself stays pure: neither is compiled into the validation path.

## Audio: none — the original is silent


Verified against the real bytecode (2026-07-23): no class in the jar
references `javax.microedition.media` (or any vendor audio API), the jar
ships zero audio assets, and the menu build never surfaces the vestigial
"Sound:" toggle (its fire branch is dead code, ported faithfully and pinned
by `tests/sound_toggle.rs`). A 1:1 port of a silent game is silent — audio
is out of scope by *fidelity*, not omission. See `docs/road-to-1.0.md`.

## Legal


Unofficial and non-commercial. Not affiliated with, endorsed by, or approved by
ZeniMax Media, Bethesda Softworks, or Vir2L Studios. Engine and documentation are
dual-licensed **MIT OR Apache-2.0**; `oracle/patches/` is **GPLv3**. Trademarks,
the no-game-data guarantee, and the full licensing picture: **`NOTICE.md`**.

## How this was built


OpenOBM was written with [Claude Code](https://claude.com/claude-code) — Fable 5,
Opus 5, and Opus 4.8 — over 11 weeks and 106 commits, June to September 2026.

```
26,050 lines of Rust      the engine
 6,816 lines of Java      the oracle harness
    32 test files         45 suites, byte-gated against the original
    26 validation loops   each one a level or subsystem proved byte-exact
```

Every correctness claim in this README was established mechanically, by diffing
against the original binary — never by hand judgement. That is the whole method:
the model does not get to decide whether the port is right, the original does.
