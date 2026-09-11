//! L02 playable level (l03_3.scr -> /l03_1.jtm) LOAD + opening (loop #28) — the
//! real "Illegitimate Son" gameplay level the l03_3_1 cutscene chains into via
//! op29. Reached by callscript; entry 1 loads /l03_1.jtm, spawns the player,
//! arms the region/pickup overlays, then drops into mode-0 gameplay (input
//! unlocked). Every opcode is already ported. Gated: the map's layers, the
//! event overlays, and the world (masked) byte-for-byte vs the real bytecode.
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

#[test]
fn l03_3_level_load_matches_the_real_game() {
    let script = std::fs::read_to_string(root().join("tests/drives/to_l03_3.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in ["l03_3_layers.txt", "l03_3_over.txt", "l03_3_world.txt"] {
        match &arts[name] {
            game::script::Artifact::Text(t) => assert_eq!(t, &fixture(name), "{name} differs"),
            _ => panic!("{name} text"),
        }
    }
    // l03_3 opens with Martin + Jauffre and pauses at a checkpoint save menu.
    assert_eq!(
        s.mode(),
        3,
        "l03_3 pauses at its checkpoint save menu (op45)"
    );
    assert!(s.world.actors[0].is_some(), "the player spawned");
}
