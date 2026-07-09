//! The op47 SEWER MAZE (loop #22): the procedural generator validated
//! byte-for-byte against the real bytecode. ONE script (`oracle/to_maze.txt`)
//! drives both sides: boot to the L01 first-dialogue hold, re-base the RNG,
//! `callscript /l01_1r.scr` (the real room-2 chain target), let the script
//! generate maze #1 + respawn the player at the entry, FIRE the entry-cell
//! action event (its dialogue makes `dumpworld` comparable), then two PAUSED
//! `setseed` + `callmaze` regens dump layers/overlays/world per seed.
//!
//! Every fixture here came out of the REAL `Oblivion.jar` running the same
//! script through OracleRun (artifacts/maze) — the generated map (collision +
//! base + edge-decor + marker layers), the event overlays (exit enter-event,
//! entry action/leave sentinels), the seeded enemy spawns (the op58-grown
//! subtype-0 row 7 scamps), and the tag-20 pickup drops.

use game::shell::Shell;
use game::text::TextMasks;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn boot_shell() -> Shell {
    let masks = TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt")).unwrap();
    Shell::boot(root().join("assets"), masks).unwrap()
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(root().join("tests/fixtures/oracle").join(name))
        .unwrap()
        .replace("\r\n", "\n")
}

/// ONE script, BOTH sides: every dump `oracle/to_maze.txt` names must equal
/// the committed oracle fixtures byte-for-byte — two independent seeds pin
/// the generator (map carve, decor, markers, overlays, spawns, pickups) as a
/// pure function of the RNG stream.
#[test]
fn maze_generation_matches_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_maze.txt")).expect("drive script");
    let mut s = boot_shell();
    let artifacts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "maze_layers_a.txt",
        "maze_over_a.txt",
        "maze_world_a.txt",
        "maze_layers_b.txt",
        "maze_over_b.txt",
        "maze_world_b.txt",
    ] {
        match &artifacts[name] {
            game::script::Artifact::Text(t) => assert_eq!(t, &fixture(name), "{name} differs"),
            game::script::Artifact::Frame(_) => panic!("{name} is a text artifact"),
        }
    }
}
