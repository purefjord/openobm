# OpenOBM

**An open reimplementation of *The Elder Scrolls Travels: Oblivion* (2006, Java ME)
— in Rust, validated byte-for-byte against the original bytecode.**

*Oblivion Mobile* was an isometric action-RPG released for J2ME feature phones in
2006 by Vir2L Studios. OpenOBM is a compatibility-first rewrite of its engine:
the whole game, from the prison cell to the end credits.

No prior reimplementation of this title is known. The closest comparable work in
the *Travels* series is [Shadowkey-RE][sk] — a different game, and data
extraction rather than a playable port.

<p align="center">
  <img src="screenshots/gameplay.png" alt="The Imperial City Prison, running in OpenOBM at the original 240x320" width="240">
  <br>
  <em>The Imperial City Prison — OpenOBM, at the original 240&times;320.</em>
</p>

> **OpenOBM ships no game data.** Running it requires your own legally obtained
> copy of the original. See [Supplying the game data](#supplying-the-game-data).

[sk]: https://github.com/minexew/Shadowkey-RE

## Status

The whole game is ported and loads end to end, and every shipped format parses
byte-exact. **It has not yet been played start to finish by a human**, so it is
not 1.0: softlocks, dead ends, and wrongness in places no fixture looks are the
open risk. Treat it as feature-complete and unproven in play.

## What is proved

Correctness was anchored to the **original binary's computation**, not to
judgement about whether output looks right. Each surface below was established
by diffing against the original — the data formats against a second, independent
transcription of the original loader algorithms, and the interactive loop against
the real `.jar` running headless on FreeJ2ME, compared frame by frame:

| Surface | Scope | Result |
|---|---|---|
| Data formats | `.jtm` maps, `lang` tables, `.cml` models, `.scr` scripts (loader, all 79 opcodes, stat tables), `ESO` saves | byte-identical |
| Game logic | 15 sweeps against real bytecode — progression, melee, XP, targeting, collision, movement, animation, effects, per-actor tick, AI, spellcasting | byte-identical |
| The shell | 70 pixel-parity frames — boot, menus, gameplay, shop, save/load, death, credits | byte-identical |
| Every level | l01 through l12 plus the ending, per-beat validated; ten sequential loads in one session | byte-identical |

The harness that produced those fixtures — a JVM transcription of the original
loaders, plus an instrumented FreeJ2ME — is **not part of this repository**. What
ships here is the engine. The table records what the port was measured against
during development; it is not reproducible from this repository alone.

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
    vm.rs        .scr bytecode VM (79-opcode decoder + control flow)
  eso-tools/   `eso-dump` — byte-comparable canonical dumps + scr-coverage
  render/      isometric map/sprite renderer behind a `Renderer` trait
  game/        the ported shell — modes, menus, gameplay loop, save/load
tools/         extract-assets.{ps1,sh} — unpack your jar into assets/
tests/drives/  input scripts that drive the shell through scripted play
docs/          controls

# NOT in this repo — you create these locally (see below):
assets/          your extracted game data
tests/fixtures/  the byte-comparison fixtures
```

## Supplying the game data

OpenOBM contains no assets, no string tables, no sprites and no disassembly.
`.gitignore` blocks all of them, and nothing of the sort exists anywhere in this
repository's history.

1. **The original MIDlet** (`Oblivion.jar`), from your own copy of the game.
2. **Its resources unpacked into `assets/`.** A `.jar` is a zip and the game's
   resources sit at its root, so this is just an unzip with the Java classes
   dropped. There is a script for it:

   ```sh
   pwsh tools/extract-assets.ps1 path/to/Oblivion.jar   # Windows
   sh   tools/extract-assets.sh  path/to/Oblivion.jar   # Linux/macOS (needs unzip)
   ```

   Both write to `assets/` and expect ~112 resource files.

That is everything needed to **play**, and to run the golden decode tests
(`cargo test -p eso-tools --features assets`).

## Build & run

```sh
cargo run -p game --features interactive --bin play --release   # play it
cargo test --workspace                                          # 80 tests, no data needed
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

The suites that need game data are opt-in, so a fresh clone tests green:

```sh
cargo test -p eso-tools --features assets      # + the golden decode tests
cargo test --workspace --features game/fixtures,eso-tools/fixtures   # everything
```

`assets` means you have extracted your own copy into `assets/`. `fixtures` means
you also have `tests/fixtures/`, the captured ground truth — which is derived
from the original game and is not distributed, so those suites are for
development rather than something a clone can reproduce.

Tools:

```sh
cargo run -p eso-tools -- jtm|lang|cml|scr|scr-trace|scr-exec|scr-coverage ./assets
cargo run -p eso-tools -- save-roundtrip path/to/eso_blob.bin
cargo run -p render --bin map-shot -- ./assets l01_1.jtm out.png
cargo run -p render --features interactive --bin map-view -- ./assets l01_1.jtm
```

Controls are in [`docs/controls.md`](docs/controls.md).

## Beyond the port

Two things here are **not** part of the faithful port. They sit on top of the
engine, are gated by nothing, and have no tests — demos, not guarantees:

- **`mapforge`** — a custom-map generator. It writes a `.jtm` map and a `.scr`
  script from scratch and boots them in the engine. Everything else here proves
  the original's formats can be *read* byte-exactly; mapforge is the proof they
  can be *written* too, which is what makes a level editor plausible. It touches
  no shipped file — custom content only.

  ```sh
  cargo run -p game --bin mapforge --release -- world    # a walking-sim level
  cargo run -p game --bin mapforge --release -- palette  # a tile contact sheet
  ```

- **widescreen** (`play wide` / `wide10`) — a viewport wider than the original's
  240x320. Good for looking around; deliberately outside the parity gates,
  because the original's framing is part of what gets validated.

## Audio: none — the original is silent

Verified against the real bytecode: no class in the jar references
`javax.microedition.media` or any vendor audio API, the jar ships zero audio
assets, and the menu build never surfaces the vestigial "Sound:" toggle (its
fire branch is dead code, ported faithfully and pinned by a test). A 1:1 port of
a silent game is silent — audio is out of scope by *fidelity*, not omission.

## Legal

Unofficial and non-commercial. Not affiliated with, endorsed by, or approved by
ZeniMax Media, Bethesda Softworks, or Vir2L Studios. Dual-licensed
**MIT OR Apache-2.0**. Trademarks and the no-game-data guarantee: **`NOTICE.md`**.

## How this was built

OpenOBM was written with [Claude Code](https://claude.com/claude-code) — Fable 5,
Opus 5, and Opus 4.8 — over 11 weeks, June to September 2026.

Every correctness claim above was established mechanically, by diffing against
the original binary — never by hand judgement. That is the whole method: the
model does not get to decide whether the port is right, the original does.
