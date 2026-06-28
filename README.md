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
| `h.f` progression | 5957 synthetic actors (8 classes × 20 levels × 37 races, ± inventory) vs **real `h.f` bytecode** | byte-identical |
| melee combat | 730 attacker/target+seed cases (damage/crit/dodge/block/armor/weapon-tiers + RNG draws) vs **real `h.a` bytecode** | byte-identical |
| XP / level-up | 560 class×level×race cases + the 52-entry XP tables vs **real `h.c`/`h.g` bytecode** | byte-identical |
| combat distance | 229 position-pair cases vs **real `h.a(int[],int[])` bytecode** | byte-identical |
| targeting | 48 querier cases over a synthetic actor array vs **real `h.j_a` bytecode** | byte-identical |
| map collision | 19 cases (bounds, solid, 4 slope tiles, OR) vs **real `h.boolean_a` bytecode** | byte-identical |
| movement step | 22-step position trace (timer/speed, delta, iso/tile, facing, wall-revert) vs **real `h.void_a` bytecode** | byte-identical |
| animation playback | 2336-line op-trace (advance/seek/reset over synthetic + real `oh_pc`/`oh_magic` graphs) vs **real `g.class` bytecode** | byte-identical |
| effect pool | 7-scenario × per-frame pool trace (timers, frame-step, projectile move/convert, homing, lifetime) vs **real `i.a(long)` bytecode** | byte-identical |
| per-actor tick (subset) | 8-scenario × per-frame field trace (timers, anim gate, attack-windup, regen, move-to-target, P/G buff-expiry, status/corpse timers) vs **real `h.a(j,long,boolean)` bytecode** | byte-identical |

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
- **M7** — `.scr` VM execution: the VM now resolves inline strings and emits
  semantic effects (load level/model, free graphics, show text, wait,
  call/return) — `startup.scr` executes as: load `/startup.cml`, four 2000ms
  waits, free the splash graphics, queue `/startup2.scr`, return. Validated
  against the **real runtime**: a FreeJ2ME resource-load hook (`MIDletLoader` +
  `-Doracle.reslog`) showed the live boot loads exactly `/startup.cml →
  /1,2,3,5.png`, which **caught and corrected** a semantic error the
  transcription-only oracle could not — op 72 is *free cached graphics*, not
  *load*. (Full opcode side effects into actor/world state remain future work;
  per-entry execution is deterministic, so decode order is run order.)
- **M8 (in progress)** — actors/stats: `actor.rs` ports `j.java`'s actor fields
  (defaults from the source initializers) and the deterministic health/fatigue
  derivation `h.java` recomputes everywhere (`max_health = level*4 + (str+O)*2 +
  endurance*2 + I`, `rate = 40000/max`, likewise fatigue). Unit-tested from the
  exact source formulas. Combat resolution, inventory item-application, and
  `h.f`'s class/level bonus tables (coupled to the `.scr` stat tables) are the
  remaining M8 slices. **Inventory stat application** (`h.b(j,int[])`) is now
  ported too: equipping gear sets the J/K/L/M/N/O/P bonus fields and recomputes
  health/fatigue; consumables restore/queue health & fatigue (clamped) — direct
  effects unit-tested. **`h.f` (class/level/race progression)** is now ported
  (`Actor::class_progression`): the race-row lookup, the equipped-item `var_short_z`
  sum, and the per-class skill table (`prog_a/b/c/d`) by level breakpoint, all
  transcribed verbatim from the decompiled switch (redundant double-writes kept).
  It is validated against the **real `h.f` bytecode**, not a transcription: a new
  oracle (`Instrument.dumpHf`) constructs synthetic `j` actors under FreeJ2ME and
  invokes the genuine private `h.f`, sweeping the whole class/level/race space; the
  Rust port reproduces all 5957 outputs byte-for-byte (`hf_matches_oracle`).
  **Melee combat damage** is now ported too (`combat.rs::melee_attack` =
  `h.a(j,j,bool)` + `h.a(int,j,j,bool,bool)`): the damage formula, weapon-tier
  override + non-player halving, crit, armor/dodge/block resolution, and the exact
  `java.util.Random` draw sequence (faithful `JavaRandom` LCG). Validated against
  the **real `h.a` bytecode** with a deterministically seeded RNG (`dumpCombat`,
  dumped while paused so the game loop can't race the shared RNG; each case carries
  one extra `nextInt()` "probe" so a wrong draw count fails the diff) — 730 cases
  match byte-for-byte (`combat_matches_oracle`). **XP / level-up** is ported too
  (`Actor::award_xp` = `h.c(j,int)` + `h.g`): award `var_short_arr_b[n]` XP, and on
  crossing `var_short_arr_a[level+1]` level up — +1 to all seven attributes, the
  class level bonus (`h.g`), a health/fatigue recompute, and the progression pass
  (`h.f`). The 52-entry XP tables are hardcoded and dumped alongside the sweep, so
  the diff validates them against the real `h.var_short_arr_a/b` statics. 560 cases
  + the tables match the **real `h.c` bytecode** (`xp_matches_oracle`). The combat
  **distance** (`combat_distance` = `h.a(int[],int[])`, the octagonal range metric
  behind targeting/AI/AoE) is ported and validated against the real method
  (`dist_matches_oracle`), and the **non-player E-update** (`target.E = max(distance
  to aggressor, E)` on a hit) now completes melee resolution for NPC targets too
  (combat sweep Phase C). Remaining M8 (deferred — they reach the unported `i.java`
  projectile/effects + map/actor-array state, not pure math): the **spell/cast**
  path, the combat **death** branch (animation/sound), and the `h.f` secondary pass.
  **Targeting** (`nearest_target` = `h.j_a`, nearest valid enemy: skips empty/dead/
  same-faction/same-kind, closest by distance, earliest index on ties) is ported and
  validated by installing a synthetic actor array into the live `b.var_j_arr_a` and
  diffing the chosen slot against the real method (`targeting_matches_oracle`).
  **Map collision** (`world::collides` = `h.boolean_a`) — the first movement
  primitive (M10): an actor's three sampled cells against the collision layer, with
  bounds, solid (`1`), and the four directional **slope** tiles (`2..=5`, resolved
  against the corner's sub-tile position). Validated by swapping a synthetic
  collision layer + dims into `b` and diffing 19 crafted cases against the real
  method (`collision_matches_oracle`). The **movement step** (`world::move_in_world`
  = `h.void_a`, "moveInWorld") is ported on top: the step timer + time-scaled speed,
  the world delta with derived iso (`var_int_arr_i`) and tile coords + facing, and
  the collision **revert** (`h.void_c` → `h.a`, rebuilding the box corners via the
  inverse iso transform `b.b`). Validated by **position-trace parity** — driving the
  real `h.void_a` through a scripted (direction, dt) sequence on a synthetic map
  (including walking into a wall) and diffing the per-step state
  (`move_matches_oracle`).
- **M9** — `ESO` save format: faithful port of `b.g()`/`b.b()` + the actor blob
  `h.a(j,…)` — `[3 flag bytes][bool_o][player?]` then `[name]` + a 31-byte actor
  header (byte/short/int fields, big-endian, with `byte`s written as
  sign-extended 2-byte pairs) + model name + item records. `parse_save` /
  `serialize_save` **round-trip byte-for-byte** (a game-written blob's own
  save→load is exactly this), proptest-fuzzed. A FreeJ2ME `RecordStore` hook
  (`-Doracle.savelog`) + `eso-dump save-roundtrip <blob>` are wired to validate a
  captured live blob; capturing one requires reaching the in-game save menu
  (the hook is ready). The item `active` bit (recomputed from combat state on
  write) is preserved as raw bytes — recomputing it is M8.
- **M5** — `.scr` data tables materialized: the section sub-parsers now capture
  full row values (actor/item/spell/etc. stats) with each subtype's exact
  signedness, inline-string-pool indexing, and the two global lists (subtype 7's
  flat list, subtype 9's slot list). All 32 scripts' tables **match the oracle
  byte-for-byte**. (Executing opcode side effects remains future work.)
- **M6** — sprite rendering: decode the indexed/`tRNS` PNGs to RGBA and draw
  `.cml` animation frames (source rect + offset + horizontal flip, per
  `g.java::a(Graphics, d, ...)`). The player's 24-group walk/attack/cast cycle
  renders as clean character poses, and the player composites over an iso map
  (`artifacts/pc_sheet.png`, `artifacts/pc_map.png`). Pixel-parity against the
  real game's in-game frames is gated on the oracle's input-injection enabler.

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
