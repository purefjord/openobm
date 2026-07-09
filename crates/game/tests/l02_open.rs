//! L02 "The Illegitimate Son" (l02_2_1.scr -> l03_1.jtm) opening — the LOAD +
//! CHECKPOINT hold, validated against the real bytecode. Reaching L02 via
//! callscript, the opening loads /l03_1.jtm + spawns Champion/Jauffre/Guard,
//! then op45 (void_f) pauses at a checkpoint menu (mode 3, k=4) BEFORE the
//! Jauffre cutscene runs (the VM halts on mode 3). This gate pins that
//! settled load state; the cutscene proper (post-Continue) is a later loop.
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
fn l02_opening_matches_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l02.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in ["l02_layers.txt", "l02_over.txt", "l02_world.txt"] {
        match &arts[name] {
            game::script::Artifact::Text(t) => assert_eq!(t, &fixture(name), "{name} differs"),
            _ => panic!("{name} text"),
        }
    }
}
