//! The MAZE EXIT chain (loop #23): leaving the L01 sewers chains through
//! `op29` to the next room's script, validated byte-for-byte against the real
//! bytecode. The sewers have two exits — leave early (`entry 9` ->
//! `/l01_1b.scr`) and sewers-cleared (`entry 10` -> `/l01_1c.scr`) — both
//! fired through the entry cell's action-event edge, which is the SAME real
//! `e.a(int)` push a death trigger uses (`callentry`). ONE script
//! (`oracle/to_sewers.txt`) drives both sides: generate the maze (seeded),
//! push the chain entry, settle at the next room's first-dialogue hold.
//!
//! Both rooms reload `/l01_1.jtm` and re-run a "you return" opening that halts
//! at its first dialogue; the actor array + map converge, but the two scripts
//! arm DIFFERENT event overlays (l01_1b's `entry 6` vs l01_1c's `entry 7`),
//! which the `dumpover` fixtures distinguish. The world dump is the
//! GENERATOR variant (`dumpworldg`): the carried/presentation state (player
//! inventory, the op76 hud flag, the dialogue speaker/open-state/text) is
//! masked because it reflects the non-deterministic unattended L01 fight and
//! the lang-overlay resolution of the opening text — neither of which the
//! exit chain determines. Every fixture came out of the real `Oblivion.jar`
//! running this exact script through OracleRun, verified stable across runs.

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

/// ONE script, BOTH sides: every dump `oracle/to_sewers.txt` names must equal
/// the committed oracle fixtures byte-for-byte — both exit chains reach their
/// next room's settled first-dialogue hold (world + layers + the distinct
/// event overlays).
#[test]
fn maze_exit_chain_matches_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_sewers.txt")).expect("drive script");
    let mut s = boot_shell();
    let artifacts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "exit_b_world.txt",
        "exit_b_layers.txt",
        "exit_b_over.txt",
        "exit_c_world.txt",
        "exit_c_layers.txt",
        "exit_c_over.txt",
    ] {
        match &artifacts[name] {
            game::script::Artifact::Text(t) => assert_eq!(t, &fixture(name), "{name} differs"),
            game::script::Artifact::Frame(_) => panic!("{name} is a text artifact"),
        }
    }
}
