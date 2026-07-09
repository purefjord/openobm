//! L02 playable level (l02_2.scr -> /l02_1.jtm) LOAD + opening (loop #28) — the
//! real "Illegitimate Son" gameplay level the l02_2_1 cutscene chains into via
//! op29. Reached by callscript; entry 1 loads /l02_1.jtm, spawns the player,
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
fn l02_2_level_load_matches_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l02_2.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in ["l02_2_layers.txt", "l02_2_over.txt", "l02_2_world.txt"] {
        match &arts[name] {
            game::script::Artifact::Text(t) => assert_eq!(t, &fixture(name), "{name} differs"),
            _ => panic!("{name} text"),
        }
    }
    // The level opened into gameplay: mode 0, the player in slot 0.
    assert_eq!(s.mode(), 0, "l02_2 drops into mode-0 gameplay");
    assert!(s.world.actors[0].is_some(), "the player spawned");
}
