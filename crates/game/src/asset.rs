//! Asset loading + image blitting for the shell.
//!
//! Reuses the M6-validated PNG decoder (`render::sprite::load_png`, which
//! EXPANDs palette/tRNS to RGBA) so indexed game PNGs like `/main.png` decode
//! exactly as the sprite renderer already validated. Blitting matches MIDP
//! `Graphics.drawImage`: the game's UI images use binary alpha (fully opaque
//! or fully transparent), so an opaque pixel overwrites and a transparent one
//! is skipped — the full-screen parity tests would catch any blend mismatch.

use crate::fb::Fb;
use render::sprite::{load_png, Png};
use std::path::Path;

pub struct Assets {
    dir: std::path::PathBuf,
}

impl Assets {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    /// Load a game resource by its slash-rooted name (e.g. "/main.png").
    pub fn image(&self, name: &str) -> anyhow::Result<Png> {
        let path = self.dir.join(name.trim_start_matches('/'));
        let bytes =
            std::fs::read(&path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        load_png(&bytes)
    }
}

/// `Graphics.drawImage(img, x, y, TOP|LEFT)` for binary-alpha images: opaque
/// pixels overwrite the framebuffer, transparent pixels are left untouched.
pub fn draw_image(fb: &mut Fb, img: &Png, x: i32, y: i32) {
    for row in 0..img.h as i32 {
        for col in 0..img.w as i32 {
            let i = ((row as u32 * img.w + col as u32) * 4) as usize;
            if img.rgba[i + 3] != 0 {
                let rgb = ((img.rgba[i] as u32) << 16)
                    | ((img.rgba[i + 1] as u32) << 8)
                    | img.rgba[i + 2] as u32;
                fb.set(x + col, y + row, rgb);
            }
        }
    }
}
