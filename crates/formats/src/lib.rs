//! Faithful, graphics-free parsers for the binary data formats of
//! *The Elder Scrolls Travels: Oblivion* (Java ME / MIDP-1.0).
//!
//! This crate is the "ground-truth preservation" layer described in `GOAL.md`:
//! it ports the original byte-level behavior of the decompiled Java so it can be
//! fuzzed and snapshot-tested in isolation, before any renderer or game loop is
//! involved. Correctness is anchored to the original binary via the oracle
//! fixtures under `tests/fixtures/`, not to hand judgement.
//!
//! Modules:
//! - [`reader`]   — the single big-endian/unsigned-byte reader all parsers use.
//! - [`iso`]      — world<->screen isometric transforms.
//! - [`lang`]     — `lang_*.txt` string tables.
//! - [`jtm`]      — `.jtm` RLE tile maps.
//! - [`asset`]    — resource loading by original `/leading-slash` paths.

pub mod actor;
pub mod asset;
pub mod cml;
pub mod combat;
pub mod iso;
pub mod jtm;
pub mod lang;
pub mod reader;
pub mod rng;
pub mod save;
pub mod scr;
pub mod vm;

pub use actor::{Actor, Tables};
pub use asset::AssetStore;
pub use cml::{parse_cml, Cml, CmlRecord};
pub use combat::{melee_attack, CombatOutcome};
pub use iso::{screen_to_world, world_to_screen, Vec2i};
pub use jtm::{parse_jtm, JtmMap};
pub use lang::{parse_lang, parse_lang_file, Lang};
pub use reader::{ParseError, Reader};
pub use rng::JavaRandom;
pub use save::{parse_save, serialize_save, Save};
pub use scr::{parse_scr, ScrProgram};
pub use vm::{Effect, ScriptVm, Step, StepKind, TextRef};
