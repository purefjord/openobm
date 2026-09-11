//! Loop C (M13, loop #39) — l09_9_cr + l09_9 PER-BEAT validation: the
//! real l08_8 exit chain, the short two-hold Cloud Ruler cutscene, the
//! l09_9 open with its OWN op21-gated opening cutscene, the hint/leave/
//! dialog one-offs, barrier dispel 1 (+ the slot-16 spawn), the
//! King's-ghost ESCORT in two legs (slot 17 "Spirit", kind 16 =
//! aggro-immune to the ghosts; op36-parked at (10,10) between legs;
//! Call 8's mid-sequence wave spawn), the gate-opening FINALE (op18
//! =73 wall strips + op22 collision opens, op32 arms the LICH boss
//! k=10, Call 9 sets E=450 last, op20 removes the escort), the ONE
//! seeded kill of the milestone (the lich dies pre-cast — the
//! risk-register summoner never gets a ticking window; the kill XP
//! levels the player to 2), the 61935 exit hold (boss corpse decayed
//! by dump time), and the paced op29 into l10_10_cr (op12-then-op45:
//! Martin frozen at 1307). Fixtures oracle-stable x2. ONE script both
//! sides, split at `# [assert-*]` markers.
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
fn pos(s: &Shell, slot: usize) -> (i32, i32) {
    let a = s.world.actors[slot].as_ref().expect("actor");
    (a.var_int_arr_b[0], a.var_int_arr_b[1])
}
fn walk(s: &Shell, slot: usize) -> (i32, i32) {
    let a = s.world.actors[slot].as_ref().expect("actor");
    (a.var_int_arr_j[0], a.var_int_arr_j[1])
}
#[test]
fn l09_beats_match_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_l09beats.txt")).expect("drive");
    let segs = segments(&script);
    assert_eq!(segs.len(), 16, "15 assert markers -> 16 segments");
    let mut s = boot_shell();
    let mut arts: HashMap<String, game::script::Artifact> = HashMap::new();

    // seg 0: boot -> l08_8 -> its exit sub -> the e0 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[0]).expect("seg0"));
    assert_eq!(s.mode(), 3, "e0: the l09_9_cr checkpoint");
    assert_eq!(pos(&s, 2), (1307, 936), "Martin frozen at the paced x");
    assert_eq!(walk(&s, 2), (1305, 936), "Martin's target still armed");

    // seg 1: the _cr cutscene + the l09_9 load + its opening cutscene.
    arts.extend(game::script::drive(&mut s, &segs[1]).expect("seg1"));
    assert_eq!(s.mode(), 0, "e1: l09_9 in gameplay");
    assert_eq!(pos(&s, 0), (4569, 1876), "the opening walk completed");
    assert_eq!(s.world.respawn, [4600, 1876], "the opening op71 anchor");
    assert!(s.world.input_unlocked, "the opening cutscene unlocked");

    // The op39 enter-hints (entries 12/14/17/21) are pure `op39; Return`
    // beats. hud text is MASKED in dumpworldg (not byte-validatable), and
    // because this drive FORCES the region entries via callentry while the
    // player stands at the opening position, the actor loop's overlay
    // resample (shell.rs:1626 — a leave edge with code -2 calls
    // set_hud_text(None)) clears the transient hint the same frame it isn't
    // standing on the tile. So these hints are gated only by their firing
    // without error; the dialogue holds they bracket are the real gates.

    // seg 2: entry 12 — a pure enter-hint (fires, transient).
    arts.extend(game::script::drive(&mut s, &segs[2]).expect("seg2"));

    // seg 3: entry 18 — the shared op40 leave.
    arts.extend(game::script::drive(&mut s, &segs[3]).expect("seg3"));
    assert!(s.world.hud.is_none(), "e18's op40 leaves no hint");

    // seg 4: entry 16 — the 61736 hold.
    arts.extend(game::script::drive(&mut s, &segs[4]).expect("seg4"));
    assert!(s.world.dialogue.is_some(), "e16: the 61736 hold is open");

    // seg 5: dismissed; entry 21 — a pure enter-hint (transient).
    arts.extend(game::script::drive(&mut s, &segs[5]).expect("seg5"));

    // seg 6: entry 22 — op40 then the 61742 hold.
    arts.extend(game::script::drive(&mut s, &segs[6]).expect("seg6"));
    assert!(s.world.dialogue.is_some(), "e22: the 61742 hold is open");

    // seg 7: dismissed; entry 13 — barrier 1 dispelled, slot 16 spawned.
    arts.extend(game::script::drive(&mut s, &segs[7]).expect("seg7"));
    assert_eq!(s.world.respawn, [5632, 4352], "e13's op71 re-anchor");
    assert!(s.world.actors[16].is_some(), "e13's op15 slot-16 spawn");

    // seg 8: entry 14 — a pure enter-hint (transient).
    arts.extend(game::script::drive(&mut s, &segs[8]).expect("seg8"));

    // seg 9: entry 15 — ESCORT leg 1 (4 holds).
    arts.extend(game::script::drive(&mut s, &segs[9]).expect("seg9"));
    assert_eq!(s.world.respawn, [2321, 4724], "e15's op71 re-anchor");
    {
        let spirit = s.world.actors[17].as_ref().expect("the escort spawned");
        assert_eq!(
            spirit.display_name.as_deref(),
            Some("Spirit"),
            "the 61853 name resolved"
        );
        assert_eq!(spirit.var_byte_u, 1, "op75 guard flag");
        assert_eq!(pos(&s, 17), (10, 10), "op36-parked between legs");
    }
    for slot in 8..=13 {
        assert!(s.world.actors[slot].is_some(), "Call 8 wave slot {slot}");
    }

    // seg 10: entry 17 — a pure enter-hint (transient).
    arts.extend(game::script::drive(&mut s, &segs[10]).expect("seg10"));

    // seg 11: entry 19 — ESCORT leg 2 (3 holds).
    arts.extend(game::script::drive(&mut s, &segs[11]).expect("seg11"));
    assert_eq!(pos(&s, 17), (10, 10), "the Spirit parked again");

    // seg 12: entry 20 — THE FINALE (3 holds + the gate + the boss arm).
    arts.extend(game::script::drive(&mut s, &segs[12]).expect("seg12"));
    {
        let boss = s.world.actors[1].as_ref().expect("the lich boss");
        assert_eq!(boss.var_byte_k, 10, "op32 armed the k=10 death trigger");
        assert_eq!(
            boss.e_field, 450,
            "Call 9's E=450 lands after Call 23's 500"
        );
    }
    assert!(s.world.actors[17].is_none(), "op20 removed the escort");
    assert!(s.world.input_unlocked, "the finale unlocked");

    // seg 13: the seeded kill -> e10's 61935 exit hold (free-run).
    arts.extend(game::script::drive(&mut s, &segs[13]).expect("seg13"));
    assert!(s.world.dialogue.is_some(), "the 61935 exit hold is open");
    assert!(
        s.world.actors[1].is_none(),
        "the boss corpse decayed and was removed by dump time"
    );
    {
        let p = s.world.actors[0].as_ref().expect("player");
        assert_eq!(p.var_byte_o, 2, "the kill XP leveled the player to 2");
        assert_eq!(
            (p.var_short_q, p.var_short_o),
            (122, 122),
            "the level-up health recompute"
        );
    }

    // seg 14: dismiss -> op29 /l10_10_cr.scr (paced) -> the e7 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[14]).expect("seg14"));
    assert_eq!(s.mode(), 3, "e7: the l10_10_cr checkpoint");
    assert_eq!(pos(&s, 2), (1307, 936), "Martin frozen at the paced x");
    assert_eq!(walk(&s, 2), (1305, 936), "Martin's target still armed");

    // The byte gates.
    for name in [
        "e0_world.txt",
        "e0_over.txt",
        "e1_world.txt",
        "e1_layers.txt",
        "e1_over.txt",
        "e2_world.txt",
        "e3_world.txt",
        "e3_layers.txt",
        "e3_over.txt",
        "e4_world.txt",
        "e4_layers.txt",
        "e4_over.txt",
        "e5_world.txt",
        "e5_layers.txt",
        "e5_over.txt",
        "e6_world.txt",
        "e6_layers.txt",
        "e6_over.txt",
        "e6b_world.txt",
        "e7_world.txt",
        "e7_layers.txt",
        "e7_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l09b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
}
