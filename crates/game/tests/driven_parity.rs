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
use game::shell::{Screen, Shell};
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

/// Drive a freshly booted shell with an OracleRun-grammar script through the
/// shared executor ([`game::script::drive`] — the same one-script-both-sides
/// runner the unified gates use), returning each `shot`'s frame by name.
fn run_script(script: &str) -> HashMap<String, Fb> {
    let mut shell = boot_shell();
    game::script::drive(&mut shell, script)
        .unwrap()
        .into_iter()
        .filter_map(|(name, a)| match a {
            game::script::Artifact::Frame(fb) => Some((name, fb)),
            game::script::Artifact::Text(_) => None,
        })
        .collect()
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
    tap(&mut s, 53); // fire Exit -> the m=19 confirm dialog (ported)
    assert_eq!(s.take_leave(), None);
    assert_eq!(s.screen(), Some(Screen::ExitDialog));
    // NO (b:B = 21): back to the main menu, cursor STILL on Exit (e:[B
    // persists across the mode round-trip) — fire re-opens the dialog.
    tap(&mut s, 21);
    assert_eq!(s.screen(), Some(Screen::MainMenu));
    tap(&mut s, 53);
    assert_eq!(s.screen(), Some(Screen::ExitDialog));
}

/// The About credits at a FIXED-SCROLL normalized shot (mirrors
/// `oracle/to_about.txt` -> `artifacts/textpages`): the roll auto-scrolls on
/// the wall clock, so both sides pin `g:S = -50` (the oracle pauses the loop
/// and injects via `setscroll`) and shoot. First pixel gate for the mode-4
/// render — body text + the `1~` gold credits markup; the up arrow and
/// bottom bar land in the clipped 320..345 band.
#[test]
fn about_fixed_scroll_at_parity() {
    let mut s = shell_at_title();
    tap(&mut s, 53); // title key -> menu
    for _ in 0..20 {
        s.tick(50);
    }
    assert_eq!(s.screen(), Some(Screen::MainMenu));
    tap(&mut s, 54); // right -> Help
    tap(&mut s, 54); // right -> About
    tap(&mut s, 53); // fire
    assert_eq!(s.screen(), Some(Screen::AboutRoll));
    for _ in 0..40 {
        s.tick(50); // ~2s into the roll, like the oracle drive
    }
    assert_eq!(s.screen(), Some(Screen::AboutRoll));
    s.set_scroll(-50);
    s.normalize_for_shot();
    let fb = s.render().expect("About paint");
    assert_parity(&fb, "about_g-50.png", "about_g-50");
}

/// The exit-dialog choreography at parity (mirrors `oracle/to_exit.txt` ->
/// `artifacts/exit`): LEFT from New Game wraps the carousel to Exit, fire
/// opens the m=19 confirm (static -> gated), NO returns to the menu with the
/// cursor still on Exit, fire re-opens byte-identically. The oracle run
/// verified e1 == e3 and e2 == e4 (hash-equal), so the NO-return and re-entry
/// shots gate against the same two fixtures.
#[test]
fn exit_dialog_at_parity() {
    let script = "\
        wait 9600\n\
        tap fire\n\
        wait 1000\n\
        tap left\n\
        wait 200\n\
        shot menu_exit.png\n\
        tap fire\n\
        wait 200\n\
        shot dialog.png\n\
        tap 21\n\
        wait 200\n\
        shot menu_after_no.png\n\
        tap fire\n\
        wait 200\n\
        shot dialog_again.png\n";
    let shots = run_script(script);
    assert_parity(
        &shots["menu_exit.png"],
        "main_menu_exit.png",
        "driven_menu_exit",
    );
    assert_parity(
        &shots["dialog.png"],
        "exit_dialog.png",
        "driven_exit_dialog",
    );
    assert_parity(
        &shots["menu_after_no.png"],
        "main_menu_exit.png",
        "driven_menu_after_no",
    );
    assert_parity(
        &shots["dialog_again.png"],
        "exit_dialog.png",
        "driven_exit_dialog_again",
    );
}

/// The Help/About choreography at parity (mirrors `oracle/to_help.txt` ->
/// `artifacts/help`): RIGHT to Help; fire opens menu PAGE 6 (still mode 3 —
/// the topic carousel); carousel to Game Overview; fire -> mode 23 (static
/// text page, gated); a held UP overscrolls but the per-paint clamp settles
/// the frame at g=20 (oracle-pinned); DOWN is dead (`p:Z` end-latch — the
/// original never clears it in-mode, oracle h5==h6); BACK pops 23 -> page 6
/// -> main menu with every cursor held (oracle h7==h3, h8==h0); About is
/// entered/backed out (its roll is animated + render-fenced: no shot);
/// page-6 re-entry keeps its cursor (h12==h3); Basic Controls -> mode 17
/// (gated). All static checkpoints diff byte-identical.
#[test]
fn help_about_at_parity() {
    let script = "\
        wait 9600\n\
        tap fire\n\
        wait 1000\n\
        tap right\n\
        wait 200\n\
        shot menu_help.png\n\
        tap fire\n\
        wait 200\n\
        shot page6_basic.png\n\
        tap right\n\
        wait 200\n\
        shot page6_custom.png\n\
        tap right\n\
        wait 200\n\
        shot page6_gameoverview.png\n\
        tap fire\n\
        wait 200\n\
        shot overview.png\n\
        press up\n\
        wait 4000\n\
        release up\n\
        wait 200\n\
        shot overview_scrolled.png\n\
        tap down\n\
        wait 200\n\
        shot overview_down_blocked.png\n\
        tap 21\n\
        wait 200\n\
        shot page6_after_back.png\n\
        tap 21\n\
        wait 200\n\
        shot menu_after_back.png\n\
        tap right\n\
        wait 200\n\
        shot menu_about.png\n\
        tap fire\n\
        wait 2000\n\
        tap 21\n\
        wait 200\n\
        shot menu_after_about.png\n\
        tap left\n\
        wait 200\n\
        tap fire\n\
        wait 200\n\
        shot page6_reentry.png\n\
        tap left\n\
        wait 200\n\
        tap left\n\
        wait 200\n\
        tap fire\n\
        wait 200\n\
        shot controls.png\n";
    let shots = run_script(script);
    assert_parity(
        &shots["menu_help.png"],
        "main_menu_help.png",
        "driven_menu_help",
    );
    assert_parity(
        &shots["page6_basic.png"],
        "help_page6_basic.png",
        "driven_page6_basic",
    );
    assert_parity(
        &shots["page6_custom.png"],
        "help_page6_custom.png",
        "driven_page6_custom",
    );
    assert_parity(
        &shots["page6_gameoverview.png"],
        "help_page6_gameoverview.png",
        "driven_page6_gameoverview",
    );
    assert_parity(
        &shots["overview.png"],
        "overview_page.png",
        "driven_overview",
    );
    assert_parity(
        &shots["overview_scrolled.png"],
        "overview_scrolled.png",
        "driven_overview_scrolled",
    );
    // DOWN after the end-latch: byte-identical to the scrolled frame
    assert_parity(
        &shots["overview_down_blocked.png"],
        "overview_scrolled.png",
        "driven_overview_down_blocked",
    );
    assert_parity(
        &shots["page6_after_back.png"],
        "help_page6_gameoverview.png",
        "driven_page6_after_back",
    );
    assert_parity(
        &shots["menu_after_back.png"],
        "main_menu_help.png",
        "driven_menu_after_back",
    );
    assert_parity(
        &shots["menu_about.png"],
        "main_menu_about.png",
        "driven_menu_about",
    );
    assert_parity(
        &shots["menu_after_about.png"],
        "main_menu_about.png",
        "driven_menu_after_about",
    );
    assert_parity(
        &shots["page6_reentry.png"],
        "help_page6_gameoverview.png",
        "driven_page6_reentry",
    );
    assert_parity(
        &shots["controls.png"],
        "controls_page.png",
        "driven_controls",
    );
}

/// YES (a:B = 22) on the dialog runs `b.c()`: mode 12 + notifyDestroyed. The
/// oracle run pinned the behavior (modelog: m=19 -> m=12, then "MIDlet sent
/// Destroyed Notification" and the JVM exits — `artifacts/exit/exitmodes.txt`).
/// Mode 12 is terminal: `a(byte)` latches, `run()` exits, nothing paints.
#[test]
fn exit_yes_destroys_the_midlet() {
    let mut s = shell_at_title();
    tap(&mut s, 53);
    for _ in 0..20 {
        s.tick(50);
    }
    tap(&mut s, 52); // left wraps to Exit
    tap(&mut s, 53); // fire -> dialog
    assert_eq!(s.screen(), Some(Screen::ExitDialog));
    assert!(!s.exited());
    tap(&mut s, 22); // YES
    assert!(s.exited());
    assert_eq!(s.mode(), 12);
    // mode 12 paints nothing (the real LCD keeps the last frame) — rendering
    // it is a loud error, and the mode latches against any further input.
    assert!(s.render().is_err());
    tap(&mut s, 21);
    assert_eq!(s.mode(), 12);
}
