//! Loop B (M13, loop #38) — l08_8_cr + l08_8 PER-BEAT validation: the
//! real l07_7 exit into the Cloud Ruler checkpoint (op45-BEFORE-op12 —
//! walkers freeze AT SPAWN, no walk window), the nine-hold cutscene
//! behind the player's op21 arrive-gate walk, the l08_8 open (10 idle
//! wander-mode ghosts, the wounded talk NPC), the talk beats, the
//! shared barrier hint/leave, THE BARRIER CHAIN (entries 12 -> 11 ->
//! 10 -> 13: respawn re-anchors, op70/op69 barrier swaps, op18/op22
//! cell writes, each arming the next region), the artifact hint/take
//! (the l08 end-state dumped AT the 61935 hold — the only window where
//! e15's map effects are visible), and the exit into l09_9_cr — whose
//! open is op12-THEN-op45, so Martin freezes mid-walk at the paced
//! 1307 (the b14/c0 class). Fixtures oracle-stable x2. ONE script both
//! sides (oracle/to_l08beats.txt), the port split at `# [assert-*]`
//! markers (the loop-37 segments pattern).
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
fn hud_text(s: &Shell) -> Option<&str> {
    s.world.hud.as_ref().map(|h| h.text.as_str())
}

#[test]
fn l08_beats_match_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_l08beats.txt")).expect("drive");
    let segs = segments(&script);
    assert_eq!(segs.len(), 13, "12 assert markers -> 13 segments");
    let mut s = boot_shell();
    let mut arts: HashMap<String, game::script::Artifact> = HashMap::new();

    // seg 0: boot -> l07_7 -> its real exit -> the d0 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[0]).expect("seg0"));
    assert_eq!(s.mode(), 3, "d0: the l08_8_cr op45 checkpoint hold");
    // op45 runs BEFORE op12 here: no mode-0 frame — everyone at spawn.
    assert_eq!(pos(&s, 0), (1026, 1974), "player at the south door");
    assert_eq!(pos(&s, 1), (1030, 547), "Jauffre at spawn");
    assert_eq!(walk(&s, 1), (1030, 548), "Jauffre's op42 armed, unrun");
    assert_eq!(pos(&s, 2), (1327, 936), "Martin at spawn");
    assert_eq!(walk(&s, 2), (1305, 936), "Martin's op41 armed, unrun");

    // seg 1: the cutscene + the l08_8 open + entry 17.
    arts.extend(game::script::drive(&mut s, &segs[1]).expect("seg1"));
    assert_eq!(s.mode(), 0, "d1: l08_8 opens straight into gameplay");
    assert_eq!(hud_text(&s), Some("Talk"), "e17's op39 61808 hint");

    // seg 2: entry 18 — the 61694 hold with the talk NPC.
    arts.extend(game::script::drive(&mut s, &segs[2]).expect("seg2"));
    {
        let d = s.world.dialogue.as_ref().expect("the 61694 hold is open");
        assert!(!d.lines.is_empty(), "the 61694 dialogue resolved");
        let npc = s.world.actors[11].as_ref().expect("the talk NPC");
        assert_eq!(
            s.world.speaker.as_deref(),
            npc.display_name.as_deref(),
            "op53 slot 11: the camera-follow speaker is the talk NPC"
        );
        assert!(npc.display_name.is_some(), "the 61852 name resolved");
    }

    // seg 3: the hold dismissed, d2 dumped, entry 8 fired.
    arts.extend(game::script::drive(&mut s, &segs[3]).expect("seg3"));
    {
        let h = s.world.hud.as_ref().expect("e8 set the barrier hint");
        assert_eq!(h.text, "Activate Lever", "the op39 61804 enter-hint");
        assert_eq!(h.timeout_ms, 60_000, "op39 secs=60");
        assert_eq!(h.color, 0xFF0000, "op39 color code 2 = red");
    }

    // seg 4: entry 9 — the shared leave event clears the hint.
    arts.extend(game::script::drive(&mut s, &segs[4]).expect("seg4"));
    assert!(s.world.hud.is_none(), "e9's op40 cleared the hint");

    // segs 5-8: the barrier chain, each re-anchoring the respawn.
    arts.extend(game::script::drive(&mut s, &segs[5]).expect("seg5"));
    assert_eq!(s.world.respawn, [3072, 1024], "e12's op71 re-anchor");
    arts.extend(game::script::drive(&mut s, &segs[6]).expect("seg6"));
    assert_eq!(s.world.respawn, [1152, 1408], "e11's op71 re-anchor");
    arts.extend(game::script::drive(&mut s, &segs[7]).expect("seg7"));
    assert_eq!(s.world.respawn, [1024, 3328], "e10's op71 re-anchor");
    arts.extend(game::script::drive(&mut s, &segs[8]).expect("seg8"));
    assert_eq!(s.world.respawn, [4352, 1280], "e13's op71 re-anchor");

    // seg 9: entry 14 — the artifact hint.
    arts.extend(game::script::drive(&mut s, &segs[9]).expect("seg9"));
    assert_eq!(hud_text(&s), Some("Take Artifact"), "e14's op39 61805");

    // seg 10: entry 15 to the 61935 hold — the l08 end-state window.
    arts.extend(game::script::drive(&mut s, &segs[10]).expect("seg10"));
    {
        let d = s.world.dialogue.as_ref().expect("the 61935 hold is open");
        assert!(!d.lines.is_empty(), "the 61935 dialogue resolved");
        assert!(
            s.world.hud.is_none(),
            "e15's op40 cleared the transient 61806 hint before the hold"
        );
    }

    // seg 11: dismiss -> Call 16 -> op29 /l09_9_cr.scr (paced load).
    arts.extend(game::script::drive(&mut s, &segs[11]).expect("seg11"));
    assert_eq!(s.mode(), 3, "d9: the l09_9_cr checkpoint");
    // op12-THEN-op45 (the l07_7_cr shape): ONE mode-0 frame ran — the
    // 1-unit Jauffre walk arrived, Martin's 22-unit walk froze at the
    // 50ms-cadence x=1307 (the b14/c0 class, byte-exact under pacing).
    assert_eq!(pos(&s, 0), (1026, 1974), "player at the south door");
    assert_eq!(pos(&s, 1), (1030, 548), "Jauffre arrived (1-unit walk)");
    assert_eq!(pos(&s, 2), (1307, 936), "Martin frozen at the paced x");
    assert_eq!(walk(&s, 2), (1305, 936), "Martin's target still armed");

    // The byte gates: every dump byte-identical to the oracle capture.
    for name in [
        "d0_world.txt",
        "d0_layers.txt",
        "d0_over.txt",
        "d1_world.txt",
        "d1_layers.txt",
        "d1_over.txt",
        "d2_world.txt",
        "d3_world.txt",
        "d4_world.txt",
        "d4_layers.txt",
        "d4_over.txt",
        "d5_world.txt",
        "d5_layers.txt",
        "d5_over.txt",
        "d6_world.txt",
        "d6_layers.txt",
        "d6_over.txt",
        "d7_world.txt",
        "d7_layers.txt",
        "d7_over.txt",
        "d8_world.txt",
        "d9a_world.txt",
        "d9a_layers.txt",
        "d9a_over.txt",
        "d9_world.txt",
        "d9_layers.txt",
        "d9_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l08b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
}
