//! `sprite-shot` — render `.cml` sprite frames to PNG, headless.
//!
//! Produces two images for the M6 visual checkpoint:
//!   <out>_sheet.png  : a contact sheet of every animation group's frames,
//!                      cropped from the sprite sheet with flip applied — if the
//!                      `.cml` frame rects are right, these are clean character
//!                      poses; if wrong, misaligned garbage.
//!   <out>_map.png    : a standing frame composited over a real iso map.
//!
//! Usage: sprite-shot <assets_dir> <model.cml> <image.png> <map.jtm> <out_prefix>

use anyhow::{bail, Context, Result};
use formats::{parse_cml, parse_jtm, AssetStore, Vec2i};
use render::sprite::{load_png, FrameView};
use render::{draw_jtm_layer, tile_to_screen, ImageRenderer, Renderer, Rgba};

fn res(name: &str) -> String {
    if name.starts_with('/') {
        name.to_string()
    } else {
        format!("/{name}")
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        bail!("usage: sprite-shot <assets_dir> <model.cml> <image.png> <map.jtm> <out_prefix>");
    }
    let store = AssetStore::new(&args[0]);
    let cml = parse_cml(&store.load(&res(&args[1]))?).map_err(|e| anyhow::anyhow!("cml: {e}"))?;
    let png = load_png(&store.load(&res(&args[2]))?).context("load image")?;

    // The record whose path matches the image (the player is /c1.png).
    let want = res(&args[2]);
    let rec = cml
        .records
        .iter()
        .find(|r| r.path == want)
        .or_else(|| cml.records.first())
        .context("no records in cml")?;

    // --- contact sheet ---
    let frames: Vec<FrameView> = rec
        .anim_groups
        .iter()
        .flat_map(|g| g.frames.iter())
        .map(FrameView::from_flags)
        .collect();
    let cell_w = 44;
    let cell_h = 48;
    let cols = 8usize;
    let rows = frames.len().div_ceil(cols).max(1);
    let mut sheet = ImageRenderer::new((cols as u32) * cell_w, (rows as u32) * cell_h);
    sheet.clear(Rgba::rgb(40, 44, 52));
    for (i, f) in frames.iter().enumerate() {
        let cx = (i % cols) as i32 * cell_w as i32;
        let cy = (i / cols) as i32 * cell_h as i32;
        // center the frame in its cell
        let dx = cx + (cell_w as i32 - f.width) / 2;
        let dy = cy + (cell_h as i32 - f.height) / 2;
        sheet.blit_frame(&png, *f, dx, dy);
    }
    let sheet_path = format!("{}_sheet.png", args[4]);
    sheet.save_png(std::path::Path::new(&sheet_path))?;

    // --- player over a map ---
    let map = parse_jtm(&store.load(&res(&args[3]))?).map_err(|e| anyhow::anyhow!("jtm: {e}"))?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..map.height {
        for x in 0..map.width {
            let p = tile_to_screen(x, y);
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }
    let pad = 24;
    let img_w = (max_x - min_x + 2 * pad) as u32;
    let img_h = (max_y - min_y + 2 * pad) as u32;
    let camera = Vec2i::new(-min_x + pad, -min_y + pad);
    let mut scene = ImageRenderer::new(img_w, img_h);
    scene.clear(Rgba::rgb(12, 12, 16));
    for layer in 0..map.layers.len() {
        draw_jtm_layer(&mut scene, &map, layer, camera, layer != 0);
    }
    // Place the first frame near the map center, anchored by its draw offset.
    if let Some(f0) = frames.first() {
        let cx = map.width / 2;
        let cy = map.height / 2;
        let p = tile_to_screen(cx, cy);
        scene.blit_frame(
            &png,
            *f0,
            p.x + camera.x + f0.off_x - f0.width / 2,
            p.y + camera.y + f0.off_y - f0.height,
        );
    }
    let map_path = format!("{}_map.png", args[4]);
    scene.save_png(std::path::Path::new(&map_path))?;

    println!(
        "{}: {} groups, {} frames -> {} + {}",
        want,
        rec.anim_groups.len(),
        frames.len(),
        sheet_path,
        map_path
    );
    Ok(())
}
