# Notices

## Original game materials

OpenOBM distributes an engine reimplementation. It does not distribute the
original game archive, an extracted resource pack, disassembly files, or the
captured reference-fixture directories used during development.

The two images under `screenshots/` are demonstration captures that depict
original game artwork and, in the gameplay image, dialogue. Source code and
tests also refer to original resource names and include a copyright-notice
comparison. This repository therefore does not claim to contain no original
game content of any kind.

The engine's MIT/Apache licensing does not grant rights to the original game's
artwork, writing, trademarks, or other materials shown or referenced here.

Users must supply their own legally obtained game data. Fonts for public play
are bundled separately under the license described below.

`.gitignore` excludes `assets/`, `tests/fixtures/`, generated `artifacts/`, and
runtime saves. These exclusions reduce accidental additions; review staged
files before publishing. The committed diagnostic snapshots contain structural
counts and hashes rather than a runnable resource pack.

## Bundled fonts

`crates/game/fonts/LiberationSans-Regular.ttf` and `LiberationSans-Bold.ttf` are
unmodified files from Liberation Fonts 2.1.5, distributed under the SIL Open Font
License 1.1. Copyright notices and the full license are in
[`crates/game/fonts/LICENSE`](crates/game/fonts/LICENSE); retain them when
redistributing the fonts or builds containing them. See the adjacent README for
upstream provenance and checksums. These fonts are not part of the original game
and are not covered by the engine's MIT/Apache license.

## Trademarks

*The Elder Scrolls*, *Oblivion*, *Elder Scrolls Travels*, *Bethesda Softworks*,
*Vir2L Studios*, and *ZeniMax* are trademarks or registered trademarks of
ZeniMax Media Inc. and its subsidiaries.

OpenOBM is an unofficial, non-commercial, fan-made project. It is not affiliated
with, endorsed by, sponsored by, or approved by ZeniMax Media, Bethesda Softworks,
or Vir2L Studios. References to the game's title identify the work this engine
reimplements.

## Engine licensing

The OpenOBM engine, tools, and project-authored documentation are dual-licensed
under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option. Original game materials remain outside that grant.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
