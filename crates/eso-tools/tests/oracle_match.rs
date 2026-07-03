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

use eso_tools::{
    dump_ai_sweep, dump_anim_trace, dump_cast_sweep, dump_cml, dump_collision_sweep,
    dump_combat_sweep, dump_corpse_sweep, dump_dist_sweep, dump_dot_sweep, dump_effects_sweep,
    dump_hf_sweep, dump_jtm, dump_lang, dump_move_sweep, dump_scr, dump_scr_trace,
    dump_targeting_sweep, dump_tick_sweep, dump_xp_sweep,
};
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

/// Melee combat damage. The Rust port (`formats::melee_attack`) runs the same
/// attacker/target + seed sweep that the FreeJ2ME oracle drove through the real
/// `h.a` bytecode with a deterministically seeded `java.util.Random`. Each line's
/// trailing `probe` (one extra `nextInt()`) makes a wrong RNG-draw count fail the
/// diff, so this validates the damage/defense/crit math *and* RNG consumption.
#[test]
fn combat_matches_oracle() {
    let rust = dump_combat_sweep().expect("rust combat sweep");
    assert_identical(&rust, &oracle("combat_sweep.txt"), "combat");
}

/// `h.a(int[],int[])` octagonal distance — the Rust `combat_distance` over the
/// same position grid the oracle drove through the real method.
#[test]
fn dist_matches_oracle() {
    let rust = dump_dist_sweep().expect("rust dist sweep");
    assert_identical(&rust, &oracle("dist_sweep.txt"), "dist");
}

/// Targeting (`h.j_a`). The Rust `nearest_target` over a synthetic actor array +
/// querier sweep that the oracle installs into the live `b.var_j_arr_a` and drives
/// through the real method — validating the skip conditions (dead / same-faction /
/// same-kind), the nearest-by-distance pick, and tie-breaking.
#[test]
fn targeting_matches_oracle() {
    let rust = dump_targeting_sweep().expect("rust targeting sweep");
    assert_identical(&rust, &oracle("targeting_sweep.txt"), "targeting");
}

/// Map collision (`h.boolean_a`). The Rust `collides` over a synthetic collision
/// layer + crafted sample cells the oracle swaps into `b.var_byte_arr_a`/dims and
/// drives through the real method — validating bounds, solid (`1`), and the four
/// directional slope tiles (`2..=5`) against sub-tile position.
#[test]
fn collision_matches_oracle() {
    let rust = dump_collision_sweep().expect("rust collision sweep");
    assert_identical(&rust, &oracle("collision_sweep.txt"), "collision");
}

/// Movement step (`h.void_a`, "moveInWorld") — position-trace parity. The Rust
/// `move_in_world` over the same scripted (direction, dt) sequence on the same
/// synthetic collision map the oracle drives through the real method: validates the
/// step-timer/speed, the world delta + derived iso/tile coords, facing, and the
/// collision revert (walking into a wall).
#[test]
fn move_matches_oracle() {
    let rust = dump_move_sweep().expect("rust move sweep");
    assert_identical(&rust, &oracle("move_sweep.txt"), "move");
}

/// XP / level-up (`h.c` + `h.g`). The Rust port (`Actor::award_xp`) runs the same
/// class×level×race×config sweep the oracle drove through the real `h.c`; the
/// dumped XP tables (Rust constants vs the real `h.var_short_arr_a/b`) and every
/// level-up result (attributes, class bonus `h.g`, health recompute, progression
/// `h.f`) must match byte-for-byte. Reuses `hf_tables.txt` for the level-up `h.f`.
#[test]
fn xp_matches_oracle() {
    let rust = dump_xp_sweep(&fixture_path("hf_tables.txt")).expect("rust xp sweep");
    assert_identical(&rust, &oracle("xp_sweep.txt"), "xp");
}

/// Animation playback primitives (`g.java`'s frame-cursor overloads). The Rust
/// `Anim` (`advance`/`seek`/`reset`/`lookup`) runs a fixed op-script over
/// synthetic `d`-graphs (every loop/clamp/seek branch) plus the real
/// `oh_pc`/`oh_magic` models; `AnimOracle.java` runs the identical script through
/// the **real `g.class` bytecode** (the four return-type-distinct `a` overloads,
/// resolved by descriptor) on equivalent graphs. Byte-identical = the port
/// reproduces the real advance/seek/reset behavior — including loop wrap,
/// non-loop clamp+done, seek past-end detection, and the flattened
/// (key/loop/frame_count) extraction from real `.cml`.
#[test]
fn anim_matches_oracle() {
    let rust = dump_anim_trace(&assets()).expect("rust anim trace");
    assert_identical(&rust, &oracle("anim_trace.txt"), "anim");
}

/// Effect pool update (`i.a(long)`). The Rust [`Effects::update`] runs crafted
/// pool images × frame sequences over the real `/oh_magic.cml` model + a synthetic
/// 25-actor array; `Instrument.dumpEffects` installs the same images into the live
/// `i.var_short_arr_a` and drives the **real `i.a(long)` bytecode**, dumping the 99
/// shorts per frame. Byte-identical = the port reproduces the per-frame timers,
/// `g.seek` frame-stepping, projectile movement/conversion, actor-homing, and
/// lifetime cycling — including the held-frame `0xFF00` mask and the post-clear
/// read quirk. Scenarios avoid melee (same-faction actors), so the (separately
/// validated) `collision_hit` path stays deterministic.
#[test]
fn effects_matches_oracle() {
    let rust = dump_effects_sweep(&assets()).expect("rust effects sweep");
    assert_identical(&rust, &oracle("effects_trace.txt"), "effects");
}

/// Per-actor tick (`h.a(j,long,boolean)`), ported subset. The Rust [`Actor::tick`]
/// runs synthetic actors × frame sequences (no model → the anim advance no-ops);
/// `Instrument.dumpTick` drives the same actors through the **real `h.a` bytecode**
/// (the regen recompute hitting the live `h.f`/`b.var_e_a`) and dumps the touched
/// fields per frame. Byte-identical = the port reproduces the timers, animation
/// advance gate, attack-windup, player health/fatigue regen, `var_byte_y`
/// countdown, and dead-corpse timer exactly. Reuses `hf_tables.txt` for the regen
/// `h.f`. (Movement, DoT, the NPC AI, and corpse removal have their own sweeps —
/// they are out of these scenarios' scope.)
#[test]
fn tick_matches_oracle() {
    let rust = dump_tick_sweep(&fixture_path("hf_tables.txt")).expect("rust tick sweep");
    assert_identical(&rust, &oracle("tick_trace.txt"), "tick");
}

/// `var_short_k` damage-over-time lap (the DoT branch of `h.a(j,long,boolean)`).
/// The Rust [`Actor::tick`] runs a non-aggressive NPC victim + dealer through the
/// DoT path; `Instrument.dumpDoT` drives the same actors through the real `h.a`,
/// dumping victim HP/timers + the effect pool per frame and an end-of-scenario RNG
/// probe. Byte-identical = the port reproduces the timer decrements, the
/// defense-bypassing damage, the `i.a(8,j2)` spawn, and the exact RNG draw count
/// (incl. the `dealer.var_byte_t` extra-draw fork).
#[test]
fn dot_matches_oracle() {
    let rust = dump_dot_sweep().expect("rust dot sweep");
    assert_identical(&rust, &oracle("dot_sweep.txt"), "dot");
}

/// Corpse removal (the dead branch of `h.a` → `b.a(var_byte_c-1)`). The Rust
/// [`Actor::tick`] removes a dead NPC from the array at the 250ms threshold;
/// `Instrument.dumpCorpse` installs the same dead NPC in `b.var_j_arr_a[1]` and
/// drives the real `h.a`, dumping the slot's presence + corpse timer per frame.
/// Byte-identical = the port reproduces the threshold and the slot nulling.
#[test]
fn corpse_matches_oracle() {
    let rust = dump_corpse_sweep().expect("rust corpse sweep");
    assert_identical(&rust, &oracle("corpse_sweep.txt"), "corpse");
}

/// NPC attack AI (`h.boolean_b` + the melee at `h.a:457`, run from the tick).
/// The Rust [`Actor::tick`] drives an aggressive NPC over a 25-slot actor array
/// through target acquisition, the E/F range decision (approach / lock+face /
/// drop, incl. the `var_byte_y == 2` hold), the cooldown gate, and the strike
/// (player targets gated by `bl`); `Instrument.dumpAI` installs the same array
/// into `b.var_j_arr_a` and drives the real `h.a`. Byte-identical = the port
/// reproduces the AI-owned fields, the target's HP/E/text, and the melee's
/// exact RNG draw count per scenario.
#[test]
fn ai_matches_oracle() {
    let rust = dump_ai_sweep().expect("rust ai sweep");
    assert_identical(&rust, &oracle("ai_trace.txt"), "ai");
}

/// The spell/cast path (`h.c` + the armed/creature branch of `h.a:1204`, run
/// from the tick). The Rust [`Actor::tick`] drives an armed/creature NPC through
/// the creature swing, the three buff types across the level tiers, AoE poison,
/// cure, AoE damage, self-heal, the bolt projectile, the `var_byte_y == 3`
/// weapon-drop, and the `y == 2` vanish/teleport-wander on a synthetic map;
/// `Instrument.dumpCast` drives the same scenarios through the real `h.a`.
/// Byte-identical = the port reproduces the caster/target fields, the effect
/// pool, and the exact RNG draw counts per scenario.
#[test]
fn cast_matches_oracle() {
    let rust = dump_cast_sweep(&fixture_path("hf_tables.txt")).expect("rust cast sweep");
    assert_identical(&rust, &oracle("cast_trace.txt"), "cast");
}
