//! Loop A (M13, loop #37) — the l07_7_cr tail + l07_7 PER-BEAT
//! validation: the real l06_b exit chain into the Cloud Ruler checkpoint
//! (Martin's one-frame walk window under framepace — the b14 class), the
//! TEN-hold cutscene (61675..61684) with the walk-out + the AUTOMATIC
//! op29 chain into /l07_7.scr (no exit region), the mode-0 open (21
//! actors, 3 pickups, the artifact region), the artifact-region beats
//! (entry 6 enter-hint; entry 7: respawn re-anchor, region clear, the
//! op18 cell write, the op70 barrier clear, the exit region, the 61980
//! hold), the exit-region hint/leave (entries 9/11 — hud text is not a
//! dump field and its band paints clipped, the loop-13 class: gated as
//! state asserts at the split points), and the exit into l08_8_cr —
//! whose open runs op45 BEFORE op12, so it freezes at the checkpoint
//! with Martin/Jauffre still AT SPAWN, walks armed (pinned below).
//! Fixtures oracle-stable x2. The drive is ONE script both sides
//! (oracle/to_l07beats.txt); the port runs it split at the
//! `# [assert-*]` markers so mid-drive state (hud text, respawn,
//! dialogue) is asserted at the exact script points.
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

/// Split the drive at `# [assert-*]` marker lines: the port executes the
/// same command stream as the oracle, in the same order, but pauses
/// between segments for state asserts. Markers are comments, so the
/// oracle side ignores them and the streams stay identical.
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

#[test]
fn l07_beats_match_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_l07beats.txt")).expect("drive");
    let segs = segments(&script);
    assert_eq!(segs.len(), 8, "7 assert markers -> 8 segments");
    let mut s = boot_shell();
    let mut arts: HashMap<String, game::script::Artifact> = HashMap::new();

    // seg 0: boot -> maze -> the real l06_b exit -> the c0 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[0]).expect("seg0"));
    assert_eq!(s.mode(), 3, "c0: the l07_7_cr op45 checkpoint hold");
    {
        let m = s.world.actors[1].as_ref().expect("Martin slot 1");
        assert_eq!(
            (m.var_int_arr_b[0], m.var_int_arr_b[1]),
            (1307, 936),
            "c0: Martin frozen mid-walk at the 50ms-cadence x (the b14 class)"
        );
        assert_eq!(
            (m.var_int_arr_j[0], m.var_int_arr_j[1]),
            (1305, 936),
            "c0: Martin's op41 walk target still armed through the freeze"
        );
    }

    // seg 1: the ten-hold cutscene -> the auto chain -> c1 -> entry 6 -> c2.
    arts.extend(game::script::drive(&mut s, &segs[1]).expect("seg1"));
    assert_eq!(s.mode(), 0, "c1/c2: l07_7 opens straight into gameplay");
    {
        let hud = s.world.hud.as_ref().expect("entry 6 set the hud hint");
        assert_eq!(hud.text, "Take Artifact", "the op39 61805 enter-hint");
        assert_eq!(hud.timeout_ms, 60_000, "op39 secs=60");
        assert_eq!(hud.color, 0xFF0000, "op39 color code 2 = red");
    }

    // seg 2: entry 7 fired; the 61980 hold is open (dismissed next seg).
    arts.extend(game::script::drive(&mut s, &segs[2]).expect("seg2"));
    {
        let d = s.world.dialogue.as_ref().expect("the 61980 hold is open");
        assert!(!d.lines.is_empty(), "the 61980 dialogue resolved");
        assert_eq!(
            s.world.speaker.as_deref(),
            Some("Champion"),
            "op53 slot 0: the camera-follow speaker is the player"
        );
    }

    // seg 3: the hold dismissed -> the c3 dumps.
    arts.extend(game::script::drive(&mut s, &segs[3]).expect("seg3"));
    assert_eq!(
        s.world.respawn,
        [2432, 640],
        "c3: entry 7's op71 re-anchor (masked in dumpworldg)"
    );

    // seg 4: entry 9 -> the c4 dump.
    arts.extend(game::script::drive(&mut s, &segs[4]).expect("seg4"));
    assert_eq!(
        s.world.hud.as_ref().map(|h| h.text.as_str()),
        Some("To Bruma"),
        "the op39 61807 exit-region enter-hint"
    );

    // seg 5: entry 11 (the region's leave event) clears the hint.
    arts.extend(game::script::drive(&mut s, &segs[5]).expect("seg5"));
    assert!(
        s.world.hud.is_none(),
        "entry 11's op40 cleared the hud hint"
    );

    // seg 6: entry 10 -> op29 /l08_8_cr.scr -> the c5 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[6]).expect("seg6"));
    assert_eq!(s.mode(), 3, "c5: the l08_8_cr checkpoint (op45 pre-op12)");
    {
        // op45 fires BEFORE op12 here (unlike l07_7_cr), so NO mode-0
        // frame runs before the freeze: the walkers hold AT SPAWN with
        // their targets armed — cadence-independent, pinned exactly.
        let p = s.world.actors[0].as_ref().expect("player");
        assert_eq!((p.var_int_arr_b[0], p.var_int_arr_b[1]), (1026, 1974));
        let j = s.world.actors[1].as_ref().expect("Jauffre slot 1");
        assert_eq!((j.var_int_arr_b[0], j.var_int_arr_b[1]), (1030, 547));
        assert_eq!((j.var_int_arr_j[0], j.var_int_arr_j[1]), (1030, 548));
        let m = s.world.actors[2].as_ref().expect("Martin slot 2");
        assert_eq!((m.var_int_arr_b[0], m.var_int_arr_b[1]), (1327, 936));
        assert_eq!((m.var_int_arr_j[0], m.var_int_arr_j[1]), (1305, 936));
    }

    // The byte gates: every dump byte-identical to the oracle capture.
    for name in [
        "c0_world.txt",
        "c0_layers.txt",
        "c0_over.txt",
        "c1_world.txt",
        "c1_layers.txt",
        "c1_over.txt",
        "c2_world.txt",
        "c3_world.txt",
        "c3_layers.txt",
        "c3_over.txt",
        "c4_world.txt",
        "c5_world.txt",
        "c5_layers.txt",
        "c5_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l07b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
}
