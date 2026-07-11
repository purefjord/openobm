//! l04_4 ("Daedroth in Kvatch", quest 4) PER-BEAT gameplay validation
//! (loop #33) — the full quest played beat by beat against the real
//! bytecode: the op45 checkpoint hold, the k=6 plaza-guard kill firing
//! entry 6's region arm, the 61770 choice dialogue + its action=10
//! re-arm, the Daedroth reveal cutscene (slots 8+9, two op53 holds, the
//! respawn re-anchor + the PLAYER's own k=15 death trigger — a retry
//! mechanism no earlier level used), dying to the Daedroth (entry 15
//! re-arms enter=14 while the mode-11 death screen shows; YES resumes at
//! the new anchor), the Daedroth kill swapping the locked exit open
//! (action 21 -> 22), the bridge (op52 walks the player while op22
//! rewrites collision cells — the first mid-level LAYER mutation gate),
//! the op29 exit into l05_5's checkpoint (layers+overlays only — its
//! world is frame-cadence state, loop #31), and leg B: the entry-10
//! op29 into /l04_4r.scr with a pre-seeded RNG — the op47 side maze
//! (floor 1) load-gated, closing the LAST unvalidated level.
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
fn l04_4_beats_match_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l04beats.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "b0_world.txt",
        "b0_over.txt",
        "b1_world.txt",
        "b1_over.txt",
        "b2_world.txt",
        "b2_over.txt",
        "b3_world.txt",
        "b4_world.txt",
        "b4_over.txt",
        "b5_world.txt",
        "b5_over.txt",
        "b6_world.txt",
        "b6_over.txt",
        "b7_world.txt",
        "b7_layers.txt",
        "b8_layers.txt",
        "b8_over.txt",
        "b9_world.txt",
        "b9_layers.txt",
        "b9_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l04b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
    // Leg B ends at l04_4r's opening dialogue hold (entry 4's first op53,
    // 61772) over the seeded floor-1 maze: mode 0, dialogue open.
    assert_eq!(s.mode(), 0, "leg B parks at the l04_4r opening dialogue");
    assert!(s.world.dialogue.is_some(), "the entry-4 op53 hold is open");
}
