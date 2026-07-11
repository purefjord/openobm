//! l02_2 (Kvatch) PER-BEAT gameplay validation (loop #32) — the full quest
//! played beat by beat against the real bytecode: the guard dialogue, the
//! three death-trigger-chained waves killed with REAL seeded melee swings
//! (`callhit` = one `h.a(att,tgt,true)` — XP, the death-sound RNG draw, the
//! `e.int_a()` loot roll and the `var_byte_k` trigger push all run inside),
//! the Martin spawn (k=36), the rescue cutscene (7 dialogue holds; the
//! burning-chapel hazard is neutralized with sethp/teleport — its live
//! damage is frame-paced and ungateable), the Daedric Dagger reward, the
//! Martin-death FAIL RELOAD (entry 36 re-runs entry 1 — the reload gate:
//! pickups zeroed, waves gone, the player's earned XP/level KEPT), and the
//! exit chain into l03_3's checkpoint. 14 artifacts byte-identical
//! (oracle-stable x2). The player levels 1 -> 3 across the ten kills — the
//! award_xp -> h.c -> h.f re-derivation validated in real gameplay.
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
fn l02_2_beats_match_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l02beats.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "b0_world.txt",
        "b1_world.txt",
        "b2_world.txt",
        "b3_world.txt",
        "b4_world.txt",
        "b4_over.txt",
        "b5_world.txt",
        "b6_world.txt",
        "b7_world.txt",
        "b7_over.txt",
        "b8_world.txt",
        "b8_over.txt",
        "b9_world.txt",
        "b9_layers.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l02b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
    // The drive ends at l03_3's op45 checkpoint (the exit chain fired from
    // the reloaded l02_2): mode 3, Martin + Jauffre spawned by the new level.
    assert_eq!(s.mode(), 3, "the exit chain parks at the l03_3 checkpoint");
    assert_eq!(
        s.world.actors[1]
            .as_ref()
            .and_then(|a| a.display_name.as_deref()),
        Some("Martin"),
        "l03_3 spawned Martin"
    );
    // The ten seeded kills leveled the player 1 -> 3 (o=3 pre-reload, and the
    // fail reload KEEPS the earned XP — visible in the b8/b9 fixtures too).
    assert_eq!(
        s.world.actors[0].as_ref().map(|a| a.var_byte_o),
        Some(3),
        "the player kept the earned level through the fail reload + exit"
    );
}
