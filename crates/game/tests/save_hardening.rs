//! The untrusted-save boundary (port audit rank 5): a `playdata/eso.bin`
//! that is STRUCTURALLY valid but SEMANTICALLY hostile must degrade to "no
//! save", never take down the window.
//!
//! Why this file exists: `read_save_file` validates a record by an exact
//! `parse_save` -> `serialize_save` round-trip, which is a check on layout
//! alone. Every value inside stays attacker-chosen, and the restore path
//! indexes on four of them — the model name (a filesystem read + a
//! `Path::join` an absolute or `..` name escapes), the class byte
//! (`class_init`'s subtype-5 `expect`), `var_byte_j` (`class_progression`'s
//! subtype-4 `expect`, read from the RAW saved value before any equip can
//! overwrite it), and each item tag (its kind's subtype row). Before
//! `Shell::install_save` gained `validate_restorable`, all four were panics
//! reachable from a hand-edited file, and Item 2 made that file real.
//!
//! The canonical path is deliberately untouched: `restore_actor` keeps its
//! panics (a self-produced blob failing these checks is a port bug, not bad
//! input), and `ModelCache::get` still panics — only the new `try_load` half
//! is fallible.

use formats::save::SaveActor;
use game::shell::Shell;
use game::text::TextMasks;
use game::world::ModelCache;
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
    let dir = std::env::temp_dir().join("to_port_save_hardening");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name)
}

/// Build a hostile record from the REAL captured one by mutating the player
/// and re-serializing — so it is a genuine `ESO` record in every structural
/// respect and clears `read_save_file` unchanged. That is the point: the
/// structural gate cannot see any of these.
fn hostile(mutate: impl FnOnce(&mut SaveActor)) -> Vec<u8> {
    let mut save = formats::parse_save(&real_blob()).unwrap();
    mutate(&mut save.player.as_mut().unwrap().actor);
    formats::serialize_save(&save)
}

/// The regression test for the bug itself: the structural gate ACCEPTS a
/// hostile record off disk, and the semantic gate is what stops it. If
/// `read_save_file` ever starts rejecting this blob the test still holds its
/// meaning — but the `is_some` assert is what proves the two gates are
/// doing different jobs.
#[test]
fn the_structural_gate_passes_a_record_the_semantic_gate_rejects() {
    let path = scratch("hostile_traversal.bin");
    let blob = hostile(|a| a.model_name = b"/../../../../Windows/win.ini".to_vec());
    game::save::write_save_file(&path, &blob).unwrap();

    let read = game::save::read_save_file(&path);
    assert_eq!(
        read.as_deref(),
        Some(blob.as_slice()),
        "the round-trip gate cannot see a hostile model name"
    );

    let mut shell = boot_shell();
    let err = shell.install_save(read.unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("not a plain relative path"),
        "rejected for escaping the asset root, got: {err}"
    );

    let _ = std::fs::remove_file(&path);
}

/// Each hostile field, one per panic site the restore path used to hit.
#[test]
fn every_hostile_field_is_an_error_not_a_panic() {
    // (label, blob, the substring the message must carry)
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "parent-dir traversal",
            hostile(|a| a.model_name = b"/../Cargo.toml".to_vec()),
            "not a plain relative path",
        ),
        (
            "backslash traversal",
            hostile(|a| a.model_name = br"..\Cargo.toml".to_vec()),
            "backslash",
        ),
        (
            "empty model name",
            hostile(|a| a.model_name = b"/".to_vec()),
            "empty resource name",
        ),
        (
            "missing model resource",
            hostile(|a| a.model_name = b"/no_such_model.cml".to_vec()),
            "model resource",
        ),
        (
            "non-cml model resource",
            hostile(|a| a.model_name = b"/l01_1.scr".to_vec()),
            "model cml parse",
        ),
        (
            "out-of-range class",
            hostile(|a| a.var_byte_f = 99),
            "class 99 has no subtype-5 row",
        ),
        (
            "negative class",
            hostile(|a| a.var_byte_f = -3),
            "class -3 has no subtype-5 row",
        ),
        (
            "out-of-range equipped weapon (h.f)",
            hostile(|a| a.var_byte_j = 99),
            "weapon 99 has no subtype-4 row",
        ),
        (
            "item kind out of range",
            hostile(|a| a.items = vec![(0x03, 1)]),
            "item kind 3 out of range",
        ),
        (
            "item id out of range",
            hostile(|a| a.items = vec![(0x00, 250)]),
            "id 250 has no subtype-4 row",
        ),
    ];

    for (label, blob, needle) in cases {
        // Every case must still be a structurally valid record, or it would
        // be caught by the cheap gate and prove nothing.
        assert_eq!(
            formats::serialize_save(&formats::parse_save(&blob).unwrap()),
            blob,
            "{label}: the case must clear the structural round-trip gate"
        );
        let mut shell = boot_shell();
        let err = match shell.install_save(blob) {
            Ok(()) => panic!("{label}: was accepted, but must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains(needle),
            "{label}: expected a message containing {needle:?}, got: {err}"
        );
    }
}

/// A rejected record leaves the shell EXACTLY as it was — validation runs
/// before the first field write, so a hostile blob cannot half-install its
/// bindings or sound flag on the way to the error.
#[test]
fn a_rejected_record_leaves_the_shell_untouched() {
    let mut save = formats::parse_save(&real_blob()).unwrap();
    save.flags = [49, 50, 51]; // not the {55,57,51} defaults
    save.bool_o = 1; // not the default off
    save.player.as_mut().unwrap().actor.var_byte_f = 99; // the rejection
    let blob = formats::serialize_save(&save);

    let mut shell = boot_shell();
    let before = shell.save_and_get_blob();
    assert!(shell.install_save(blob).is_err());

    assert_eq!(
        shell.save_and_get_blob(),
        before,
        "no binding, sound flag or player survived the rejected install"
    );
    assert_eq!(&before[0..3], &[55, 57, 51], "bindings still the defaults");
    assert_eq!(before[3], 0, "sound flag still off");
    assert_eq!(before[4], 0, "no player installed");
}

/// The no-false-positive gate: the real captured record still installs, and
/// still round-trips byte-for-byte (the Item-2 strong form, re-asserted here
/// so a future tightening of `validate_restorable` cannot quietly start
/// rejecting genuine saves).
#[test]
fn the_real_record_still_installs_and_round_trips() {
    let fixture = real_blob();
    let mut shell = boot_shell();
    shell.install_save(fixture.clone()).unwrap();
    assert_eq!(
        shell.save_and_get_blob(),
        fixture,
        "validation is transparent to a genuine record"
    );
}

/// The model-name gate on its own terms. `get` (the canonical path) keeps
/// its panic; `try_load` is the fallible half the validator calls.
#[test]
fn try_load_accepts_shipped_names_and_rejects_escaping_ones() {
    let mut models = ModelCache::new(root().join("assets"));

    // Shipped names are MIDP-absolute — the leading slash must stay legal.
    models.try_load("/oh_pc.cml").expect("a shipped model");
    models
        .try_load("oh_pc.cml")
        .expect("without the leading slash");

    for name in [
        "/../Cargo.toml",
        "../../Cargo.toml",
        r"..\Cargo.toml",
        r"C:\Windows\win.ini",
        "/",
        "",
        "/no_such_model.cml",
    ] {
        assert!(
            models.try_load(name).is_err(),
            "{name:?} must not resolve to a loadable model"
        );
    }
}
