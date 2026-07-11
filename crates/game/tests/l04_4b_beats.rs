//! l04_4b (the Kvatch REMATCH after the side maze) PER-BEAT validation
//! (loop #35): the mode-0 open (no checkpoint, no plaza gate), the
//! rematch reveal (entry 9: the second "Mythic Dawn" pair, boss slot 7
//! k=12, two op53 holds, the tail re-anchoring respawn + arming the
//! PLAYER retry k=11), dying at the boss's side (entry 11 re-arms the
//! region to enter=10 = the tail only, during the mode-11 screen), the
//! seeded boss kill swapping the locked exit open (action 17 -> 18),
//! the bridge-path patrol cleared before the op52 walk (the loop-33
//! frame-cadence lesson), the bridge's op22 collision rewrites, and the
//! exit op29 into l05_5's checkpoint — the side branch's spine rejoin.
//! Fixtures oracle-stable x2.
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
fn l04_4b_beats_match_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l04bbeats.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "b0_world.txt",
        "b0_layers.txt",
        "b0_over.txt",
        "b1_world.txt",
        "b2_world.txt",
        "b2_over.txt",
        "b3_world.txt",
        "b3_over.txt",
        "b4_world.txt",
        "b4_over.txt",
        "b6_world.txt",
        "b6_layers.txt",
        "b7_layers.txt",
        "b7_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l04bb_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
    // The exit chain parks at l05_5's op45 checkpoint (loop #31's level).
    assert_eq!(s.mode(), 3, "the exit chain parks at the l05_5 checkpoint");
}
