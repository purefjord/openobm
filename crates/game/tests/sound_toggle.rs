//! Item-1 gates (docs/road-to-1.0.md): the Sound toggle — b.java:1953-56,
//! DEAD CODE in this SKU. The silence finding (2026-07-23): no class in the
//! jar references any media API (constant pools carry only lcdui/midlet/rms,
//! vendor audio APIs absent), the jar ships zero audio assets, and `l()`
//! never builds a lang-4 ("Sound:") item — so the `startsWith(lang 4)` fire
//! branch can never match a real menu item. The branch is ported faithfully
//! anyway (transcription-class evidence — the direct-call tests below fire
//! the real branch body), and the deadness itself is pinned as an invariant
//! so any future menu change that surfaced a lang-4 item would fail loudly.

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

/// Boot to the main menu: the op60 title key-gate needs one FIRE.
const TO_MENU: &str = "wait 12000\ntap fire\nwait 1000\n";

/// The proven fast pre-roll (play.rs / loop-16): title key -> New Game ->
/// class fire (Monk) -> the L01 load into the first-dialogue hold.
const TO_GAMEPLAY: &str =
    "timescale 10\nwait 5000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\nwait 20000\n";

/// The no-player path: `o` flips, the record is settings-only (player byte
/// 0 — `has_save` faithfully stays false, the loop-19 class), no mode
/// change, and a second toggle flips `o` back.
#[test]
fn sound_toggle_writes_a_settings_only_record() {
    let mut shell = boot_shell();
    game::script::drive(&mut shell, TO_MENU).unwrap();
    assert_eq!(shell.mode(), 3, "at the main menu");

    shell.sound_toggle_for_test();
    assert_eq!(shell.mode(), 3, "no mode change (the branch has none)");
    let blob = shell.save_and_get_blob();
    assert_eq!(&blob[0..3], &[55, 57, 51], "default bindings (keys 7/9/3)");
    assert_eq!(blob[3], 1, "o flipped true");
    assert_eq!(blob[4], 0, "settings-only record: no player stored");

    shell.sound_toggle_for_test();
    let blob = shell.save_and_get_blob();
    assert_eq!(blob[3], 0, "o flipped back");
    assert_eq!(blob[4], 0, "still no player");
}

/// The player-stashed path: `g()` re-serializes the live player (byte 4 = 1,
/// the saved level-script name) — plus the faithful ORDER quirk: `l()` runs
/// BEFORE `g()` (b.java:1954-56), so the FIRST toggle from a no-save state
/// rebuilds the menus while `boolean_b()` is still false ("Load Game" not
/// yet inserted); only the SECOND toggle's rebuild sees the record.
#[test]
fn sound_toggle_with_a_player_reserializes_it() {
    let mut shell = boot_shell();
    game::script::drive(&mut shell, TO_GAMEPLAY).unwrap();
    assert_eq!(shell.mode(), 0, "in gameplay (the first-dialogue hold)");
    let load_game = shell.lang().get(3).to_string();

    shell.sound_toggle_for_test(); // l() saw has_save == false, then g() wrote the player
    assert!(
        !shell.menu_pages()[0].contains(&load_game),
        "l() ran before g(): the first rebuild predates the record"
    );
    let blob = shell.save_and_get_blob();
    assert_eq!(blob[3], 1, "o flipped true");
    assert_eq!(blob[4], 1, "the live player was serialized");
    let save = formats::parse_save(&blob).unwrap();
    let player = save.player.expect("player record");
    assert_eq!(
        String::from_utf8_lossy(&player.name),
        "/l01_1.scr",
        "the record names the live level script"
    );

    shell.sound_toggle_for_test(); // this rebuild sees the record
    assert!(
        shell.menu_pages()[0].contains(&load_game),
        "the second rebuild inserts Load Game"
    );
    assert!(shell.menu_pages()[5].contains(&load_game));
    let blob = shell.save_and_get_blob();
    assert_eq!(blob[3], 0, "o flipped back");
    assert_eq!(blob[4], 1, "the player is still in the record");

    // the deadness invariant holds on the save-present menu variant too
    assert_no_sound_item(&shell);
}

/// The deadness invariant: lang 4 is exactly "Sound:" and NO item on any
/// `l()`-built page starts with it — the branch is unreachable by taps.
#[test]
fn no_menu_item_ever_starts_with_the_sound_prefix() {
    let mut shell = boot_shell();
    game::script::drive(&mut shell, TO_MENU).unwrap();
    assert_eq!(shell.mode(), 3);
    assert_eq!(shell.lang().get(4), "Sound:", "lang 4 pinned (non-empty)");
    assert_no_sound_item(&shell);
}

fn assert_no_sound_item(shell: &Shell) {
    let prefix = shell.lang().get(4);
    assert!(!prefix.is_empty());
    for (p, page) in shell.menu_pages().iter().enumerate() {
        for item in page {
            assert!(
                !item.starts_with(prefix),
                "page {p} item {item:?} would arm the dead Sound branch"
            );
        }
    }
}
