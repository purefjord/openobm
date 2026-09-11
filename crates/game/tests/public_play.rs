//! Run with `cargo test -p game --features assets --test public_play` after
//! extracting your own archive. No reference captures are read or compared.
use game::script::{drive, Artifact};
use game::shell::Shell;
use game::text::TextMasks;
use std::path::PathBuf;

#[test]
fn public_fonts_support_menus_dialogue_gameplay_and_level_loading() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let assets = root.join("assets");
    assert!(
        assets.join("oh_menu.cml").is_file(),
        "Extract your own game archive into assets/ first; see README.md"
    );
    let cases = [
        ("boot", include_str!("../../../tests/drives/to_boot.txt")),
        ("about", include_str!("../../../tests/drives/to_about.txt")),
        ("help", include_str!("../../../tests/drives/to_help.txt")),
        (
            "menus",
            include_str!("../../../tests/drives/to_smallmenus.txt"),
        ),
        (
            "gameplay",
            include_str!("../../../tests/drives/to_gameplay.txt"),
        ),
        (
            "levels",
            include_str!("../../../tests/drives/to_alllevels.txt"),
        ),
    ];
    for (name, script) in cases {
        let mut shell = Shell::boot(&assets, TextMasks::bundled()).expect("boot with public fonts");
        let checkpoints = drive(&mut shell, script).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            shell.unported_boundary().is_none(),
            "{name}: unported boundary"
        );
        let frame = shell.render().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            frame.pixels().iter().any(|&p| p != 0),
            "{name}: empty frame"
        );
        // Optional local screenshots for visual review; never required by the test.
        if let Some(output) = std::env::var_os("OPENOBM_SMOKE_OUTPUT") {
            let output = PathBuf::from(output).join(name);
            frame.save_png(&output.join("final.png")).unwrap();
            for (file, artifact) in checkpoints {
                if let Artifact::Frame(fb) = artifact {
                    fb.save_png(&output.join(file)).unwrap();
                }
            }
        }
    }
}
