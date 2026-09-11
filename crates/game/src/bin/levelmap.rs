//! `levelmap` — render every level's FULL map to a PNG (UESP-style
//! whole-level images) through the byte-validated shell: each level is
//! loaded by its own script (so op18 tile writes, op58 growth and spawns
//! are the real thing), then drawn with the validated tile/actor blits
//! over the entire map bounds instead of the 240x320 viewport. The two
//! op47 mazes render a seeded generation (pass a different seed for a
//! different dungeon).
//!
//!     cargo run -p game --bin levelmap --release [-- <out_dir> [seed]]
//!
//! Defaults: out_dir = artifacts/levelmaps, seed = 12345.

use anyhow::{Context, Result};
use game::shell::Shell;
use game::text::TextMasks;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn save_png(fb: &game::fb::Fb, path: &Path) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), fb.w as u32, fb.h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    let mut data = Vec::with_capacity((fb.w * fb.h * 3) as usize);
    for y in 0..fb.h {
        for x in 0..fb.w {
            let px = fb.get(x, y);
            data.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
    }
    writer.write_image_data(&data)?;
    Ok(())
}

/// (output name, the scripts to run in order — mazes need their host level's
/// map+tileset loaded first, then a seeded generation).
const LEVELS: &[(&str, &[&str])] = &[
    ("l01_prison", &["/l01_1.scr"]),
    ("l01_sewers_maze", &["/l01_1.scr", "seed", "/l01_1r.scr"]),
    ("l02_jauffre_kvatch_gate", &["/l02_2_1.scr"]),
    ("l02_kvatch", &["/l02_2.scr"]),
    ("l04_daedroth", &["/l04_4.scr"]),
    ("l04_side_maze", &["/l04_4.scr", "seed", "/l04_4r.scr"]),
    ("l05_paradise", &["/l05_5.scr"]),
    ("l06_cloud_ruler", &["/l06_6_cr.scr"]),
    ("l06_main", &["/l06_6.scr"]),
    ("l06_a", &["/l06_a.scr"]),
    ("l06_ba", &["/l06_6_ba.scr"]),
    ("l06_b", &["/l06_b.scr"]),
    ("l07", &["/l07_7.scr"]),
    ("l08", &["/l08_8.scr"]),
    ("l09", &["/l09_9.scr"]),
    ("l10", &["/l10_10.scr"]),
    ("l11", &["/l11_11.scr"]),
    ("l12", &["/l12_12.scr"]),
];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("artifacts/levelmaps"));
    let seed: u64 = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(12345);
    std::fs::create_dir_all(&out_dir)?;

    let masks = TextMasks::bundled();
    let mut shell = Shell::boot(root().join("assets"), masks)?;
    // The proven cold-boot prelude (logo -> title -> menu -> class fire).
    game::script::drive(
        &mut shell,
        "timescale 10\nwait 5000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\nwait 20000\n",
    )?;

    for (name, steps) in LEVELS {
        let mut drive = String::new();
        for step in *steps {
            if *step == "seed" {
                drive.push_str(&format!("pause\nsetseed {seed}\n"));
            } else {
                drive.push_str(&format!("callscript {step}\nunpause\nwait 12000\n"));
            }
        }
        game::script::drive(&mut shell, &drive).with_context(|| format!("driving {name}"))?;
        let fb = shell.render_level_map(true)?;
        let out = out_dir.join(format!("{name}.png"));
        save_png(&fb, &out)?;
        println!("{name}: {}x{} px -> {}", fb.w, fb.h, out.display());
    }
    Ok(())
}
