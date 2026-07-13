//! NON-CANONICAL widescreen VIEWER — this is tooling, not the port.
//!
//! The transcribed paint (`gpaint`/`paint`) is the single source of truth
//! and stays byte-gated at 240x320; nothing in this module is ever
//! oracle-compared and nothing here feeds back into the shell. Like
//! `render_full_map` (the levelmap atlases) and `mapforge`, this reads the
//! same validated `World` state and composites it through the same blit
//! functions (`draw_model_frame`, `draw_actor`) — just over an arbitrary
//! viewport centered on the camera actor, so the interactive frontend can
//! offer a 16:9/16:10 view of custom maps.
//!
//! Deliberately NOT drawn here (the frontend falls back to the canonical
//! frame instead): every non-gameplay mode, the dialogue box, the HUD and
//! the effects pool — menus and conversations stay pure phone.

use crate::asset::Assets;
use crate::fb::Fb;
use crate::gpaint::{draw_actor, draw_model_frame};
use crate::text::TextMasks;
use crate::world::{ModelCache, World};
use formats::lang::Lang;

/// The iso anchor of a cell — same transform the canonical paint uses
/// (`m()`'s precompute: world_to_screen of the cell origin, x shifted by
/// half a tile sprite).
fn cell_iso(x: i32, y: i32) -> (i32, i32) {
    let v = formats::world_to_screen(formats::Vec2i::new(x * 128, y * 128));
    (v.x - 16, v.y)
}

/// Largest sprite overhangs in the level tilesets (tree 30x60 rising 48,
/// the 78-wide spire cluster): cells whose anchor lands within this margin
/// of the viewport still get drawn, everything further is culled.
const MARGIN: i32 = 110;

/// Render one wide frame: `vw x vh` pixels centered on the camera-followed
/// actor (slot 0 when the camera is free). Draw order is exactly
/// `render_full_map`'s: base layers 0..len-1, then the top layer + actors
/// interleaved per cell.
#[allow(clippy::too_many_arguments)]
pub fn render_wide(
    world: &mut World,
    models: &mut ModelCache,
    assets: &Assets,
    masks: &TextMasks,
    lang: &Lang,
    level_model: &str,
    level_bg: u32,
    vw: i32,
    vh: i32,
) -> Fb {
    let mut fb = Fb::new(vw, vh);
    fb.fill(level_bg);

    // Center the view on the camera actor (a hair below center so a bit
    // more world shows ahead of the walker than behind).
    let slot = if world.cam_follow >= 0 {
        world.cam_follow as usize
    } else {
        0
    };
    let view = match world.actors.get(slot).and_then(|a| a.as_ref()) {
        Some(a) => {
            let v = formats::world_to_screen(formats::Vec2i::new(
                a.var_int_arr_b[0],
                a.var_int_arr_b[1],
            ));
            [vw / 2 - v.x, vh / 2 + 12 - v.y]
        }
        None => [vw / 2, vh / 2],
    };
    let visible = |ix: i32, iy: i32| {
        let (sx, sy) = (ix + view[0], iy + view[1]);
        sx > -MARGIN && sx < vw + MARGIN && sy > -MARGIN && sy < vh + MARGIN
    };

    let model = models.get(level_model);
    let (lcml, lanim) = (model.cml.clone(), model.anim.clone());
    for layer_idx in 0..world.layers.len().saturating_sub(1) {
        for x in 0..world.map_w {
            for y in 0..world.map_h {
                let tile = world.layers[layer_idx][(x * world.map_h + y) as usize];
                if tile == 0 {
                    continue;
                }
                let (ix, iy) = cell_iso(x, y);
                if visible(ix, iy) {
                    draw_model_frame(
                        &mut fb,
                        assets,
                        &lcml,
                        &lanim,
                        i32::from(tile),
                        ix + view[0],
                        iy + view[1],
                    );
                }
            }
        }
    }
    if !world.layers.is_empty() {
        let top = world.layers.len() - 1;
        for x in 0..world.map_w {
            for y in 0..world.map_h {
                let (ix, iy) = cell_iso(x, y);
                if !visible(ix, iy) {
                    continue;
                }
                let tile = world.layers[top][(x * world.map_h + y) as usize];
                if tile != 0 {
                    draw_model_frame(
                        &mut fb,
                        assets,
                        &lcml,
                        &lanim,
                        i32::from(tile),
                        ix + view[0],
                        iy + view[1],
                    );
                }
                for n6 in 0..world.actors.len() {
                    let hit = world.actors[n6].as_ref().is_some_and(|a| {
                        i32::from(a.var_byte_arr_a[0]) == x && i32::from(a.var_byte_arr_a[1]) == y
                    });
                    if hit {
                        let mut a = world.actors[n6].take().unwrap();
                        let anim = world.actor_anims[n6]
                            .take()
                            .expect("live actor has an anim instance");
                        let cml = models.get(&a.model_name).cml.clone();
                        draw_actor(&mut fb, assets, &cml, &anim, masks, lang, &mut a, view);
                        world.actor_anims[n6] = Some(anim);
                        world.actors[n6] = Some(a);
                    }
                }
            }
        }
    }
    fb
}
