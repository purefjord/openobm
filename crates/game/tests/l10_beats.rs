//! Loop D (M13, loop #40) — l10_10_cr + l10_10 PER-BEAT validation: the
//! artifact-fetch level (== the l07_7 pattern). l10_10_cr loads direct
//! (l09_9's opening cutscene halts, so its exit isn't a practical entry;
//! l10_10 carries no rewards, so cumulative carry is moot) to its op45
//! checkpoint — NOT re-dumped here (already byte-gated as loop-39's e7,
//! Martin at 1307 under framepace; the port just asserts mode 3). The
//! _cr cutscene chains automatically into l10_10, which opens straight
//! to mode 0 (no checkpoint) with 9 STATIC kind-6 scamps (no walks, all
//! outside aggro) and the artifact region. The take (entry 9) clears the
//! region + barrier, rewrites cell (4,2), re-anchors respawn (512,640),
//! arms the exit region, and halts at the 61935 hold (the l10 end-state
//! window). The exit (entry 11) chains into l11_11_cr, whose opening
//! cutscene settles at the 61710 hold with Martin arrived at 1305
//! (cadence-free). Every gated dump is cadence-free -> NO framepace, no
//! kills, no seeds. Fixtures oracle-stable x2. ONE script both sides,
//! split at `# [assert-*]` markers.
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

#[test]
fn l10_beats_match_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_l10beats.txt")).expect("drive");
    let segs = segments(&script);
    assert_eq!(segs.len(), 6, "5 assert markers -> 6 segments");
    let mut s = boot_shell();
    let mut arts: HashMap<String, game::script::Artifact> = HashMap::new();

    // seg 0: boot -> load l10_10_cr direct -> its op45 checkpoint.
    arts.extend(game::script::drive(&mut s, &segs[0]).expect("seg0"));
    assert_eq!(
        s.mode(),
        3,
        "f0: the l10_10_cr checkpoint (gated as loop-39 e7)"
    );

    // seg 1: the _cr cutscene chains into the l10_10 open.
    arts.extend(game::script::drive(&mut s, &segs[1]).expect("seg1"));
    assert_eq!(s.mode(), 0, "f1: l10_10 opens straight into gameplay");
    assert_eq!(pos(&s, 0), (4618, 3000), "player at its op41-walked spawn");
    for slot in 1..=9 {
        assert!(
            s.world.actors[slot].is_some(),
            "the kind-6 scamp slot {slot}"
        );
    }
    assert_eq!(s.world.respawn, [4619, 3000], "the open op71 anchor");

    // seg 2: entry 8 (hint) + entry 9 (take) -> the 61935 hold.
    arts.extend(game::script::drive(&mut s, &segs[2]).expect("seg2"));
    assert!(s.world.dialogue.is_some(), "the 61935 take hold is open");

    // seg 3: dismissed -> the f2 end-state dumps.
    arts.extend(game::script::drive(&mut s, &segs[3]).expect("seg3"));
    assert_eq!(s.world.respawn, [512, 640], "e9's op71 re-anchor");

    // seg 4: entry 10 (hint) + entry 11 (exit) -> l11_11_cr's open hold.
    arts.extend(game::script::drive(&mut s, &segs[4]).expect("seg4"));
    assert_eq!(s.mode(), 0, "f3: l11_11_cr opens into its cutscene");
    assert_eq!(pos(&s, 0), (1026, 1064), "the l11 opening walk completed");
    assert_eq!(
        pos(&s, 2),
        (1305, 936),
        "Martin arrived (cadence-free hold)"
    );

    // The byte gates (f0 skipped — loop-39 e7 covers the checkpoint).
    for name in [
        "f1_world.txt",
        "f1_layers.txt",
        "f1_over.txt",
        "f2_world.txt",
        "f2_layers.txt",
        "f2_over.txt",
        "f3_world.txt",
        "f3_layers.txt",
        "f3_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l10b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
}
