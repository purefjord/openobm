//! Screenshot parity: the shell's paint functions vs the REAL game's LCD
//! snapshots (oracle frames captured by OracleRun, committed under
//! tests/fixtures/oracle/frames/). The shell renders the logical 240x345
//! framebuffer; the device shows the top 320 rows, so we diff that region.
//!
//! These call the paint layer directly with known state (the boot-driven
//! test in driven_parity.rs proves the same frames emerge from executing the
//! real startup scripts). On mismatch a diff PNG is written to target/parity/
//! (gitignored) for inspection; the fixtures themselves are regenerated only
//! from OUR oracle (never copied from the parallel port's notes).

use game::asset::Assets;
use game::fb::Fb;
use game::paint::{paint_menu_page, paint_startup, LCD_H, SCREEN_H, SCREEN_W};
use game::text::TextMasks;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn masks() -> TextMasks {
    TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt")).unwrap()
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

/// The three boot logo pages: startup.cml records 1..3 over the op10 colors
/// (black, black, white), no key-gate text.
#[test]
fn boot_logos_match_oracle() {
    let (masks, assets) = (masks(), Assets::new(root().join("assets")));
    for (png, bg, fixture) in [
        ("/1.png", 0x00_00_00, "boot_logo1.png"),
        ("/2.png", 0x00_00_00, "boot_logo2.png"),
        ("/3.png", 0xFF_FF_FF, "boot_logo3.png"),
    ] {
        let mut fb = Fb::new(SCREEN_W, SCREEN_H);
        paint_startup(&mut fb, &masks, &assets, png, bg, false).unwrap();
        assert_parity(&fb, fixture, fixture.trim_end_matches(".png"));
    }
}

#[test]
fn title_matches_oracle() {
    // mode 8 title, blink-ON phase: cream bg (op10's 0xF5F2E2) + /5.png logo
    // centered on (120,172) + "Press any key".
    let mut fb = Fb::new(SCREEN_W, SCREEN_H);
    paint_startup(
        &mut fb,
        &masks(),
        &Assets::new(root().join("assets")),
        "/5.png",
        0xF5_F2_E2,
        true,
    )
    .unwrap();
    assert_parity(&fb, "title.png", "title");
}

#[test]
fn main_menu_matches_oracle() {
    // recon: empty RecordStore -> page 0 = [New Game, Help, About, Exit],
    // cursor 0. (With a save present a "Continue" item would appear; that's a
    // separate fixture once save-capture lands.)
    let items: Vec<String> = ["New Game", "Help", "About", "Exit"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut fb = Fb::new(SCREEN_W, SCREEN_H);
    paint_menu_page(
        &mut fb,
        &masks(),
        &Assets::new(root().join("assets")),
        0,
        &items,
        0,
        true,
    )
    .unwrap();
    assert_parity(&fb, "main_menu_newgame.png", "main_menu");
}

#[test]
fn class_select_matches_oracle() {
    // The REAL class order is the startup.scr class-table order (subtype-5
    // rows -> lang 9..16): Monk, Nightblade, Barbarian, Archer, Knight,
    // Spellsword, Sorcerer, Battlemage. (An earlier recon note listed this
    // wrong; the boot drive's tap-right frames pinned it.)
    let classes: Vec<String> = [
        "Monk",
        "Nightblade",
        "Barbarian",
        "Archer",
        "Knight",
        "Spellsword",
        "Sorcerer",
        "Battlemage",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (masks, assets) = (masks(), Assets::new(root().join("assets")));
    for (cursor, fixture) in [
        (0usize, "class_select_monk.png"),
        (1, "class_select_nightblade.png"),
        (2, "class_select_barbarian.png"),
    ] {
        let mut fb = Fb::new(SCREEN_W, SCREEN_H);
        paint_menu_page(&mut fb, &masks, &assets, 1, &classes, cursor, true).unwrap();
        assert_parity(&fb, fixture, fixture.trim_end_matches(".png"));
    }
}
