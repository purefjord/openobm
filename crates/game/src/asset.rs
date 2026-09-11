//! Asset loading + image blitting for the shell.
//!
//! Reuses the M6-validated PNG decoder (`render::sprite::load_png`, which
//! EXPANDs palette/tRNS to RGBA) so indexed game PNGs like `/main.png` decode
//! exactly as the sprite renderer already validated. Blitting matches MIDP
//! `Graphics.drawImage`: the game's UI images use binary alpha (fully opaque
//! or fully transparent), so an opaque pixel overwrites and a transparent one
//! is skipped — the full-screen parity tests would catch any blend mismatch.
//!
//! Images are cached by name (the original `g.var_java_util_Hashtable_a`
//! Image cache) — the tile/sprite draw hits the same sheet hundreds of times
//! per frame.

use crate::fb::Fb;
use render::sprite::{load_png, Png};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

/// Resolve a MIDP resource name inside the supplied asset directory.
/// Reject host-specific paths and symlinks that resolve outside that directory.
pub(crate) fn resource_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let relative = name.strip_prefix('/').unwrap_or(name);
    anyhow::ensure!(
        !relative.is_empty()
            && !relative.contains(['\\', ':'])
            && !relative.chars().any(char::is_control)
            && Path::new(relative)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "invalid resource name"
    );
    let root = root.canonicalize()?;
    let resolved = root.join(relative).canonicalize()?;
    anyhow::ensure!(
        resolved.starts_with(&root) && resolved.is_file(),
        "resource is not a file inside the asset directory"
    );
    Ok(resolved)
}

pub struct Assets {
    dir: std::path::PathBuf,
    cache: RefCell<HashMap<String, Rc<Png>>>,
}

impl Assets {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Load a game resource by its slash-rooted name (e.g. "/main.png").
    pub fn image(&self, name: &str) -> anyhow::Result<Rc<Png>> {
        if let Some(img) = self.cache.borrow().get(name) {
            return Ok(img.clone());
        }
        let path = resource_path(&self.dir, name)?;
        let bytes =
            std::fs::read(&path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let img = Rc::new(load_png(&bytes)?);
        self.cache
            .borrow_mut()
            .insert(name.to_string(), img.clone());
        Ok(img)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_stay_inside_the_asset_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("assets");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("inside.scr"), b"inside").unwrap();
        std::fs::write(temp.path().join("outside.scr"), b"outside").unwrap();
        assert_eq!(
            std::fs::read(resource_path(&root, "/inside.scr").unwrap()).unwrap(),
            b"inside"
        );
        for name in [
            "/../outside.scr",
            "../outside.scr",
            "/C:/outside.scr",
            "C:\\outside.scr",
            "\\\\server\\outside.scr",
            "//outside.scr",
            "/inside.scr:stream",
            "/missing.scr",
            "/",
        ] {
            assert!(resource_path(&root, name).is_err(), "accepted {name:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn resource_symlink_cannot_escape_the_asset_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("assets");
        std::fs::create_dir(&root).unwrap();
        let outside = temp.path().join("outside.scr");
        std::fs::write(&outside, b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link.scr")).unwrap();
        assert!(resource_path(&root, "/link.scr").is_err());
    }
}
