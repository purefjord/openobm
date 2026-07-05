//! Loop #20 gates: the SHOP (mode 1, `o()` + the f Buy/Sell pages) and the
//! m22 interrupt screen. ONE script (`oracle/to_shop.txt`) drove the real
//! jar at the L01 first-dialogue hold with an injected stock list (L01
//! ships none) through the REAL b.f() checkpoint menu; the same script
//! drives the shell here — 11 shots + 2 world dumps, byte-identical.
//!
//! The oracle run pinned the decode's behavior visually: the Axe is RED for
//! the Monk (q2 == q1: fire on a disabled item is dead), a buy REBUILDS the
//! menu (gold title 100 -> 35, cursor reset to the Buy top), sells pay the
//! quarter price in place (+16 -> 51) with the bought club appended LAST,
//! and the m22 YES restores the saved mode.

use game::fb::Fb;
use game::paint::LCD_H;
use game::shell::{Screen, Shell};
use game::text::TextMasks;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn boot_shell() -> Shell {
    let masks = TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt")).unwrap();
    Shell::boot(root().join("assets"), masks).unwrap()
}

fn assert_parity(fb: &Fb, fixture: &str, label: &str) {
    let real = Fb::load_png(&root().join("tests/fixtures/oracle/frames").join(fixture)).unwrap();
    let (diff, bad) = fb.diff_region(&real, LCD_H);
    if bad != 0 {
        let out = root().join("target/parity");
        let _ = std::fs::create_dir_all(&out);
        let _ = fb.save_png(&out.join(format!("{label}_rust.png")));
        let _ = diff.save_png(&out.join(format!("{label}_diff.png")));
    }
    assert_eq!(bad, 0, "{label}: {bad} pixels differ from {fixture}");
}

/// Virtual-time boot to the settled L01 first-dialogue hold, with the
/// drive's stock list injected (mirrors the oracle prelude 1:1 — the same
/// commands, minus the real-time padding).
const TO_HOLD: &str = "wait 12000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\n\
                       wait 45000\nsetflat 0 1 0 7 1 1 2 1\n";

#[test]
fn shop_drive_matches_the_oracle() {
    let script = std::fs::read_to_string(root().join("oracle/to_shop.txt")).unwrap();
    let mut shell = boot_shell();
    let artifacts = game::script::drive(&mut shell, &script).unwrap();
    let mut frames = 0;
    for (name, artifact) in &artifacts {
        match artifact {
            game::script::Artifact::Frame(fb) => {
                frames += 1;
                assert_parity(fb, &format!("shop_{name}"), name.trim_end_matches(".png"));
            }
            game::script::Artifact::Text(txt) => {
                let fixture =
                    std::fs::read_to_string(root().join("tests/fixtures/oracle").join(name))
                        .unwrap();
                assert_eq!(txt, &fixture, "{name} dump differs from the oracle");
            }
        }
    }
    assert_eq!(frames, 11, "the drive takes 11 shots");
    // the end state: Gold 51 (100 - 65 + 16), the FIRST club sold (the
    // bought one remains, still armed id 7), mode 0 at the held dialogue
    assert_eq!(shell.world.gold, 51);
    let p = shell.world.actors[0].as_ref().unwrap();
    let tags: Vec<i32> = p
        .var_int_arr_k
        .iter()
        .take_while(|&&t| t != 0)
        .copied()
        .collect();
    assert_eq!(
        tags,
        vec![0x103, 0x205, 0x205, 0x205, 7],
        "shirt + 3 poisons + the bought club"
    );
    assert_eq!(
        p.var_byte_j, 7,
        "selling the armed club re-armed the other one"
    );
    assert_eq!(shell.screen(), Some(Screen::Gameplay));
}

/// A second buy of the 65-gold club at 35 gold fails the `var_int_b >=
/// price` check: nothing is bought or charged, but the node unmarks and
/// the title refreshes (faithful).
#[test]
fn buy_without_enough_gold_is_a_paid_noop() {
    let mut shell = boot_shell();
    let script = format!(
        "{TO_HOLD}callf\nwait 300\ntap fire\nwait 300\n\
         tap down\nwait 300\ntap fire\nwait 300\n\
         tap fire\nwait 300\n" // the rebuild reset the cursor to the RED axe — dead
    );
    game::script::drive(&mut shell, &script).unwrap();
    assert_eq!(shell.world.gold, 35, "one club bought");
    // navigate to the club again and fire: 35 < 65 -> no purchase
    game::script::drive(&mut shell, "tap down\nwait 300\ntap fire\nwait 300\n").unwrap();
    assert_eq!(
        shell.world.gold, 35,
        "the gold check blocked the second buy"
    );
    let p = shell.world.actors[0].as_ref().unwrap();
    let clubs = p
        .var_int_arr_k
        .iter()
        .take_while(|&&t| t != 0)
        .filter(|&&t| t == 7)
        .count();
    assert_eq!(clubs, 2, "still exactly the two clubs");
}

/// hideNotify parks the loop and swaps to m22 from anywhere outside the
/// guarded modes; the YES key restores the SAVED mode (here: gameplay).
#[test]
fn interrupt_restores_gameplay() {
    let mut shell = boot_shell();
    game::script::drive(&mut shell, TO_HOLD).unwrap();
    assert_eq!(shell.screen(), Some(Screen::Gameplay));
    game::script::drive(&mut shell, "callhide\nwait 500\ncallshow\nwait 300\n").unwrap();
    assert_eq!(shell.screen(), Some(Screen::Interrupt));
    game::script::drive(&mut shell, "tap 22\nwait 300\n").unwrap();
    assert_eq!(shell.screen(), Some(Screen::Gameplay));
}
