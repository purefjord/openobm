//! Debug isometric map renderer.
//!
//! Per `spec.txt`, drawing goes through a small [`Renderer`] trait so the port
//! is not wired to one graphics crate. The map-drawing logic ([`draw_jtm_layer`])
//! is backend-agnostic and places each tile with the **verified** isometric
//! transform from [`formats::world_to_screen`], so the picture is correct by
//! construction rather than by a hand-tuned projection.
//!
//! Two backends implement the trait:
//!  - [`ImageRenderer`] — a headless CPU rasterizer that writes a PNG. This is
//!    what produces the M2 visual checkpoint and can run in CI with no display.
//!  - the macroquad window in `bin/map_view.rs` (behind the `interactive`
//!    feature) — the live, pannable view with a tile-under-cursor readout.

use formats::{world_to_screen, JtmMap, Vec2i};

/// 8-bit RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba(r, g, b, 255)
    }
}

/// Minimal drawing surface the map renderer needs. A diamond is the iso tile
/// footprint; that is the only primitive required for the debug view.
pub trait Renderer {
    /// Fill the whole surface with one color.
    fn clear(&mut self, color: Rgba);
    /// Fill an axis-aligned diamond centered at `(cx, cy)` with half-width `hw`
    /// and half-height `hh`, clipped to the surface.
    fn fill_diamond(&mut self, cx: i32, cy: i32, hw: i32, hh: i32, color: Rgba);
}

/// One world unit per this many sub-units; with the game's `>>3 / >>4` shifts a
/// step of 64 world units maps to one tile of the 16x8 iso diamond lattice
/// (`world_to_screen(64,0) == (8,4)`). See `iso.rs`.
pub const WORLD_PER_TILE: i32 = 64;
/// Resulting on-screen iso tile half-extents for `WORLD_PER_TILE`.
pub const TILE_HALF_W: i32 = 8;
pub const TILE_HALF_H: i32 = 4;

/// Screen position (before camera offset) of tile `(tx, ty)`, via the verified
/// world->screen transform.
pub fn tile_to_screen(tx: usize, ty: usize) -> Vec2i {
    world_to_screen(Vec2i::new(
        tx as i32 * WORLD_PER_TILE,
        ty as i32 * WORLD_PER_TILE,
    ))
}

/// A stable, readable color for a tile id (0 = empty floor, dim).
pub fn tile_color(id: u8) -> Rgba {
    if id == 0 {
        return Rgba::rgb(28, 30, 38);
    }
    // Deterministic hue spread; distinct ids get visibly distinct colors.
    let h = (u32::from(id).wrapping_mul(2654435761)) >> 8;
    let r = 70 + (h & 0x7F) as u8;
    let g = 70 + ((h >> 7) & 0x7F) as u8;
    let b = 70 + ((h >> 14) & 0x7F) as u8;
    Rgba::rgb(r, g, b)
}

/// Draw one layer of a map as iso diamonds, in painter's order (back-to-front:
/// increasing `x + y`). `camera` is added to every tile's screen position. When
/// `skip_empty` is set, id-0 tiles are not drawn (so an overlay layer composites
/// over the floor instead of overwriting it).
pub fn draw_jtm_layer<R: Renderer>(
    r: &mut R,
    map: &JtmMap,
    layer: usize,
    camera: Vec2i,
    skip_empty: bool,
) {
    for sum in 0..=(map.width + map.height).saturating_sub(2) {
        for x in 0..map.width {
            let y = match sum.checked_sub(x) {
                Some(y) if y < map.height => y,
                _ => continue,
            };
            let Some(id) = map.tile(layer, x, y) else {
                continue;
            };
            if skip_empty && id == 0 {
                continue;
            }
            let p = tile_to_screen(x, y);
            r.fill_diamond(
                p.x + camera.x,
                p.y + camera.y,
                TILE_HALF_W,
                TILE_HALF_H,
                tile_color(id),
            );
        }
    }
}

/// Headless CPU rasterizer backed by an RGBA8 buffer; writes PNG.
pub struct ImageRenderer {
    width: i32,
    height: i32,
    pixels: Vec<u8>, // RGBA8, row-major
}

impl ImageRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width as i32,
            height: height as i32,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    fn put(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i] = c.0;
        self.pixels[i + 1] = c.1;
        self.pixels[i + 2] = c.2;
        self.pixels[i + 3] = c.3;
    }

    /// Encode the buffer to a PNG file.
    pub fn save_png(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        let w = std::io::BufWriter::new(file);
        let mut enc = png::Encoder::new(w, self.width as u32, self.height as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&self.pixels)?;
        Ok(())
    }
}

impl Renderer for ImageRenderer {
    fn clear(&mut self, color: Rgba) {
        for px in self.pixels.chunks_exact_mut(4) {
            px[0] = color.0;
            px[1] = color.1;
            px[2] = color.2;
            px[3] = color.3;
        }
    }

    fn fill_diamond(&mut self, cx: i32, cy: i32, hw: i32, hh: i32, color: Rgba) {
        if hw <= 0 || hh <= 0 {
            self.put(cx, cy, color);
            return;
        }
        // |dx|/hw + |dy|/hh <= 1, scanned by row.
        for dy in -hh..=hh {
            // span half-width at this row
            let span = hw - (hw * dy.abs()) / hh;
            for dx in -span..=span {
                self.put(cx + dx, cy + dy, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_step_matches_iso_lattice() {
        // One tile right -> (+8, +4); one tile down -> (-8, +4). Matches the
        // game's >>3 / >>4 shifts at WORLD_PER_TILE granularity.
        assert_eq!(tile_to_screen(0, 0), Vec2i::new(0, 0));
        assert_eq!(tile_to_screen(1, 0), Vec2i::new(8, 4));
        assert_eq!(tile_to_screen(0, 1), Vec2i::new(-8, 4));
        assert_eq!(tile_to_screen(1, 1), Vec2i::new(0, 8));
    }

    #[test]
    fn diamond_fills_center_and_clips_bounds() {
        let mut img = ImageRenderer::new(8, 8);
        img.clear(Rgba::rgb(0, 0, 0));
        img.fill_diamond(4, 4, 3, 2, Rgba::rgb(255, 0, 0));
        // center painted
        let i = ((4 * 8 + 4) * 4) as usize;
        assert_eq!(img.pixels[i], 255);
        // a far corner stays background (diamond, not a rect): pixel (0,0)
        assert_eq!(img.pixels[0], 0);
    }

    #[test]
    fn off_surface_draw_does_not_panic() {
        let mut img = ImageRenderer::new(4, 4);
        img.fill_diamond(-100, -100, 5, 5, Rgba::rgb(1, 2, 3));
        img.fill_diamond(1000, 1000, 5, 5, Rgba::rgb(1, 2, 3));
    }
}
