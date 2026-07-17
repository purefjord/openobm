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
        // Every level's MAP layers + event overlays + WORLD dump match
        // byte-for-byte. Combat levels that drop into unlocked gameplay with
        // NPCs mid-walk (e.g. l07: 13/21 walking) match because a walking
        // actor's position is a pure function of accumulated GAME time.
        //
        // l05 (the Mankar Camoran boss scene) is the one frame-cadence case
        // (loop #31): its throne-procession walk is FROZEN by an op45
        // checkpoint a frame-counted window after op12, so the walkers' rest
        // positions depend on frame dt. The drive therefore loads l05 under
        // `framepace 50` (the 2026-07 pacer pins the oracle's frame dt to
        // the port's fixed 50ms tick), which makes the dump byte-exact —
        // walkers at 2084/2081, the values the retired 2-field cadence mask
        // used to pin structurally.
        for kind in ["layers", "over", "world"] {
            let name = format!("{lv}_{kind}.txt");
            let got = match &arts[&name] {
                game::script::Artifact::Text(t) => t,
                _ => panic!("{name} text"),
            };
            let want = fixture(&name);
            assert_eq!(got, &want, "{name} differs");
        }
    }
}
