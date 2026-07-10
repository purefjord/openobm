//! ALL remaining game levels (loop #29) — quests 4-14 + the ending, each
//! validated at LOAD against the real bytecode. One combined drive
//! (`oracle/to_alllevels.txt`) callscripts every level from the L01 hold and
//! dumps its map layers + event overlays + (masked) world; this gate replays
//! that same script through the shell and diffs all 30 artifacts byte-for-byte.
//!
//! Every opcode across all 32 game scripts is ported (global scr-coverage
//! check; op77 = the end_15 credits trigger was wired this loop), so every
//! level is fully PLAYABLE — these gates pin that each one LOADS and spawns
//! its actors identically to the real game. Deep per-beat gameplay inside a
//! level is non-deterministic free-roam, beyond a load gate.
//!
//! Level -> map (by CONTENT; the l0N numbers do NOT align): l04_4->l04_1,
//! l05_5->l05_1, l06_6->l06_1, l07_7->l07_1, l08_8->l08_1, l09_9->l09_1,
//! l10_10->l10_1, l11_11->l11_1, l12_12->l12_1, end_15->l13_clrl.

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
fn all_levels_load_matches_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_alllevels.txt")).expect("drive");
    let mut s = boot_shell();
    let arts = game::script::drive(&mut s, &script).expect("drive");
    let levels = [
        "l04", "l05", "l06", "l07", "l08", "l09", "l10", "l11", "l12", "end15",
    ];
    for lv in levels {
        // Every level's MAP layers + event overlays match byte-for-byte (the
        // deterministic load). The masked WORLD dump also matches — including
        // combat levels that drop into unlocked gameplay with NPCs mid-walk
        // (e.g. l07: 13/21 walking): a walking actor's position is a pure
        // function of accumulated GAME time, so it is frame-cadence-
        // independent while the world ticks freely.
        //
        // l05 (the Mankar Camoran boss scene) needs a 2-field mask (loop #31):
        // its opening runs a throne-procession walk (op34s toward x=2040) and
        // then FREEZES the world at an op45 checkpoint (mode 3) with actors 14
        // + 16 still mid-walk. The walk window between op12 (mode 0) and op45
        // is FRAME-counted (the VM steps one op per frame), and the original
        // run() loop has NO sleep — frame duration is repaint+gc cost, i.e.
        // machine-paced. The walkers' frozen positions are therefore
        // cadence-dependent state by the game's own construction (two real
        // handsets would disagree the same way): the oracle's headless frames
        // gave them ~100 game-ms of walking (pos 2069/2062), the port's fixed
        // 50ms frames give 50 (2084/2081). Same class as timers/anim cursors —
        // masked; everything else in the dump stays byte-gated.
        for kind in ["layers", "over", "world"] {
            let name = format!("{lv}_{kind}.txt");
            let got = match &arts[&name] {
                game::script::Artifact::Text(t) => t,
                _ => panic!("{name} text"),
            };
            let want = fixture(&name);
            if lv == "l05" && kind == "world" {
                assert_eq!(
                    mask_walk_frozen_pos(got),
                    mask_walk_frozen_pos(&want),
                    "{name} differs beyond the cadence-masked walker positions"
                );
                assert_walk_frozen_structure(got);
            } else {
                assert_eq!(got, &want, "{name} differs");
            }
        }
    }
}

/// Mask the two cadence-frozen walkers' `pos=` field (see the l05 comment in
/// the test body); every other field on those lines stays byte-compared.
fn mask_walk_frozen_pos(dump: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in dump.lines() {
        if line.starts_with("actor 14 ") || line.starts_with("actor 16 ") {
            let start = line.find(" pos=").expect("walker line has pos=");
            let end = start
                + 1
                + line[start + 1..]
                    .find(' ')
                    .expect("pos= is not the last field");
            out.push(format!("{} pos=<cadence>{}", &line[..start], &line[end..]));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n") + "\n"
}

/// Pin the masked fields' structure on the PORT dump: both walkers hold their
/// op34 walk target (the freeze keeps it armed forever — mode 3 halts the
/// actor loop), sit on the walk row, and have progressed from the spawn
/// column x=2099 toward the throne column x=2040 without passing it. The
/// exact x (2084/2081) is the fixed-50ms-cadence value: it changes only if
/// the script runner's frame dt changes.
fn assert_walk_frozen_structure(dump: &str) {
    for (slot, y, walk, x50) in [
        (14, 2060, "walk=2040,2061", 2084),
        (16, 2415, "walk=2040,2416", 2081),
    ] {
        let line = dump
            .lines()
            .find(|l| l.starts_with(&format!("actor {slot} ")))
            .expect("walker line");
        assert!(line.contains(walk), "actor {slot} walk target");
        let pos = line
            .split(" pos=")
            .nth(1)
            .and_then(|s| s.split(' ').next())
            .expect("pos field");
        let (px, py) = pos.split_once(',').expect("pos pair");
        let (px, py): (i32, i32) = (px.parse().unwrap(), py.parse().unwrap());
        assert_eq!(py, y, "actor {slot} walk row");
        assert!((2040..=2099).contains(&px), "actor {slot} bounded progress");
        assert_eq!(px, x50, "actor {slot} at the 50ms-cadence rest position");
    }
}
