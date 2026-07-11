//! The l06 COMPLEX ("Spies" -> the Great Gate approach) PER-BEAT
//! validation (loop #36) — SEVEN level scripts in one drive: the Cloud
//! Ruler checkpoint + guard walk-in/out cutscene (op17/op21/op20), the
//! Bruma "Spies" level (spy discovery, the k=18 kill arming BOTH branch
//! exits, the guard-talk chain), the 2-floor l06_a maze (leave-early
//! op16 re-arm; the floor-2 CAPTIVE rescue), **l06_6b — the Bruma
//! rescued-NPC variant that no previous trace, chain doc, or gate ever
//! saw** (its talk rewards a Silver Shortsword, op37 tag 22), the
//! l06_6_ba Great Gate approach (the k=6 guard dying to a Mythic Dawn
//! — the faithful killer — arming the exit), the l06_b maze (op47's
//! third parameterization [3,4,6]), the op29 handoff into l07_7_cr,
//! and leg B: the direct-to-ba branch + the leave-early l06_6a variant.
//! Fixtures oracle-stable x2.
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

/// The two Cloud Ruler opens (b0 = l06_6_cr, b14 = l07_7_cr) freeze
/// Martin's 22-unit op41 walk after ONE mode-0 frame (op12 -> op45 is a
/// one-op window) — the loop-31 l05 class: the distance covered is
/// frame-cadence-dependent (the port's fixed 50ms covers 20 units ->
/// x=1307; the oracle's ~100 game-ms headless frame clamps at the 1305
/// target; real handsets would disagree the same way). Mask ONLY his
/// `pos=`; every other field stays byte-compared, and the exact port
/// value is pinned structurally below.
fn mask_martin_pos(dump: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in dump.lines() {
        if line.starts_with("actor 1 ") {
            let start = line.find(" pos=").expect("Martin line has pos=");
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

/// Pin the masked field on the PORT dump: Martin holds the armed walk
/// target, frozen at the exact fixed-50ms-cadence x.
fn assert_martin_frozen(dump: &str, x50: i32) {
    let line = dump
        .lines()
        .find(|l| l.starts_with("actor 1 "))
        .expect("Martin line");
    assert!(
        line.contains(&format!(" pos={x50},936 walk=1305,936")),
        "Martin frozen at the 50ms-cadence x: {line}"
    );
}

#[test]
fn l06_chain_beats_match_the_real_game() {
    let script = std::fs::read_to_string(root().join("oracle/to_l06beats.txt")).expect("drive");
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
        "b4_world.txt",
        "b4_over.txt",
        "b5_over.txt",
        "b6_world.txt",
        "b6_layers.txt",
        "b6_over.txt",
        "b7_over.txt",
        "b8_world.txt",
        "b8_layers.txt",
        "b8_over.txt",
        "b9_world.txt",
        "b9_layers.txt",
        "b9_over.txt",
        "b10_world.txt",
        "b11_world.txt",
        "b11_layers.txt",
        "b11_over.txt",
        "b12_world.txt",
        "b12_over.txt",
        "b13_world.txt",
        "b13_layers.txt",
        "b13_over.txt",
        "b14_world.txt",
        "b14_layers.txt",
        "b14_over.txt",
        "b15_world.txt",
        "b16_world.txt",
        "b16_over.txt",
    ] {
        match &arts[name] {
            game::script::Artifact::Text(t) => {
                // The two Cloud Ruler checkpoint opens carry Martin's
                // cadence-frozen walk (see mask_martin_pos).
                if name == "b0_world.txt" || name == "b14_world.txt" {
                    assert_martin_frozen(t, 1307);
                    assert_eq!(
                        mask_martin_pos(t),
                        mask_martin_pos(&fixture(&format!("l06b_{name}"))),
                        "{name} differs"
                    );
                } else {
                    assert_eq!(t, &fixture(&format!("l06b_{name}")), "{name} differs")
                }
            }
            _ => panic!("{name} text"),
        }
    }
    // Leg B ends back at l06_6_ba's opening 61671 guard hold.
    assert_eq!(s.mode(), 0, "l06_6_ba opens in mode 0");
    assert!(s.world.dialogue.is_some(), "the 61671 guard hold is open");
    // The rescue-path reward rode along in leg A: the Silver Shortsword
    // (op37 kind 0 id 22 -> inventory tag 22) on the carried player.
    let p = s.world.actors[0].as_ref().expect("player");
    assert!(
        p.var_int_arr_k.contains(&22),
        "the Silver Shortsword reward is in the carried inventory"
    );
}
