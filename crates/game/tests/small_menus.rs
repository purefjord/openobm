//! Loop #19 gates: the Custom Controls redefine (m5/m20) and the overview
//! stat tables (m18). ONE script (`oracle/to_smallmenus.txt`) drove the real
//! jar (artifacts/smallmenus -> tests/fixtures/oracle/frames/smallmenus_*);
//! the same script drives the shell here and every shot must match its
//! fixture byte-for-byte over the LCD window. The oracle run also pinned the
//! decode's identities internally: s10 == s9 (DOWN dead — every record fits
//! the page, `q:Z` stays false), s1 == s5 == s6 (the m20 bounce restores the
//! browse title; a rebind is visually silent — the binding row paints at
//! y=333, inside the clipped band), s0 == s8 (BACK restores page 6 exactly).
//!
//! The commit path ("Save Changes" -> f <- g + `g()` — a real RMS write the
//! oracle drive deliberately avoids) is behavior-gated below through the
//! save blob's binding bytes.

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

/// Boot to the main menu (virtual time), then to the Help submenu and into
/// Custom Controls (page-6 cursor: Basic Controls -> Custom Controls).
const TO_M5: &str = "wait 12000\ntap fire\nwait 1000\ntap right\nwait 300\ntap fire\n\
                     wait 300\ntap right\nwait 300\ntap fire\nwait 300\n";

/// The unified two-sided gate: the oracle's script, byte-identical shots.
#[test]
fn small_menus_drive_matches_the_oracle() {
    let script = std::fs::read_to_string(root().join("tests/drives/to_smallmenus.txt")).unwrap();
    let mut shell = boot_shell();
    let shots: Vec<(String, Fb)> = game::script::drive(&mut shell, &script)
        .unwrap()
        .into_iter()
        .filter_map(|(name, a)| match a {
            game::script::Artifact::Frame(fb) => Some((name, fb)),
            game::script::Artifact::Text(_) => None,
        })
        .collect();
    assert_eq!(shots.len(), 19, "the drive takes 19 shots");
    for (name, fb) in &shots {
        assert_parity(
            fb,
            &format!("smallmenus_{name}"),
            name.trim_end_matches(".png"),
        );
    }
}

/// FIRE on "Save Changes" commits the edit table (f <- g) and writes the
/// real save record: the blob's first three bytes are the bindings, and a
/// menu-time record has no player (byte 4 = 0 — it does NOT count as a
/// save for `b()Z`, faithful).
#[test]
fn save_changes_commits_the_edit_table() {
    let mut shell = boot_shell();
    // capture on Quick Health, bind num1, cursor to Save Changes, FIRE
    let script = format!(
        "{TO_M5}tap fire\nwait 300\ntap num1\nwait 300\n\
         tap down\nwait 300\ntap down\nwait 300\ntap down\nwait 300\n\
         tap fire\nwait 300\n"
    );
    game::script::drive(&mut shell, &script).unwrap();
    assert_eq!(shell.screen(), Some(Screen::HelpTopics)); // mode 3, page 6
    let blob = shell.save_and_get_blob();
    assert_eq!(&blob[0..3], &[49, 57, 51], "f:[B committed (QH -> num1)");
    assert_eq!(blob[4], 0, "settings-only record: no player stored");
}

/// BACK from mode 5 discards the edit: the live table is untouched, and
/// re-entering Custom Controls re-copies f into g.
#[test]
fn back_discards_the_edit_table() {
    let mut shell = boot_shell();
    // bind num1 into the EDIT table, then BACK out without committing;
    // re-enter (g <- f again) and commit immediately.
    let script = format!(
        "{TO_M5}tap fire\nwait 300\ntap num1\nwait 300\ntap 21\nwait 300\n\
         tap fire\nwait 300\n\
         tap down\nwait 300\ntap down\nwait 300\ntap down\nwait 300\n\
         tap fire\nwait 300\n"
    );
    game::script::drive(&mut shell, &script).unwrap();
    let blob = shell.save_and_get_blob();
    assert_eq!(
        &blob[0..3],
        &[55, 57, 51],
        "the discarded edit never reached f:[B"
    );
}

/// During capture (`j:Z`), the soft keys do NOTHING — they neither bind nor
/// cancel: BACK does not leave mode 5, and the next accepted key still binds.
#[test]
fn capture_ignores_the_soft_keys() {
    let mut shell = boot_shell();
    let script = format!("{TO_M5}tap fire\nwait 300\ntap 21\nwait 300\ntap 22\nwait 300\n");
    game::script::drive(&mut shell, &script).unwrap();
    assert_eq!(
        shell.screen(),
        Some(Screen::ControlsRedefine),
        "soft keys during capture must not exit mode 5"
    );
    // the capture is STILL armed: num1 binds, then commit pins it
    let tail = "tap num1\nwait 300\n\
                tap down\nwait 300\ntap down\nwait 300\ntap down\nwait 300\n\
                tap fire\nwait 300\n";
    game::script::drive(&mut shell, tail).unwrap();
    let blob = shell.save_and_get_blob();
    assert_eq!(&blob[0..3], &[49, 57, 51], "capture survived the soft keys");
}
