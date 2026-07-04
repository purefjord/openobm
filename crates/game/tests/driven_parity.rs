//! Input-DRIVEN screenshot parity from REAL BOOT: one script boots the shell
//! cold (`/startup.scr` through the ported loader + script VM) and drives it
//! logo pages -> legal scroll -> title -> main menu -> class select,
//! snapshotting on each `shot` — the same contract OracleRun uses to drive the
//! real jar (`oracle/to_boot.txt` -> `tests/fixtures/oracle/frames/`). This
//! proves the checkpoints come from executing the actual startup scripts
//! through the ported `b(J)`/`a(byte)`/VM machinery, not hand-set state.
//!
//! Sync convention: `wait` advances whole frames at a fixed 50ms dt; `tap`
//! feeds a key; `shot` captures the settled frame. Wall-clock timings differ
//! from the oracle (resource loads are instant here), so each side's script
//! uses its own waits — the CONTENT of a settled screen is what must match,
//! byte-for-byte over the LCD window. Animated screens (loader, legal scroll)
//! are never gated (recon policy); the logo pages hold static for 2000ms
//! script-wait windows and ARE gated.

use game::fb::Fb;
use game::paint::LCD_H;
use game::script::{parse, Cmd};
use game::shell::{Leave, Screen, Shell};
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

/// Drive a freshly booted shell with an OracleRun-grammar script, returning
/// each `shot`'s frame by name.
fn run_script(script: &str) -> HashMap<String, Fb> {
    let mut shell = boot_shell();
    let mut shots = HashMap::new();
    for cmd in parse(script).unwrap() {
        match cmd {
            Cmd::Wait(ms) => {
                let mut left = ms as i32;
                while left > 0 {
                    shell.tick(50.min(left));
                    left -= 50;
                }
            }
            Cmd::Tap(k) | Cmd::Press(k) => {
                shell.press(k);
            }
            Cmd::Release(_) => {}
            Cmd::Shot(name) => {
                shots.insert(name, shell.render().unwrap());
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

/// The full boot at parity: the script VM steps the logo pages and the title,
/// a key releases the op60 gate, op29 chains to startup2.scr, op44 enters the
/// menu. Static checkpoints diffed byte-identical against the real game.
///
/// Timeline (50ms frames): the loader is instant, then one opcode per frame
/// (op43, op10) puts logo1 up within 2 frames; each op11 wait is 2000ms. Shots
/// land mid-window. After the title tap, the VM resumes (4x op72 + op29 + op56
/// + op43 + op44 = 7 frames = 350ms); 1000ms is comfortably settled.
#[test]
fn boot_to_class_select_at_parity() {
    let script = "\
        wait 1000\n\
        shot logo1.png\n\
        wait 2000\n\
        shot logo2.png\n\
        wait 2000\n\
        shot logo3.png\n\
        wait 2000\n\
        shot legal.png\n\
        wait 2000\n\
        wait 600\n\
        shot title.png\n\
        tap fire\n\
        wait 1000\n\
        shot menu.png\n\
        tap fire\n\
        wait 200\n\
        shot class_monk.png\n\
        tap right\n\
        wait 200\n\
        shot class_nightblade.png\n\
        tap right\n\
        wait 200\n\
        shot class_barbarian.png\n";
    let shots = run_script(script);
    assert_parity(&shots["logo1.png"], "boot_logo1.png", "driven_logo1");
    assert_parity(&shots["logo2.png"], "boot_logo2.png", "driven_logo2");
    assert_parity(&shots["logo3.png"], "boot_logo3.png", "driven_logo3");
    // legal.png is the animated legal scroll: rendered (exercises the mode-21
    // text page) but not gated — scroll position is frame-rate-coupled.
    assert!(shots.contains_key("legal.png"));
    assert_parity(&shots["title.png"], "title.png", "driven_title");
    assert_parity(&shots["menu.png"], "main_menu_newgame.png", "driven_menu");
    assert_parity(
        &shots["class_monk.png"],
        "class_select_monk.png",
        "driven_class_monk",
    );
    // The class carousel order is the CLASS TABLE order (startup.scr subtype-5
    // rows -> lang 9..16): Monk, Nightblade, Barbarian, ... — regression pin
    // for the order bug the recon doc had wrong.
    assert_parity(
        &shots["class_nightblade.png"],
        "class_select_nightblade.png",
        "driven_class_nightblade",
    );
    assert_parity(
        &shots["class_barbarian.png"],
        "class_select_barbarian.png",
        "driven_class_barbarian",
    );
}

/// Boot the shell and run frames until the title gate arms (helper for the
/// input-behavior tests below — they start where the old seeded shell did).
fn shell_at_title() -> Shell {
    let mut s = boot_shell();
    for _ in 0..400 {
        // 20s of frames — well past the ~8.4s scripted boot
        s.tick(50);
        if s.screen() == Some(Screen::Title) {
            return s;
        }
    }
    panic!("boot never reached the title");
}

fn tap(s: &mut Shell, key: i32) {
    s.press(key);
    s.tick(50);
}

#[test]
fn carousel_wraps_like_the_original() {
    // RIGHT from "New Game" cycles New Game->Help->About->Exit->New Game; a
    // full lap returns to the first item (the real menu wraps).
    let mut s = shell_at_title();
    tap(&mut s, 53); // fire: title -> (VM resumes: startup2 chain) -> menu
    for _ in 0..20 {
        s.tick(50);
    }
    assert_eq!(s.screen(), Some(Screen::MainMenu));
    for _ in 0..4 {
        tap(&mut s, 54); // right x4 = full lap
    }
    // back on New Game -> fire enters class select
    tap(&mut s, 53);
    assert_eq!(s.take_leave(), None);
    assert_eq!(s.screen(), Some(Screen::ClassSelect));
}

#[test]
fn left_wraps_to_last_item() {
    let mut s = shell_at_title();
    tap(&mut s, 53);
    for _ in 0..20 {
        s.tick(50);
    }
    assert_eq!(s.screen(), Some(Screen::MainMenu));
    tap(&mut s, 52); // left wraps to last item = "Exit"
    tap(&mut s, 53); // fire Exit -> confirm dialog (fenced boundary)
    assert_eq!(s.take_leave(), Some(Leave::Mode(19)));
}
