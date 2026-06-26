//! `map-view` — interactive macroquad window: draws a `.jtm` map as iso
//! diamonds (verified transform), pans with the arrow keys / WASD, and shows the
//! tile under the cursor. Build with `--features interactive`.
//!
//! Usage: map-view <assets_dir> <name.jtm>

use formats::{parse_jtm, screen_to_world, AssetStore, JtmMap, Vec2i};
use macroquad::prelude::*;
use render::{tile_color, tile_to_screen, Renderer, Rgba, WORLD_PER_TILE};

/// macroquad-backed [`Renderer`]: issues immediate-mode draw calls.
struct MqRenderer;

fn to_mq(c: Rgba) -> Color {
    Color::from_rgba(c.0, c.1, c.2, c.3)
}

impl Renderer for MqRenderer {
    fn clear(&mut self, color: Rgba) {
        clear_background(to_mq(color));
    }

    fn fill_diamond(&mut self, cx: i32, cy: i32, hw: i32, hh: i32, color: Rgba) {
        let (cx, cy, hw, hh) = (cx as f32, cy as f32, hw as f32, hh as f32);
        let top = vec2(cx, cy - hh);
        let bottom = vec2(cx, cy + hh);
        let left = vec2(cx - hw, cy);
        let right = vec2(cx + hw, cy);
        let col = to_mq(color);
        draw_triangle(top, right, bottom, col);
        draw_triangle(top, bottom, left, col);
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Oblivion map-view".to_owned(),
        window_width: 960,
        window_height: 640,
        ..Default::default()
    }
}

/// Recover the tile under a screen pixel by inverting the iso projection.
fn tile_under(map: &JtmMap, screen: Vec2i, camera: Vec2i) -> Option<(usize, usize)> {
    let w = screen_to_world(Vec2i::new(screen.x - camera.x, screen.y - camera.y));
    // Round to the nearest tile (world units per tile = WORLD_PER_TILE).
    let half = WORLD_PER_TILE / 2;
    let tx = (w.x + half).div_euclid(WORLD_PER_TILE);
    let ty = (w.y + half).div_euclid(WORLD_PER_TILE);
    if tx < 0 || ty < 0 || tx as usize >= map.width || ty as usize >= map.height {
        return None;
    }
    Some((tx as usize, ty as usize))
}

#[macroquad::main(window_conf)]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: map-view <assets_dir> <name.jtm>");
        return;
    }
    let store = AssetStore::new(&args[0]);
    let res = if args[1].starts_with('/') {
        args[1].clone()
    } else {
        format!("/{}", args[1])
    };
    let bytes = match store.load(&res) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("loading {res}: {e}");
            return;
        }
    };
    let map = match parse_jtm(&bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("parse {res}: {e}");
            return;
        }
    };

    // Start centered on the map.
    let mut camera = Vec2i::new(screen_width() as i32 / 2, 80);
    let mut r = MqRenderer;

    loop {
        let speed = 6;
        if is_key_down(KeyCode::Left) || is_key_down(KeyCode::A) {
            camera.x += speed;
        }
        if is_key_down(KeyCode::Right) || is_key_down(KeyCode::D) {
            camera.x -= speed;
        }
        if is_key_down(KeyCode::Up) || is_key_down(KeyCode::W) {
            camera.y += speed;
        }
        if is_key_down(KeyCode::Down) || is_key_down(KeyCode::S) {
            camera.y -= speed;
        }

        r.clear(Rgba::rgb(12, 12, 16));
        // painter's order, all layers (floor draws empties, overlays skip them)
        for layer in 0..map.layers.len() {
            for sum in 0..=(map.width + map.height).saturating_sub(2) {
                for x in 0..map.width {
                    let Some(y) = sum.checked_sub(x).filter(|&y| y < map.height) else {
                        continue;
                    };
                    let Some(id) = map.tile(layer, x, y) else {
                        continue;
                    };
                    if layer != 0 && id == 0 {
                        continue;
                    }
                    let p = tile_to_screen(x, y);
                    r.fill_diamond(p.x + camera.x, p.y + camera.y, 8, 4, tile_color(id));
                }
            }
        }

        let (mx, my) = mouse_position();
        let readout = match tile_under(&map, Vec2i::new(mx as i32, my as i32), camera) {
            Some((tx, ty)) => {
                let id = map.tile(0, tx, ty).unwrap_or(0);
                format!("tile ({tx},{ty}) id={id}")
            }
            None => "tile (-,-)".to_string(),
        };
        draw_text(
            &format!(
                "{res}  {}x{}  {}  [arrows/WASD pan]",
                map.width, map.height, readout
            ),
            10.0,
            22.0,
            22.0,
            WHITE,
        );

        next_frame().await;
    }
}
