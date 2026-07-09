//! L02 "The Illegitimate Son" CUTSCENE (loop #27): past the checkpoint into
//! the Jauffre dialogue, validated against the real bytecode. The opening
//! pauses at the op45 checkpoint save-menu (loop #25/#26); `callmode 0`
//! (Continue Playing = the real `b.a((byte)0)`) resumes the VM, and entry 4
//! walks the actors then HALTS at Jauffre's first line. Because entry 1's op56
//! loaded /lang.cml, the dialogue text RESOLVES on the headless oracle here
//! (unlike the sewers exit path) — so this gate validates the dialogue text
//! directly, in addition to the map / overlays / actors.
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
fn l02_cutscene_matches_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l02cut.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    // The map, event overlays, and actor array (masked world dump — the carried
    // player inventory / the racy respawn stay masked) byte-for-byte.
    for name in ["l02cut_layers.txt", "l02cut_over.txt", "l02cut_world.txt"] {
        match &arts[name] {
            game::script::Artifact::Text(t) => assert_eq!(t, &fixture(name), "{name} differs"),
            _ => panic!("{name} text"),
        }
    }
    // The cutscene resumed past the checkpoint (mode 0) and HALTED at Jauffre's
    // first line — the dialogue text resolves through the op56-loaded /lang.cml
    // exactly as the real game shows it (oracle full dump, wiki-confirmed).
    assert_eq!(
        s.mode(),
        0,
        "the cutscene resumed to gameplay past Continue"
    );
    let dlg = s.world.dialogue.as_ref().expect("Jauffre dialogue is open");
    assert_eq!(
        dlg.lines,
        vec![
            "Jauffre: You there! Why do you wear the".to_string(),
            "robes of the Emperor?!".to_string(),
        ],
        "the Jauffre opening line resolves + word-wraps like the real game"
    );
    assert_eq!(
        s.world.speaker.as_deref(),
        Some("Jauffre"),
        "op53 set the speaker to the follow target"
    );
}
