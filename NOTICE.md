# Notices

## This repository contains no game data

OpenOBM is an **engine reimplementation only**. No asset, data file, string
table, sprite, map, script, audio, bytecode, or disassembly from the original
game is present in this repository or anywhere in its git history.

To run or test OpenOBM you must supply your own legally obtained copy of the
original game. See "Supplying the game data" in `README.md`.

This separation is enforced, not incidental:

- The engine contains no `include_bytes!`/`include_str!` of game data — every
  asset is read from a directory you point it at, at runtime.
- `.gitignore` blocks the game-derived paths (`assets/`, `tests/fixtures/`,
  `artifacts/`) so they cannot be committed by accident.
- The committed `insta` snapshots record **hashes and structural counts only**
  (dimensions, record counts, opcode tallies) — enough to prove byte-exactness,
  never enough to reconstruct content.

## Trademarks

*The Elder Scrolls*, *Oblivion*, *Elder Scrolls Travels*, *Bethesda Softworks*,
*Vir2L Studios*, and *ZeniMax* are trademarks or registered trademarks of
ZeniMax Media Inc. and its subsidiaries.

OpenOBM is an unofficial, non-commercial, fan-made project. It is **not**
affiliated with, endorsed by, sponsored by, or approved by ZeniMax Media,
Bethesda Softworks, or Vir2L Studios. The original game is
© 2006 Vir2L Studios LLC / Bethesda Softworks LLC, ZeniMax Media companies.

References to the original game's title in this documentation are nominative —
they identify the work this engine reimplements, which cannot be described
without naming it.

## Licensing

The OpenOBM engine, tools, and documentation are dual-licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
