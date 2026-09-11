//! Loop E (M13, loop #41) — l11_11_cr + l11_11 PER-BEAT validation: the
//! PLAYABLE Cloud Ruler scene l11_11_cr (a "Talk" region whose action
//! fires an exit cutscene with op68 impact flashes -> op29 /l11_11.scr)
//! and the BOSS level l11_11 (Mankar Camoran, the oh_liches slot-1
//! boss). The exit is a SCRIPTED KILL: region 7 arms the boss's k=8
//! death trigger, and killing it (the player is the faithful humanoid
//! attacker in a player-vs-boss fight) fires op29 -> /l12_12.scr. op68
//! is already ported (the 68|69 effects-spawn arm), so no engine change.
//! Every l11 open is op12-into-a-free-cutscene (no op45) -> g0/g1/g2 are
//! cadence-free; l12_12's open IS op12-then-op45, so the boss-kill's
//! op29 load runs PACED and g3 pins Martin. Fixtures oracle-stable x2.
//! ONE script both sides, split at `# [assert-*]` markers.
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
fn l11_beats_match_the_real_game() {
    let script =
        std::fs::read_to_string(root().join("tests/drives/to_l11beats.txt")).expect("drive");
    let segs = segments(&script);
    assert_eq!(segs.len(), 5, "4 assert markers -> 5 segments");
    let mut s = boot_shell();
    let mut arts: HashMap<String, game::script::Artifact> = HashMap::new();

    // seg 0: boot -> load l11_11_cr direct -> its opening 61710 hold.
    arts.extend(game::script::drive(&mut s, &segs[0]).expect("seg0"));
    assert_eq!(s.mode(), 0, "g0: l11_11_cr opens into its cutscene");
    assert!(s.world.dialogue.is_some(), "g0: the 61710 hold is open");
    assert_eq!(pos(&s, 0), (1026, 1064), "the opening walk completed");
    assert_eq!(pos(&s, 2), (1305, 936), "Martin arrived (cadence-free)");

    // seg 1: dismiss -> the Talk region exit cutscene -> the l11_11 open.
    arts.extend(game::script::drive(&mut s, &segs[1]).expect("seg1"));
    assert_eq!(s.mode(), 0, "g1: l11_11 opens straight into gameplay");
    assert_eq!(pos(&s, 0), (3800, 533), "the boss-level player spawn");
    {
        let boss = s.world.actors[1].as_ref().expect("the boss slot 1");
        assert_eq!(
            boss.display_name.as_deref(),
            Some("Mankar Camoran"),
            "the 61849 boss name resolved"
        );
        assert_eq!(boss.model_name, "/oh_liches.cml", "the lich boss model");
        assert_eq!(boss.var_byte_k, -1, "the boss k not yet armed at open");
    }
    assert!(s.world.actors[2].is_some(), "dremora slot 2");
    assert!(s.world.actors[3].is_some(), "dremora slot 3");

    // seg 2: entry 7 -> the boss-encounter 61714 hold.
    arts.extend(game::script::drive(&mut s, &segs[2]).expect("seg2"));
    assert!(s.world.dialogue.is_some(), "g2: the 61714 hold is open");
    assert_eq!(
        pos(&s, 0),
        (554, 557),
        "the player walked into the encounter"
    );
    assert!(
        s.world.actors[1].is_some(),
        "the boss is present at the hold"
    );

    // seg 3: dismiss -> arm k=8 -> the seeded kill -> op29 /l12_12.scr.
    arts.extend(game::script::drive(&mut s, &segs[3]).expect("seg3"));
    assert_eq!(s.mode(), 3, "g3: the l12_12 Great Gate checkpoint");
    {
        let p = s.world.actors[0].as_ref().expect("player");
        assert_eq!(p.var_byte_o, 2, "the boss kill leveled the player to 2");
        assert_eq!(
            (p.var_short_q, p.var_short_o),
            (122, 122),
            "the level-up health recompute"
        );
        assert_eq!(
            (p.var_short_r, p.var_short_p),
            (30, 30),
            "the swing-drained fatigue capped on the paced free-run"
        );
    }

    // The byte gates.
    for name in [
        "g0_world.txt",
        "g0_layers.txt",
        "g0_over.txt",
        "g1_world.txt",
        "g1_layers.txt",
        "g1_over.txt",
        "g2_world.txt",
        "g2_layers.txt",
        "g2_over.txt",
        "g3_world.txt",
        "g3_layers.txt",
        "g3_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                assert_eq!(t, &fixture(&format!("l11b_{name}")), "{name} differs")
            }
            _ => panic!("{name} text"),
        }
    }
}
