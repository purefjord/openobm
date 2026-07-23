//! Item-2 gates (docs/road-to-1.0.md): `play` save persistence + the
//! ctor-time `b(false)` restore (`Shell::install_save`).
//!
//! The review-found hazard this file pins: without the b(false) restore, a
//! relaunch-with-save followed by any settings-only `g()` (Save Changes,
//! the dead Sound toggle) would re-serialize a `None` player and silently
//! wipe the record — the real game preserves it because the constructor
//! pre-deserializes the player (b.java:194 -> 2924). The file helpers
//! (`read_save_file`/`write_save_file`) are the `play` frontend's whole
//! persistence layer, so gating them here gates the wiring headless.

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

fn real_blob() -> Vec<u8> {
    std::fs::read(root().join("tests/fixtures/oracle/eso_l01_dialogue2.bin")).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("to_port_save_persistence");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name)
}

/// Boot to the main menu: the op60 title key-gate needs one FIRE.
const TO_MENU: &str = "wait 12000\ntap fire\nwait 1000\n";

/// The proven fast pre-roll (play.rs / loop-16): title key -> New Game ->
/// class fire (Monk) -> the L01 load into the first-dialogue hold.
const TO_GAMEPLAY: &str =
    "timescale 10\nwait 5000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\nwait 20000\n";

/// The wipe scenario: install the REAL captured record (loop-18 fixture),
/// fire a settings-only `g()` twice (the dead Sound toggle — o flips on,
/// then back off) — the player must survive both writes because
/// `install_save` pre-deserialized it into slot 0.
#[test]
fn a_settings_only_write_preserves_the_installed_player() {
    let fixture = real_blob();
    let mut shell = boot_shell();
    shell.install_save(fixture.clone()).unwrap();

    shell.sound_toggle_for_test(); // l(); g() — o now 1
    let blob = shell.save_and_get_blob();
    assert_eq!(blob[4], 1, "the player SURVIVED the settings-only write");
    assert_eq!(blob[3], 1, "o flipped on");

    shell.sound_toggle_for_test(); // o back to the fixture's 0
    let blob = shell.save_and_get_blob();
    let save = formats::parse_save(&blob).unwrap();
    let player = save.player.expect("player preserved");
    assert_eq!(&player.name, b"/l01_1.scr", "the saved level-script name");
    assert_eq!(&player.actor.model_name, b"/oh_pc.cml");
    // The strong form: restore -> re-serialize reproduces the real record
    // byte-for-byte (no level reload happened, so the loop-18 "save-load-
    // SAVE not byte-stable" caveat — the loader re-equip — does not apply).
    assert_eq!(blob, fixture, "b(false) -> g() round-trips the real blob");
}

/// `install_save` restores the bindings + sound flag into LIVE state (the
/// re-serialization below rebuilds the record from the fields, not the
/// slot, so equality proves the fields were written).
#[test]
fn install_restores_bindings_and_sound_into_live_state() {
    let mut shell = boot_shell();
    let custom = {
        let tables = shell.tables();
        game::save::build_save([49, 57, 51], true, "", None, 0, tables)
    };
    shell.install_save(custom).unwrap();
    let blob = shell.save_and_get_blob();
    assert_eq!(&blob[0..3], &[49, 57, 51], "bindings restored (QH -> num1)");
    assert_eq!(blob[3], 1, "sound flag restored");
    assert_eq!(blob[4], 0, "no player in a settings-only record");
}

/// A corrupt blob is rejected and leaves the shell untouched.
#[test]
fn a_corrupt_blob_is_rejected() {
    let mut shell = boot_shell();
    assert!(shell.install_save(vec![1, 2, 3]).is_err());
    assert!(shell.save_blob().is_none(), "the slot stays wiped");
}

/// Install-then-boot-to-menu: `boolean_b()` sees the record and `l()`
/// lifts "Load Game" into pages 0 and 5 (the timing question — install
/// runs before the op44 menu build, so the first build already sees it).
#[test]
fn an_installed_save_lifts_load_game_into_the_menu() {
    let mut shell = boot_shell();
    shell.install_save(real_blob()).unwrap();
    game::script::drive(&mut shell, TO_MENU).unwrap();
    assert_eq!(shell.mode(), 3);
    let load_game = shell.lang().get(3).to_string();
    assert!(shell.menu_pages()[0].contains(&load_game));
    assert!(shell.menu_pages()[5].contains(&load_game));
}

/// The full `play` wiring, headless: session 1 plays to L01 and writes a
/// player record through the file helpers; session 2 (a fresh shell — the
/// relaunch) reads + installs it and gets "Load Game" in the menu.
#[test]
fn relaunch_via_the_file_helpers_restores_the_save() {
    let path = scratch("eso_relaunch.bin");
    let _ = std::fs::remove_file(&path);

    let mut session1 = boot_shell();
    game::script::drive(&mut session1, TO_GAMEPLAY).unwrap();
    assert_eq!(session1.mode(), 0);
    session1.sound_toggle_for_test(); // any g(): writes the live player
    session1.sound_toggle_for_test(); // o back to 0
    let blob = session1.save_blob().expect("record written").to_vec();
    game::save::write_save_file(&path, &blob).unwrap();

    assert_eq!(
        game::save::read_save_file(&path).as_deref(),
        Some(blob.as_slice()),
        "write -> read round-trip"
    );

    let mut session2 = boot_shell(); // the relaunch
    session2
        .install_save(game::save::read_save_file(&path).unwrap())
        .unwrap();
    game::script::drive(&mut session2, TO_MENU).unwrap();
    let load_game = session2.lang().get(3).to_string();
    assert!(session2.menu_pages()[0].contains(&load_game));

    let _ = std::fs::remove_file(&path);
}

/// File-helper edges: missing, corrupt, and short files are all `None`
/// (the wiped baseline); a write replaces an existing file atomically.
#[test]
fn file_helper_edge_cases() {
    assert_eq!(
        game::save::read_save_file(&scratch("missing.bin")),
        None,
        "missing -> None"
    );

    let corrupt = scratch("corrupt.bin");
    std::fs::write(&corrupt, b"garbage").unwrap();
    assert_eq!(game::save::read_save_file(&corrupt), None);
    std::fs::write(&corrupt, [55u8, 57]).unwrap(); // shorter than the header
    assert_eq!(game::save::read_save_file(&corrupt), None);
    let _ = std::fs::remove_file(&corrupt);

    let target = scratch("replace.bin");
    let first = real_blob();
    game::save::write_save_file(&target, &first).unwrap();
    let mut second = first.clone();
    second[3] = 1; // still parseable — only the sound flag differs
    game::save::write_save_file(&target, &second).unwrap();
    assert_eq!(
        game::save::read_save_file(&target).as_deref(),
        Some(second.as_slice()),
        "rename replaced the existing file"
    );
    let _ = std::fs::remove_file(&target);
}
