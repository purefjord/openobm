//! The class-fire level load (m=6 -> 15 -> 10 -> 0) into the mode-0 gameplay
//! loop, driven from COLD BOOT through the real scripts — the state slice.
//! Byte-level validation against the real bytecode lives in the oracle sweep
//! (`level_matches_oracle`); these tests pin the CHAIN and the world-state
//! shape the L01 choreography must produce.

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

fn tap(s: &mut Shell, key: i32) {
    s.press(key);
    s.tick(50);
}

/// Boot to the class-select screen (title key -> menu -> New Game).
fn shell_at_class_select() -> Shell {
    let mut s = boot_shell();
    for _ in 0..400 {
        s.tick(50);
        if s.screen() == Some(Screen::Title) {
            break;
        }
    }
    assert_eq!(s.screen(), Some(Screen::Title), "boot never reached title");
    tap(&mut s, 53); // title key
    for _ in 0..20 {
        s.tick(50);
    }
    assert_eq!(s.screen(), Some(Screen::MainMenu));
    tap(&mut s, 53); // New Game
    assert_eq!(s.screen(), Some(Screen::ClassSelect));
    s
}

/// The full chain: class fire -> loader (m=6) -> op73 please-wait (m=15,
/// mode-gate closed) -> the level ops -> op74 + op66 intro text (m=10) ->
/// auto-scroll to the end -> mode 0 -> the entry-7 cutscene reaches its first
/// FIRE-gated guard dialogue.
#[test]
fn class_fire_loads_l01_into_gameplay() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53); // fire Monk -> the loader on /l01_1.scr
    assert_eq!(s.take_leave(), None, "the class fire is ported, no Leave");

    // The load choreography runs one opcode per frame: collect the modes seen
    // and stop at the first FIRE-gated cutscene dialogue (running the whole
    // scripted fight unattended ends in the fenced player-death screen —
    // faithfully — so the state assertions land at the first settled hold).
    let mut seen = vec![s.mode()];
    let mut fired_dialogue = false;
    for _ in 0..4000 {
        // ~200s of virtual time cap: the intro scroll alone is ~35s
        s.tick(50);
        if seen.last() != Some(&s.mode()) {
            seen.push(s.mode());
        }
        if s.mode() == 0 && s.world.dialogue.is_some() {
            fired_dialogue = true;
            break;
        }
    }
    assert_eq!(
        seen,
        vec![6, 15, 10, 0],
        "the real chain is loader -> please-wait -> intro text -> gameplay"
    );
    assert!(fired_dialogue, "the entry-7 cutscene shows guard dialogue");
    // The dialogue holds the VM (e.b(J) prologue) until FIRE after >= 1000ms:
    // an early FIRE is swallowed by the 1s rule (d(7)).
    let held = s.world.dialogue.clone();
    tap(&mut s, 53); // too early: < 1000ms open
    assert!(
        s.world.dialogue.is_some(),
        "FIRE inside 1s does not dismiss"
    );
    for _ in 0..25 {
        s.tick(50); // 1.25s more: past the 1s hold
    }
    assert_eq!(
        s.world.dialogue.as_ref().map(|d| &d.lines),
        held.as_ref().map(|d| &d.lines),
        "the dialogue text holds while open"
    );
    s.press(53); // FIRE >= 1s after open dismisses (the b(J) tail d(7))
    s.tick(50);
    assert!(s.world.dialogue.is_none(), "FIRE dismissed the dialogue");

    // The world after the load: the player (Monk, class 1) in slot 0 with the
    // camera following; the L01 map loaded; spawned NPCs present.
    let p = s.world.actors[0].as_ref().expect("player in slot 0");
    assert_eq!(p.var_byte_f, 1, "class-select cursor 0 = class 1 (Monk)");
    assert_eq!(p.var_byte_c, 1);
    assert_eq!(p.var_byte_s, 0, "the player never drops loot");
    assert!(s.world.map_w > 0 && s.world.map_h > 0, "l01_1.jtm loaded");
    assert!(!s.world.layers.is_empty());
    let npcs = (1..25).filter(|&i| s.world.actors[i].is_some()).count();
    assert!(npcs > 0, "the L01 choreography spawns guards/emperor");
    assert_eq!(s.world.gold, 100, "b:I = 100 on class fire");
    assert!(s.world.respawn != [0, 0], "op71 set the respawn anchor");
}

/// The intro text page is fed by op66 (`b.e(String)` = lang 445 via the L01
/// overlay lang table) and auto-scrolls without input.
#[test]
fn intro_text_appears_and_autoscrolls_unattended() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    let mut reached_intro = false;
    for _ in 0..4000 {
        s.tick(50);
        if s.screen() == Some(Screen::IntroText) {
            reached_intro = true;
        }
        if reached_intro && s.mode() == 0 {
            return; // scrolled through unattended
        }
    }
    panic!("the intro text never auto-scrolled into mode 0 (reached_intro={reached_intro})");
}
