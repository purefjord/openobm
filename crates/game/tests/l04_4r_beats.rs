//! l04_4r (the 3-FLOOR side maze off "Daedroth in Kvatch") PER-BEAT
//! validation (loop #34): the seeded floor-1 hold, the entry-cell
//! leave-early chain (dialogue 61767 + the action=12 re-arm), two seeded
//! op47 descents (each behind its own dialogue hold + teleport — floor 3
//! has cfg[18]=0, boss-only), the k=9 boss kill firing entry 9's TRIPLE
//! op37 reward + op35 (a real no-op this loop ported) + op29 into
//! /l04_4b.scr (the Kvatch REMATCH level), and leg B: the leave-early
//! route (entry 5 -> dismiss -> action=12) converging on the same
//! rematch. Fixtures oracle-stable x2.
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
fn l04_4r_beats_match_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_l04rbeats.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "b0_world.txt",
        "b0_layers.txt",
        "b0_over.txt",
        "b1_world.txt",
        "b1_over.txt",
        "b2_world.txt",
        "b2_layers.txt",
        "b2_over.txt",
        "b3_world.txt",
        "b3_layers.txt",
        "b3_over.txt",
        "b4_world.txt",
        "b4_layers.txt",
        "b4_over.txt",
        "b5_world.txt",
        "b5_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l04rb_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
    // Both legs end in l04_4b (the Kvatch rematch): mode-0 gameplay, the
    // player at its op15 spawn.
    assert_eq!(s.mode(), 0, "l04_4b is a straight mode-0 open");
    let p = s.world.actors[0].as_ref().expect("player");
    assert_eq!(
        (p.var_int_arr_b[0], p.var_int_arr_b[1]),
        (1508, 2289),
        "the rematch spawn"
    );
    // The leg-A boss reward (entry 9's triple op37) rides the carried
    // player through leg B: weapon 9, armor 31, consumable 7 (inventory
    // tags kind << 8 | id).
    for tag in [9, (1 << 8) | 31, (2 << 8) | 7] {
        assert!(
            p.var_int_arr_k.contains(&tag),
            "reward tag {tag} missing from the carried inventory"
        );
    }
}
