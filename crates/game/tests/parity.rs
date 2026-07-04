//! Screenshot parity: the shell's painted screens vs the REAL game's LCD
//! snapshots (oracle frames captured by OracleRun, committed under
//! tests/fixtures/oracle/frames/). The shell renders the logical 240x345
//! framebuffer; the device shows the top 320 rows, so we diff that region.
//!
//! These are the menu-page checkpoints (mode 3, k=0 main menu and k=1 class
//! select). Each is `/main.png` + stamped text — every pixel byte-identical to
//! the original bytecode's output. The title (CML logo composite) and animated
//! screens land with the sprite-render integration in a later sub-slice.
//!
//! On mismatch a diff PNG is written to target/parity/ (gitignored) for
//! inspection; the fixtures themselves are regenerated only from OUR oracle
//! (never copied from the parallel codex project).

use game::asset::Assets;
use game::fb::Fb;
use game::paint::{paint_menu_page, MenuPage, LCD_H, SCREEN_H, SCREEN_W};
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

#[test]
fn main_menu_matches_oracle() {
    // recon: empty RecordStore -> page 0 = [New Game, Help, About, Exit],
    // cursor 0. (With a save present a "Continue" item would appear; that's a
    // separate fixture once save-capture lands.)
    let items = ["New Game", "Help", "About", "Exit"];
    let mut fb = Fb::new(SCREEN_W, SCREEN_H);
    paint_menu_page(
        &mut fb,
        &masks(),
        &Assets::new(root().join("assets")),
        MenuPage::Main,
        &items,
        0,
    )
    .unwrap();
    assert_parity(&fb, "main_menu_newgame.png", "main_menu");
}

#[test]
fn class_select_matches_oracle() {
    // recon: 8 classes, cursor 0 = "Monk".
    let classes = [
        "Monk",
        "Archer",
        "Knight",
        "Sorcerer",
        "Barbarian",
        "Nightblade",
        "Spellsword",
        "Battlemage",
    ];
    let mut fb = Fb::new(SCREEN_W, SCREEN_H);
    paint_menu_page(
        &mut fb,
        &masks(),
        &Assets::new(root().join("assets")),
        MenuPage::ClassSelect,
        &classes,
        0,
    )
    .unwrap();
    assert_parity(&fb, "class_select_monk.png", "class_select");
}
