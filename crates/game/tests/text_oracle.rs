//! The text stamper vs REAL game frames: stamp a corpus string at the exact
//! (x, y, font, color) the game's drawString used (from the recon textlog)
//! and compare the pixels against the oracle LCD snapshot of that screen.
//! This validates the whole path — capture fidelity, bbox offsets, placement
//! math — against the original bytecode's output, not the capture tool.

use game::fb::Fb;
use game::text::{GameFont, TextMasks};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn masks() -> TextMasks {
    TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt")).unwrap()
}

/// Compare `fb` against the oracle frame in a padded box around a stamped
/// string; the padding asserts placement (an off-by-one leaks ink outside).
fn assert_box_matches(fb: &Fb, real: &Fb, x: i32, y: i32, m: &game::text::Mask, pad: i32) {
    let (x0, y0) = (x + m.dx - pad, y + m.dy - pad);
    let (x1, y1) = (x + m.dx + m.mw as i32 + pad, y + m.dy + m.mh as i32 + pad);
    let mut bad = 0;
    for yy in y0..y1 {
        for xx in x0..x1 {
            if fb.get(xx, yy) != real.get(xx, yy) {
                bad += 1;
            }
        }
    }
    assert_eq!(
        bad, 0,
        "{bad} differing pixels in box ({x0},{y0})-({x1},{y1})"
    );
}

#[test]
fn masks_fixture_parses() {
    let m = masks();
    assert_eq!(m.metrics(GameFont::LargeBold).midp_height, 14);
    assert_eq!(m.metrics(GameFont::LargeBold).ascent, 15);
    // layout ground truth from the recon: the game centers as 120 - w/2
    assert_eq!(m.string_width(GameFont::LargeBold, "New Game"), 74); // -> x=83
    assert_eq!(m.string_width(GameFont::SmallBold, "Press any key"), 69); // -> x=86
}

#[test]
fn stamp_matches_real_menu_frame() {
    // main menu (recon: m=3 k=0): "New Game" bold-large white at req (83,138)
    // over the flat black band of the menu screen.
    let masks = masks();
    let real = Fb::load_png(&root().join("artifacts/recon/01_menu_restored.png")).unwrap();
    let mut fb = Fb::new(240, 320);
    fb.fill(0x000000);
    masks.stamp(
        &mut fb,
        GameFont::LargeBold,
        "New Game",
        83,
        138,
        0xFF_FF_FF,
    );
    assert_box_matches(
        &fb,
        &real,
        83,
        138,
        masks.get(GameFont::LargeBold, "New Game"),
        2,
    );
}

#[test]
fn stamp_matches_real_title_frame() {
    // title (recon: m=8): "Press any key" bold-small BLACK on the cream
    // background (245,242,226) — a non-black background and black ink.
    let masks = masks();
    let real = Fb::load_png(&root().join("artifacts/recon/00_title.png")).unwrap();
    let mut fb = Fb::new(240, 320);
    fb.fill(0xF5_F2_E2);
    masks.stamp(
        &mut fb,
        GameFont::SmallBold,
        "Press any key",
        86,
        221,
        0x00_00_00,
    );
    assert_box_matches(
        &fb,
        &real,
        86,
        221,
        masks.get(GameFont::SmallBold, "Press any key"),
        2,
    );
}

#[test]
fn stamp_matches_real_class_select_frame() {
    // class select (recon: m=3 k=1): green bold-large header + white class
    // name, both centered via 120 - w/2 (58 and 101 in the textlog).
    let masks = masks();
    let real = Fb::load_png(&root().join("artifacts/recon/02_class_restored.png")).unwrap();
    let mut fb = Fb::new(240, 320);
    fb.fill(0x000000);
    masks.stamp(
        &mut fb,
        GameFont::LargeBold,
        "Select Your Class",
        58,
        114,
        0x0F_F0_00,
    );
    masks.stamp(&mut fb, GameFont::LargeBold, "Monk", 101, 138, 0xFF_FF_FF);
    assert_box_matches(
        &fb,
        &real,
        58,
        114,
        masks.get(GameFont::LargeBold, "Select Your Class"),
        2,
    );
    assert_box_matches(
        &fb,
        &real,
        101,
        138,
        masks.get(GameFont::LargeBold, "Monk"),
        2,
    );
}

#[test]
fn centering_formula_reproduces_recon_positions() {
    // x = 120 - stringWidth/2 (Java int division), recon-verified:
    let m = masks();
    let center = |f, s| 120 - m.string_width(f, s) / 2;
    assert_eq!(center(GameFont::LargeBold, "New Game"), 83);
    assert_eq!(center(GameFont::LargeBold, "Monk"), 101);
    assert_eq!(center(GameFont::LargeBold, "Select Your Class"), 58);
    assert_eq!(center(GameFont::SmallBold, "Press any key"), 86);
}
