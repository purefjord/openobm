//! Loop F (M13, loop #42, FINAL) — l12_12 + end_15 PER-BEAT + THE
//! SPINE-CARRY drive: the milestone capstone. ONE drive chains
//! l11_11_cr -> l11_11 (boss kill, player -> level 2) -> l12_12 (the
//! Great Gate) -> end_15 -> op77 (THE GAME END), proving the final
//! handoff with a player that lived through the spine. op61 (mode 9,
//! l12's exit region) and op77 (mode 4, the game end) are the coverage
//! opcodes -- op61 was the last apply() gap, ported this loop; op77 was
//! loop #29. l12's region B (the Great Gate Daedra, an E=30000 Xivilai,
//! no op32) is FREE COMBAT -- non-deterministic like l02_2's fight (the
//! loop-28 precedent), exercised + evidenced in the REPL, not byte-gated
//! (the drive fires the EXIT region A directly). h0 (l12 checkpoint,
//! Martin PACED), h1 (l12 open), h2 (end_15 open) are byte-gated + the
//! cumulative-carry (player o=2 at end_15) asserted; h3 = the op77
//! terminal (mode 4 + the game-in-progress flag cleared -- the credits
//! paint reads no world, so the player's mid-walk-at-op77 pos is moot).
//! Fixtures oracle-stable x2. ONE script both sides, split at
//! `# [assert-*]` markers.
use game::shell::Shell;
use game::text::TextMasks;
use std::collections::HashMap;
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
fn segments(script: &str) -> Vec<String> {
    let mut segs = vec![String::new()];
    for line in script.lines() {
        if line.trim_start().starts_with("# [assert-") {
            segs.push(String::new());
        } else {
            let cur = segs.last_mut().unwrap();
            cur.push_str(line);
            cur.push('\n');
        }
    }
    segs
}
fn player_level(s: &Shell) -> i8 {
    s.world.actors[0].as_ref().expect("player").var_byte_o
}

#[test]
fn l12_beats_and_the_game_end_match_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l12beats.txt")).expect("drive");
    let segs = segments(&script);
    assert_eq!(segs.len(), 5, "4 assert markers -> 5 segments");
    let mut s = boot_shell();
    let mut arts: HashMap<String, game::script::Artifact> = HashMap::new();

    // seg 0: the spine l11_11_cr -> l11_11 boss kill -> the l12 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[0]).expect("seg0"));
    assert_eq!(s.mode(), 3, "h0: the l12_12 Great Gate checkpoint");
    assert_eq!(player_level(&s), 2, "the l11 boss kill carried level 2 in");
    {
        let m = s.world.actors[1].as_ref().expect("Martin slot 1");
        assert_eq!(
            (m.var_int_arr_b[0], m.var_int_arr_b[1]),
            (4297, 3973),
            "Martin at the paced walk position"
        );
    }

    // seg 1: the l12 cutscene -> the mode-0 open.
    arts.extend(game::script::drive(&mut s, &segs[1]).expect("seg1"));
    assert_eq!(s.mode(), 0, "h1: l12_12 in gameplay");
    assert_eq!(
        (
            s.world.actors[0].as_ref().unwrap().var_int_arr_b[0],
            s.world.actors[0].as_ref().unwrap().var_int_arr_b[1]
        ),
        (4297, 4439),
        "the player at its l12 spawn"
    );
    assert_eq!(player_level(&s), 2, "the player still level 2");
    for slot in 2..=10 {
        assert!(s.world.actors[slot].is_some(), "the l12 kind-6 slot {slot}");
    }

    // seg 2: the exit (region A) -> op61 (mode 9) + op29 -> end_15 open.
    arts.extend(game::script::drive(&mut s, &segs[2]).expect("seg2"));
    assert_eq!(s.mode(), 0, "h2: end_15 opens into its final conversation");
    assert!(s.world.dialogue.is_some(), "the 61981 hold is open");
    // THE CUMULATIVE-CARRY ASSERT (the plan's spine-carry requirement):
    // the level-2 player rode l11 -> l12 -> end_15 (op15 reuses var_j_a).
    assert_eq!(
        player_level(&s),
        2,
        "the player carried level 2 to the game end"
    );

    // seg 3: the 6-hold conversation -> op17 walk -> op77 (THE GAME END).
    arts.extend(game::script::drive(&mut s, &segs[3]).expect("seg3"));
    assert_eq!(s.mode(), 4, "op77: the game-end credits (mode 4)");
    assert!(
        !s.left_gameplay(),
        "op77 cleared the game-in-progress flag (var_boolean_f=false)"
    );

    // The byte gates (h3 is behavior-only: op77's mode-4 credits paint
    // reads no world, so its dump is the cadence-dependent mid-walk).
    for name in [
        "h0_world.txt",
        "h0_layers.txt",
        "h0_over.txt",
        "h1_world.txt",
        "h1_layers.txt",
        "h1_over.txt",
        "h2_world.txt",
        "h2_layers.txt",
        "h2_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l12b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
}
