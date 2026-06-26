//! `map-shot` — headless renderer that draws a `.jtm` map to a PNG using the
//! verified isometric transform. This produces the M2 visual checkpoint without
//! needing a display or GPU.
//!
//! Usage: map-shot <assets_dir> <name.jtm> <out.png>

use anyhow::{bail, Context, Result};
use formats::{parse_jtm, AssetStore, Vec2i};
use render::{
    draw_jtm_layer, tile_to_screen, ImageRenderer, Renderer, Rgba, TILE_HALF_H, TILE_HALF_W,
};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        bail!("usage: map-shot <assets_dir> <name.jtm> <out.png>");
    }
    let store = AssetStore::new(&args[0]);
    let res = if args[1].starts_with('/') {
        args[1].clone()
    } else {
        format!("/{}", args[1])
    };
    let bytes = store.load(&res).with_context(|| format!("loading {res}"))?;
    let map = parse_jtm(&bytes).map_err(|e| anyhow::anyhow!("parse {res}: {e}"))?;
    if map.width == 0 || map.height == 0 {
        bail!("{res} is empty ({}x{})", map.width, map.height);
    }

    // Screen-space bounding box over every tile center, plus a tile-sized margin.
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
    let margin = 8;
    let pad_x = TILE_HALF_W + margin;
    let pad_y = TILE_HALF_H + margin;
    let img_w = (max_x - min_x + 2 * pad_x) as u32;
    let img_h = (max_y - min_y + 2 * pad_y) as u32;
    let camera = Vec2i::new(-min_x + pad_x, -min_y + pad_y);

    let mut img = ImageRenderer::new(img_w, img_h);
    img.clear(Rgba::rgb(12, 12, 16));
    // Floor layer draws empties; overlay layers composite over it.
    for layer in 0..map.layers.len() {
        draw_jtm_layer(&mut img, &map, layer, camera, layer != 0);
    }

    let out = std::path::Path::new(&args[2]);
    img.save_png(out)
        .with_context(|| format!("writing {}", out.display()))?;
    println!(
        "{res}: {}x{} tiles, {} layers -> {} ({}x{} px)",
        map.width,
        map.height,
        map.layers.len(),
        out.display(),
        img_w,
        img_h
    );
    Ok(())
}
