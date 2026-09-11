//! Loop #21 gates: the m9 OUTRO end-transition — the LAST fenced piece of
//! the `b.java` loop. ONE script (`oracle/to_outro.txt`) at the L01 hold:
//! calllang 12 + callmode 9 (the op61 arm), the pinned outro page, the end
//! push -> the m9 reset (menu re-init + the actor NULL-ALL with `var_j_a`
//! stashed) -> mode 4 (r1 came back HASH-IDENTICAL to the committed
//! about_g-50.png — the outro credits ARE the About credits) -> mode 3 ->
//! a post-outro New Game whose spawner REUSES the stashed player.
//!
//! The drive DISPROVED the old "player stash" fence theory: the class fire
//! nulls `var_j_a` itself (2660: dup_x2 aastore + putfield — one null into
//! both), so the post-outro New Game spawns a FRESH player and the second
//! hold is RUN-1-IDENTICAL (the byte-equal dump below). The same drive also
//! exposed the map-load pickup reset (b.java:688) the first load could
//! never show.

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

#[test]
fn outro_drive_matches_the_oracle() {
    let script = std::fs::read_to_string(root().join("tests/drives/to_outro.txt")).unwrap();
    let mut shell = boot_shell();
    let artifacts = game::script::drive(&mut shell, &script).unwrap();
    let mut frames = 0;
    for (name, artifact) in &artifacts {
        match artifact {
            game::script::Artifact::Frame(fb) => {
                frames += 1;
                assert_parity(fb, &format!("outro_{name}"), name.trim_end_matches(".png"));
            }
            game::script::Artifact::Text(txt) => {
                let fixture =
                    std::fs::read_to_string(root().join("tests/fixtures/oracle").join(name))
                        .unwrap();
                assert_eq!(txt, &fixture, "{name} dump differs from the oracle");
            }
        }
    }
    assert_eq!(frames, 4, "the drive takes 4 shots");
    // The end state: a FRESH player at the second hold (the class fire
    // nulled var_j_a — factory spawn), run-1-identical loadout.
    assert_eq!(shell.screen(), Some(Screen::Gameplay));
    let p = shell.world.actors[0].as_ref().unwrap();
    let tags: Vec<i32> = p
        .var_int_arr_k
        .iter()
        .take_while(|&&t| t != 0)
        .copied()
        .collect();
    assert_eq!(
        tags,
        vec![7, 0x103, 0x205, 0x205, 0x205],
        "a fresh Monk loadout — no NG+ carry-over"
    );
    assert_eq!(p.var_byte_j, 7);
}

/// The m9 end in isolation: the null-all leaves EVERY slot empty (the dump
/// showed zero actor lines); the untouched scalars (camera, gold, the OPEN
/// dialogue) survive to the menu.
#[test]
fn outro_end_nulls_every_slot() {
    let mut shell = boot_shell();
    game::script::drive(
        &mut shell,
        "wait 12000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\nwait 45000\n\
         calllang 12\ncallmode 9\nwait 300\nsetscroll -2000\nwait 1000\n",
    )
    .unwrap();
    assert_eq!(shell.mode(), 4, "the m9 end chains into the credits");
    assert!(
        shell.world.actors.iter().all(Option::is_none),
        "all 25 slots nulled"
    );
    assert!(
        shell.world.dialogue.is_some(),
        "the open dialogue is untouched"
    );
    assert_eq!(shell.world.gold, 100);
}
