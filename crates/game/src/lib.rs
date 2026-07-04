//! The `b.java` shell: the mode state machine, the main loop, and the menu
//! pages, validated by **screenshot parity** against the real bytecode running
//! on FreeJ2ME (see `docs/loop-recon.md` for the recon this is built on).
//!
//! Unlike the per-method field-trace sweeps of M8–M10, the loop is wall-clock
//! and interactive, so its ground truth is LCD frames: one input script drives
//! both the real jar (oracle/OracleRun) and this shell, and the checkpoint
//! frames must match byte-for-byte on static, input-settled screens.

pub mod asset;
pub mod dump;
pub mod fb;
pub mod fmenu;
pub mod gpaint;
pub mod paint;
pub mod script;
pub mod shell;
pub mod text;
pub mod vm;
pub mod world;
pub mod wrap;
