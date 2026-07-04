//! Input-DRIVEN screenshot parity: one script drives the shell's mode machine
//! (title -> fire -> main menu -> fire -> class select), snapshotting on each
//! `shot` — the same contract OracleRun uses to drive the real jar. This proves
//! the checkpoints come from the real `b(J)` input dispatch + set-mode
//! transitions, not hand-set state.
//!
//! Sync convention: `wait` advances the blink clock; `tap` feeds a key; `shot`
//! captures the settled frame. Menus are static (input-gated), so the only
//! time-dependent pixel is the title blink — and the script waits land it on a
//! known phase (no mask).

use game::asset::Assets;
use game::fb::Fb;
use game::paint::LCD_H;
use game::script::{parse, Cmd};
use game::shell::Shell;
use game::text::TextMasks;
use std::collections::HashMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Drive the shell with an OracleRun-grammar script, returning each `shot`'s
/// frame by name. `wait` ticks the blink at a fixed 50ms dt (frame counts need
/// not match the oracle; only settled screens are compared).
fn run_script(script: &str) -> HashMap<String, Fb> {
    let masks = TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt")).unwrap();
    let assets = Assets::new(root().join("assets"));
    let mut shell = Shell::new();
    let mut shots = HashMap::new();
    for cmd in parse(script).unwrap() {
        match cmd {
            Cmd::Wait(ms) => {
                let mut left = ms as i32;
                while left > 0 {
                    shell.tick_blink(50.min(left));
                    left -= 50;
                }
            }
            Cmd::Tap(k) | Cmd::Press(k) => {
                shell.press(k);
            }
            Cmd::Release(_) => {}
            Cmd::Shot(name) => {
                shots.insert(name, shell.render(&masks, &assets).unwrap());
            }
        }
    }
    shots
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
fn title_to_class_select_at_parity() {
    // The real navigation (to_gameplay.txt): title "press any key" -> main menu
    // -> New Game (cursor 0) -> class select. Two fires, exactly as recon.
    // A leading wait lands the blink on the ON phase for the title checkpoint.
    let script = "\
        shot title.png\n\
        tap fire\n\
        shot main_menu_newgame.png\n\
        tap fire\n\
        shot class_select_monk.png\n";
    let shots = run_script(script);
    assert_parity(&shots["title.png"], "title.png", "driven_title");
    assert_parity(
        &shots["main_menu_newgame.png"],
        "main_menu_newgame.png",
        "driven_menu",
    );
    assert_parity(
        &shots["class_select_monk.png"],
        "class_select_monk.png",
        "driven_class",
    );
}

#[test]
fn carousel_wraps_like_the_original() {
    // RIGHT from "New Game" cycles New Game->Help->About->Exit->New Game; a
    // full lap returns to the first item (the real menu wraps).
    use game::shell::{Screen, Shell};
    let mut s = Shell::new();
    s.press(53); // fire: title -> main menu
    assert_eq!(s.screen(), Screen::MainMenu);
    for _ in 0..4 {
        s.press(54); // right x4 = full lap
    }
    // back on New Game -> fire enters class select
    assert_eq!(s.press(53), None);
    assert_eq!(s.screen(), Screen::ClassSelect);
}

#[test]
fn left_wraps_to_last_item() {
    use game::shell::{Leave, Screen, Shell};
    let mut s = Shell::new();
    s.press(53); // -> main menu, cursor 0 (New Game)
    s.press(52); // left wraps to last item = "Exit"
    assert_eq!(s.screen(), Screen::MainMenu);
    assert_eq!(s.press(53), Some(Leave::Mode(19))); // fire Exit -> dialog
}
