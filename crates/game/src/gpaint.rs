//! The gameplay paint — `b.paint` case 0 (tiles + actors + HUD + dialogue),
//! case 15 (please-wait), and their helpers `r()`/`q()`/`b(Graphics)`,
//! `h.a(j, Graphics, int[])` (the actor draw), `i.a(Graphics, int[])` (the
//! effect draw), `b.a(Graphics)` (the dialogue box), and the frame blit
//! `g.a(Graphics, d, int, int, int)`. Transcribed from the CFR decompile
//! (b.java:797-894, 3121-3159; h.java:512-557; i.java; g.java:192-236) with
//! javap cross-checks; faithful quirks are kept and commented.

use crate::asset::{draw_image, Assets};
use crate::fb::Fb;
use crate::paint::{SCREEN_H, SCREEN_W};
use crate::text::{GameFont, TextMasks};
use crate::world::{frame_size_of, resolve_frame, FrameRef, ModelCache, World};
use formats::anim::Anim;
use formats::lang::Lang;
use formats::Actor;
use formats::Cml;

/// Tile geometry statics (`b.<clinit>`): world size per tile (`var_short_e`),
/// tile sprite width/height (`var_byte_i`/`var_byte_j`).
const TILE_WORLD: i32 = 128;
const TILE_W: i32 = 32;
const TILE_H: i32 = 16;

/// `m()`'s per-cell iso precompute (`var_short_arr_a`): the iso transform of
/// the cell's world origin, x shifted left by half a tile sprite.
fn cell_iso(x: i32, y: i32) -> (i32, i32) {
    let v = formats::world_to_screen(formats::Vec2i::new(x * TILE_WORLD, y * TILE_WORLD));
    (v.x - (TILE_W >> 1), v.y)
}

/// `g.a(Graphics, d, int n, int n2, int n3)` — draw group `n`'s CURRENT frame
/// at `(n2, n3)`. Static records draw the whole image at `(x+off, y+off)`;
/// animated frames draw their source sub-rect (mirrored when flipped) clipped
/// to `(x+off, y+off, min(screenW, w), min(screenH, h))`, and only when the
/// clip origin is left of/above the screen edges (`n4 < a:S && n5 < b:S` —
/// no left/top precheck, faithful). Returns the frame width (`var_short_c`).
pub fn draw_model_frame(
    fb: &mut Fb,
    assets: &Assets,
    cml: &Cml,
    anim: &Anim,
    key: i32,
    x: i32,
    y: i32,
) -> i32 {
    let Some(frame) = resolve_frame(cml, anim, key) else {
        return 0; // d3 == null / var_d_b == null
    };
    match frame {
        FrameRef::Static {
            path,
            off_x,
            off_y,
            width,
        } => {
            let img = assets.image(&path).expect("static frame image");
            draw_image(fb, &img, x + off_x, y + off_y);
            width
        }
        FrameRef::Rect { path, view: f } => {
            let n4 = x + f.off_x;
            let n5 = y + f.off_y;
            // The original clips to the 240x320 LCD; every gated paint uses
            // an LCD-sized Fb, so clipping to fb dims is behavior-identical
            // there while letting the levelmap tool blit onto a full-map
            // canvas.
            if n4 < fb.w && n5 < fb.h {
                let img = assets.image(&path).expect("frame sheet image");
                let clip_w = fb.w.min(f.width);
                let clip_h = fb.h.min(f.height);
                for row in 0..clip_h {
                    for col in 0..clip_w {
                        let src_col = if f.flip { f.width - 1 - col } else { col };
                        let px = f.src_x + src_col;
                        let py = f.src_y + row;
                        if px < 0 || py < 0 || px >= img.w as i32 || py >= img.h as i32 {
                            continue;
                        }
                        let i = ((py as u32 * img.w + px as u32) * 4) as usize;
                        if img.rgba[i + 3] == 0 {
                            continue;
                        }
                        let rgb = ((img.rgba[i] as u32) << 16)
                            | ((img.rgba[i + 1] as u32) << 8)
                            | img.rgba[i + 2] as u32;
                        fb.set(n4 + col, n5 + row, rgb);
                    }
                }
            }
            f.width
        }
    }
}

/// `h.int_a(j)` / `h.int_b(j)` — the actor's current pose frame height/width
/// (0 in the death pose).
fn pose_size(a: &Actor, cml: &Cml, anim: &Anim) -> (i32, i32) {
    if a.var_byte_e == 6 {
        return (0, 0);
    }
    let key = i32::from(a.var_byte_d)
        + i32::from(formats::actor::ANIM_STATE_OFFSETS[a.var_byte_e as usize]);
    frame_size_of(cml, anim, key)
}

/// `b.r()` (b.java:2647) — the camera keep-on-screen pass, run every paint:
/// when following a live actor, going off-screen sets the dirty flag; a set
/// dirty flag (INCLUDING one set earlier by a tile write — faithful quirk)
/// recenters the view on the actor with the pose-height y lift.
pub fn r_camera(world: &mut World, models: &mut ModelCache) {
    let q = world.cam_follow;
    if q < 0 {
        return;
    }
    let Some(a) = world.actors[q as usize].as_ref() else {
        return;
    };
    if a.var_byte_q != 0 {
        return;
    }
    let model_name = a.model_name.clone();
    let anim = world.actor_anims[q as usize]
        .as_ref()
        .expect("followed actor has an anim instance");
    let (w, h) = pose_size(a, &models.get(&model_name).cml, anim);
    let iso = a.var_int_arr_i;
    if iso[1] - h + world.view[1] < 0
        || iso[1] + world.view[1] > SCREEN_H
        || iso[0] + world.view[0] < 0
        || iso[0] + w + world.view[0] > SCREEN_W
    {
        world.dirty = true;
    }
    if world.dirty {
        world.view = [
            crate::world::CENTER_X - iso[0],
            crate::world::CENTER_Y - iso[1] + h,
        ];
    }
}

/// `b.q()` (b.java:2495) — the visible tile range from the four screen
/// corners through the inverse iso (`b.c`: screen -> world -> `>> 7`).
pub fn q_range(world: &mut World) {
    let tile = |sx: i32, sy: i32| -> [i32; 2] {
        let w = formats::screen_to_world(formats::Vec2i::new(sx, sy));
        [w.x >> 7, w.y >> 7]
    };
    let (vx, vy) = (world.view[0], world.view[1]);
    let e = tile(-vx, -vy);
    let f = tile(-vx + SCREEN_W, -vy);
    let g = tile(-vx, -vy + SCREEN_H);
    let h = tile(-vx + SCREEN_W, -vy + SCREEN_H);
    world.range_lo = [e[0], f[1]];
    world.range_hi = [h[0] + 3, g[1] + 3];
}

/// `b.b(Graphics)` (b.java:765) — re-render the base-map offscreen when the
/// dirty flag is set: fill the level clear color (`var_int_c`), then draw
/// every layer EXCEPT the last over the visible range. The frame-height
/// bottom-cull term `n` persists across cells/layers from the last non-zero
/// tile (faithful quirk).
pub fn base_map(
    world: &mut World,
    models: &mut ModelCache,
    assets: &Assets,
    level_model: &str,
    level_bg: u32,
) {
    // The shell's per-frame paint-state pass moves `dirty` into `base_stale`
    // (the real loop consumes it in every paint; we defer only the pixels).
    if world.dirty {
        world.dirty = false;
        world.base_stale = true;
    }
    if !world.base_stale && world.base_cache.is_some() {
        return;
    }
    world.base_stale = false;
    let mut cache = Fb::new(SCREEN_W, SCREEN_H);
    cache.fill(level_bg);
    let model = models.get(level_model);
    let (lcml, lanim) = (&model.cml, &model.anim);
    let mut n = 0i32;
    for layer_idx in 0..world.layers.len().saturating_sub(1) {
        let mut x = world.range_lo[0];
        while x <= world.range_hi[0] && x < world.map_w {
            let mut y = world.range_lo[1];
            while y <= world.range_hi[1] && y < world.map_h {
                if x < 0 || y < 0 {
                    y += 1;
                    continue;
                }
                let tile = world.layers[layer_idx][(x * world.map_h + y) as usize];
                let (ix, iy) = cell_iso(x, y);
                let n2 = ix + world.view[0];
                let n3 = iy + world.view[1];
                if tile != 0 {
                    n = frame_size_of(lcml, lanim, i32::from(tile)).1; // g.b(d, tile)
                }
                if !(n2 <= -TILE_W
                    || n2 >= SCREEN_W
                    || n3 <= -TILE_H
                    || n3 >= SCREEN_H + n
                    || tile == 0)
                {
                    draw_model_frame(&mut cache, assets, lcml, lanim, i32::from(tile), n2, n3);
                }
                y += 1;
            }
            x += 1;
        }
    }
    world.base_cache = Some(cache);
}

/// The floating combat text placeholders formats::combat stores, resolved to
/// the real lang strings for display + the color pick.
fn resolve_floating(lang: &Lang, s: &str) -> (String, bool, bool) {
    if s == "<471>" {
        (lang.get(471).to_string(), true, false) // dodge (green)
    } else if s == "<470>" {
        (lang.get(470).to_string(), false, true) // block (blue)
    } else if let Some(n) = s.strip_prefix("<472>") {
        (format!("{}{}", lang.get(472), n), false, false) // crit prefix
    } else {
        (s.to_string(), false, false)
    }
}

/// `h.a(j, Graphics, int[])` (h.java:512) — one actor: corpse OR shadow +
/// pose sprite, the enemy health bar, the floating damage text (color/rise
/// state initialized on first draw), and the dialogue-facing bubble.
#[allow(clippy::too_many_arguments)]
pub fn draw_actor(
    fb: &mut Fb,
    assets: &Assets,
    cml: &Cml,
    anim: &Anim,
    masks: &TextMasks,
    lang: &Lang,
    a: &mut Actor,
    view: [i32; 2],
) {
    let size = |key: i32| frame_size_of(cml, anim, key);
    let facing_key = i32::from(a.var_byte_d);
    let pose_key =
        facing_key + i32::from(formats::actor::ANIM_STATE_OFFSETS[a.var_byte_e as usize]);
    let iso = a.var_int_arr_i;
    if a.var_byte_q == 1 || a.var_byte_e == 6 {
        // The corpse frame (-55), centered on the walk-frame width.
        let walk_w = size(facing_key).0; // var_byte_d + offsets[0]
        let (cw, ch) = size(-55);
        let x = iso[0] + view[0] + (walk_w >> 1) - (cw >> 1);
        let y = iso[1] + view[1] - ch + 3;
        draw_model_frame(fb, assets, cml, anim, -55, x, y);
        return;
    }
    // Shadow (-56), centered on the walk-frame width; melee states 2/3 sit
    // 3px lower.
    let walk_w = size(facing_key).0;
    let (sw, sh) = size(-56);
    let extra = if a.var_byte_e == 2 || a.var_byte_e == 3 {
        3
    } else {
        0
    };
    draw_model_frame(
        fb,
        assets,
        cml,
        anim,
        -56,
        iso[0] + view[0] + (walk_w >> 1) - (sw >> 1),
        iso[1] + view[1] - sh + 3 + extra,
    );
    // The pose sprite.
    let pose_h = size(pose_key).1;
    draw_model_frame(
        fb,
        assets,
        cml,
        anim,
        pose_key,
        iso[0] + view[0],
        iso[1] + view[1] - pose_h,
    );
    // Enemy health bar (alive, non-player, faction 0).
    if a.var_byte_q == 0 && a.var_byte_c != 1 && a.var_byte_r == 0 {
        let (fw, fh) = size(facing_key);
        let n2 = iso[0] + view[0] + (fw >> 1) - 10;
        let n = iso[1] + view[1] - fh - 6;
        fb.fill_rect(n2, n, 21, 1, 0xFF_FF_FF); // drawRect 20x3 outline
        fb.fill_rect(n2, n + 3, 21, 1, 0xFF_FF_FF);
        fb.fill_rect(n2, n, 1, 4, 0xFF_FF_FF);
        fb.fill_rect(n2 + 20, n, 1, 4, 0xFF_FF_FF);
        let hp = 19 * i32::from(a.var_short_q) / i32::from(a.var_short_o);
        fb.fill_rect(n2 + 1, n + 1, hp, 2, 0xFF_00_00);
    }
    // Floating damage text: first draw initializes the rise position + color.
    if a.var_byte_q == 0 && a.floating_text.is_some() {
        let (fw, fh) = size(facing_key);
        if a.q_field == 0 {
            let q0 = iso[1] - fh - if a.var_byte_c == 1 { 6 } else { 10 };
            a.q_field = q0 as i16;
            a.r_field = q0 as i16;
            let s = a.floating_text.as_deref().unwrap();
            let (_, dodge, block) = resolve_floating(lang, s);
            if dodge {
                a.var_int_c = 65280;
                a.var_int_d = 8704;
            } else if block {
                a.var_int_c = 255;
                a.var_int_d = 34;
            } else {
                a.var_int_c = 0xFF_0000;
                a.var_int_d = 0x22_0000;
            }
        }
        let s = a.floating_text.clone().unwrap();
        let (text, _, _) = resolve_floating(lang, &s);
        // Drawn with the Graphics default font (no setFont has run yet in
        // paint case 0) = the MIDP default medium.
        masks.stamp(
            fb,
            GameFont::MediumPlain,
            &text,
            iso[0] + view[0] + (fw >> 1) - 10,
            i32::from(a.q_field) + view[1],
            a.var_int_c as u32,
        );
    }
    // The dialogue-facing bubble (-54) + face icon.
    if a.var_byte_g != -1 {
        let (fw, _) = size(facing_key);
        let pose_h2 = size(pose_key).1;
        // NOTE the y term subtracts the -54 frame WIDTH (g.a, not g.b) — a
        // faithful quirk of the original.
        let bubble_w = size(-54).0;
        let n2 = iso[0] + view[0] + fw - 4;
        let mut n = iso[1] + view[1] - pose_h2 - bubble_w - 4;
        if a.var_byte_c != 1 {
            n -= 8;
        }
        draw_model_frame(fb, assets, cml, anim, -54, n2, n);
        if a.var_byte_g != -2 {
            draw_model_frame(fb, assets, cml, anim, i32::from(a.var_byte_g), n2, n);
        }
    }
}

/// `i.a(Graphics, int[])` — draw every armed, un-held effect slot: world ->
/// iso, screen-bounds check, SEEK the oh_magic group to the slot's frame
/// counter (mutating the shared cursor — faithful), draw.
fn draw_effects(fb: &mut Fb, assets: &Assets, models: &mut ModelCache, world: &mut World) {
    let magic = models.get("/oh_magic.cml");
    let pool = *world.effects.raw();
    let mcml = magic.cml.clone();
    let mut s = 0usize;
    while s < formats::effects::POOL_LEN {
        if pool[s] == -1 || (i32::from(pool[s + 4]) & 0xFF00) == 65280 {
            s += formats::effects::STRIDE;
            continue;
        }
        let v = formats::world_to_screen(formats::Vec2i::new(
            i32::from(pool[s + 1]),
            i32::from(pool[s + 2]),
        ));
        let x = v.x + world.view[0];
        let y = v.y + world.view[1];
        // Faithful bounds: `> a:S`/`> b:S` (inclusive edges pass).
        if !(0..=SCREEN_W).contains(&x) || !(0..=SCREEN_H).contains(&y) {
            s += formats::effects::STRIDE;
            continue;
        }
        let kind = i32::from(pool[s]) & 0xFF;
        magic.anim.seek(kind, i32::from(pool[s + 4]));
        draw_model_frame(fb, assets, &mcml, &magic.anim, kind, x, y);
        s += formats::effects::STRIDE;
    }
}

/// `b.a(Graphics)` (b.java:3121) — the dialogue box: the frame pieces
/// (group 51 cap, 52 tiles, 50 cap along the top), the visible window of
/// wrapped lines (small bold, white; the first line's speaker prefix in
/// dark red), the up (54) / down (53) scroll arrows. Recomputes `shown_all`
/// (`var_boolean_r`) — a paint side effect, faithful.
fn draw_dialogue(
    fb: &mut Fb,
    assets: &Assets,
    models: &mut ModelCache,
    masks: &TextMasks,
    world: &mut World,
    ui_model: &str,
) {
    let model = models.get(ui_model);
    let (ucml, uanim) = (model.cml.clone(), model.anim.clone());
    let (w51, h51) = frame_size_of(&ucml, &uanim, 51);
    let w52 = frame_size_of(&ucml, &uanim, 52).0;
    let w50 = frame_size_of(&ucml, &uanim, 50).0;
    let w54 = frame_size_of(&ucml, &uanim, 54).0;
    let (w53, h53) = frame_size_of(&ucml, &uanim, 53);
    let var_int_r = SCREEN_W - 10;
    let (var_int_s, var_int_t) = (12, 7); // field initializers
    let small_h = masks.metrics(GameFont::SmallBold).midp_height;

    draw_model_frame(fb, assets, &ucml, &uanim, 51, 5, 5);
    let tiles = (var_int_r - 5 - w51 - w50) / w52;
    let mut n3 = 0;
    while n3 <= tiles {
        draw_model_frame(fb, assets, &ucml, &uanim, 52, 5 + w51 + n3 * w52, 5);
        n3 += 1;
    }
    draw_model_frame(fb, assets, &ucml, &uanim, 50, 5 + w51 + n3 * w52, 5);
    let n4 = n3;

    let Some(d) = world.dialogue.as_mut() else {
        return;
    };
    let window_h = d.window_h;
    let mut first_visible = false;
    let mut bl2 = true;
    let mut n2 = var_int_t;
    let mut idx = 0usize;
    while idx < d.lines.len() && bl2 {
        let line = &d.lines[idx];
        if n2 - d.scroll >= var_int_t && n2 - d.scroll <= var_int_t + window_h - small_h {
            first_visible |= idx == 0;
            let speaker_prefix = world
                .speaker
                .as_ref()
                .filter(|f| idx == 0 && line.starts_with(f.as_str()));
            if let Some(f) = speaker_prefix {
                let rest = &line[f.len() + 2..];
                let prefix = format!("{f}: ");
                masks.stamp(
                    fb,
                    GameFont::SmallBold,
                    &prefix,
                    var_int_s,
                    n2 - d.scroll,
                    0x66_0000,
                );
                let pw = masks.substring_width(GameFont::SmallBold, &prefix);
                masks.stamp(
                    fb,
                    GameFont::SmallBold,
                    rest,
                    var_int_s + pw,
                    n2 - d.scroll,
                    0xFF_FF_FF,
                );
            } else {
                masks.stamp(
                    fb,
                    GameFont::SmallBold,
                    line,
                    var_int_s,
                    n2 - d.scroll,
                    0xFF_FF_FF,
                );
            }
        }
        n2 += small_h + 1;
        bl2 = n2 - d.scroll < var_int_t + window_h - small_h - 1;
        idx += 1;
    }
    d.shown_all = idx == d.lines.len() && bl2; // var_boolean_r
    let shown_all = d.shown_all;
    if !first_visible {
        draw_model_frame(
            fb,
            assets,
            &ucml,
            &uanim,
            54,
            5 + w51 + n4 * w52 - w54 + 3,
            8,
        );
    }
    if !shown_all {
        draw_model_frame(
            fb,
            assets,
            &ucml,
            &uanim,
            53,
            5 + w51 + n4 * w52 - w53 + 3,
            5 + h51 - h53 - 3,
        );
    }
}

/// `b.paint` case 0 — the gameplay frame.
#[allow(clippy::too_many_arguments)]
pub fn paint_gameplay(
    fb: &mut Fb,
    world: &mut World,
    models: &mut ModelCache,
    assets: &Assets,
    masks: &TextMasks,
    lang: &Lang,
    level_model: &str,
    ui_model: &str,
    level_bg: u32,
) {
    // r()/q() run in the shell's per-frame paint-state pass (idempotent at a
    // settled shot); the base render is deferred here.
    base_map(world, models, assets, level_model, level_bg);
    // drawImage(offscreen, 0, 0)
    if let Some(cache) = &world.base_cache {
        for y in 0..SCREEN_H {
            for x in 0..SCREEN_W {
                fb.set(x, y, cache.get(x, y));
            }
        }
    }
    // The top layer + actors, interleaved per visible cell.
    if !world.layers.is_empty() {
        let top = world.layers.len() - 1;
        let mut n = 0i32; // the persisting frame-height cull term (faithful)
        let mut x = world.range_lo[0];
        while x <= world.range_hi[0] && x < world.map_w {
            let mut y = world.range_lo[1];
            while y <= world.range_hi[1] && y < world.map_h {
                if x < 0 || y < 0 {
                    y += 1;
                    continue;
                }
                let tile = world.layers[top][(x * world.map_h + y) as usize];
                let (ix, iy) = cell_iso(x, y);
                let n2 = ix + world.view[0];
                let n3 = iy + world.view[1];
                if tile != 0 {
                    let m = models.get(level_model);
                    n = frame_size_of(&m.cml, &m.anim, i32::from(tile)).1;
                }
                if !(n2 <= -TILE_W || n2 >= SCREEN_W || n3 <= -TILE_H || n3 >= SCREEN_H + n) {
                    if tile != 0 {
                        let m = models.get(level_model);
                        let (c, a) = (m.cml.clone(), m.anim.clone());
                        draw_model_frame(fb, assets, &c, &a, i32::from(tile), n2, n3);
                    }
                    for n6 in 0..world.actors.len() {
                        let hit = world.actors[n6].as_ref().is_some_and(|a| {
                            i32::from(a.var_byte_arr_a[0]) == x
                                && i32::from(a.var_byte_arr_a[1]) == y
                        });
                        if hit {
                            let view = world.view;
                            let mut a = world.actors[n6].take().unwrap();
                            let anim = world.actor_anims[n6]
                                .take()
                                .expect("live actor has an anim instance");
                            let cml = models.get(&a.model_name).cml.clone();
                            draw_actor(fb, assets, &cml, &anim, masks, lang, &mut a, view);
                            world.actor_anims[n6] = Some(anim);
                            world.actors[n6] = Some(a);
                        }
                    }
                }
                y += 1;
            }
            x += 1;
        }
    }
    // The HUD block (gated on var_boolean_e + a live player object).
    if world.hud_enabled && world.actors[0].is_some() {
        let (alive, hp, hpmax, fat, fatmax, icon_v, icon_w) = {
            let p = world.actors[0].as_ref().unwrap();
            (
                p.var_byte_q == 0,
                i32::from(p.var_short_q),
                i32::from(p.var_short_o),
                i32::from(p.var_short_r),
                i32::from(p.var_short_p),
                p.var_byte_v,
                p.var_byte_w,
            )
        };
        if alive {
            let n7 = 70.min(70 * hp / hpmax);
            let n8 = 70.min(70 * fat / fatmax);
            fb.fill_rect(18, 10, n7, 7, 0xFF_0000);
            fb.fill_rect(18, 18, n8, 7, 0x00_00FF);
        }
        {
            let m = models.get(level_model);
            let (c, a) = (m.cml.clone(), m.anim.clone());
            draw_model_frame(fb, assets, &c, &a, -56, 0, 0); // the HUD frame
        }
        if icon_v != -1 {
            let ui = models.get(ui_model);
            let (c, a) = (ui.cml.clone(), ui.anim.clone());
            let vw = frame_size_of(&c, &a, i32::from(icon_v)).0;
            draw_model_frame(fb, assets, &c, &a, i32::from(icon_v), SCREEN_W - vw - 2, 2);
            if icon_w != -1 {
                // NOTE: the x uses the v icon's width doubled (faithful).
                draw_model_frame(
                    fb,
                    assets,
                    &c,
                    &a,
                    i32::from(icon_w),
                    SCREEN_W - (vw << 1) - 4,
                    2,
                );
            }
        }
    }
    // Soft-key labels (small bold, white; in the clipped 320..345 band).
    let small_h = masks.metrics(GameFont::SmallBold).midp_height;
    let menu = lang.get(422).to_uppercase();
    masks.stamp(
        fb,
        GameFont::SmallBold,
        &menu,
        2,
        SCREEN_H - small_h - 2,
        0xFF_FF_FF,
    );
    if world.hud_enabled {
        let inv = lang.get(421).to_uppercase();
        let w = masks.string_width(GameFont::SmallBold, lang.get(421));
        // Faithful: the x measures the PRE-uppercase string (like the exit
        // dialog's YES quirk — `stringWidth(lang421)` then draws uppercase).
        masks.stamp(
            fb,
            GameFont::SmallBold,
            &inv,
            SCREEN_W - w - 2,
            SCREEN_H - small_h - 2,
            0xFF_FF_FF,
        );
    }
    // The HUD floating-text band (medium font).
    if let Some(h) = world.hud.as_mut() {
        let med_h = masks.metrics(GameFont::MediumPlain).midp_height;
        let var_int_h = SCREEN_H - med_h - 5;
        h.y = var_int_h;
        fb.fill_rect(
            0,
            var_int_h - 5,
            SCREEN_W,
            SCREEN_H - (var_int_h - 5),
            0x00_0000,
        );
        if !h.blink_hidden {
            if h.x == -1 {
                h.x = match h.style {
                    0 | 1 => {
                        (SCREEN_W >> 1) - (masks.string_width(GameFont::MediumPlain, &h.text) >> 1)
                    }
                    2 => -masks.string_width(GameFont::MediumPlain, &h.text),
                    3 => SCREEN_W,
                    _ => h.x,
                };
            }
            let (x, color, text) = (h.x, h.color as u32, h.text.clone());
            masks.stamp(fb, GameFont::MediumPlain, &text, x, var_int_h, color);
        }
    }
    draw_effects(fb, assets, models, world);
    // The dialogue box (f.a:B == 0 always inside the slice).
    if world.dialogue.is_some() {
        draw_dialogue(fb, assets, models, masks, world, ui_model);
    }
}

/// `b.paint` case 15 — the please-wait screen: black fill, white LARGE BOLD
/// literal "Please Wait..." centered on (120, 172), the oh_pc group-5 anim
/// centered below.
pub fn paint_please_wait(fb: &mut Fb, masks: &TextMasks, models: &mut ModelCache, assets: &Assets) {
    fb.fill(0x00_0000);
    let s = "Please Wait...";
    let w = masks.string_width(GameFont::LargeBold, s);
    let fh = masks.metrics(GameFont::LargeBold).midp_height;
    masks.stamp(
        fb,
        GameFont::LargeBold,
        s,
        (SCREEN_W >> 1) - (w >> 1),
        (SCREEN_H >> 1) - (fh >> 1),
        0xFF_FF_FF,
    );
    let pc = models.get("/oh_pc.cml");
    let (c, a) = (pc.cml.clone(), pc.anim.clone());
    let aw = frame_size_of(&c, &a, 5).0;
    draw_model_frame(
        fb,
        assets,
        &c,
        &a,
        5,
        (SCREEN_W >> 1) - (aw >> 1),
        (SCREEN_H >> 1) + fh,
    );
}

/// Full-level map render — a DEBUG/ATLAS tool, not a gated paint. Composes
/// the same validated draw calls as `base_map` + `paint_gameplay`'s
/// top-layer/actor interleave, but over the ENTIRE map bounds instead of
/// the 240x320 viewport (UESP-style whole-level images; the `levelmap`
/// bin). Read-only over world state apart from the actor take/put the
/// interleave shares with the gated path.
#[allow(clippy::too_many_arguments)]
pub fn render_full_map(
    world: &mut World,
    models: &mut ModelCache,
    assets: &Assets,
    masks: &TextMasks,
    lang: &Lang,
    level_model: &str,
    level_bg: u32,
    with_actors: bool,
) -> Fb {
    // Canvas bounds over every cell's iso anchor; generous top pad for tall
    // wall frames (they extend above the anchor), one tile all around.
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for x in 0..world.map_w {
        for y in 0..world.map_h {
            let (ix, iy) = cell_iso(x, y);
            min_x = min_x.min(ix);
            min_y = min_y.min(iy);
            max_x = max_x.max(ix);
            max_y = max_y.max(iy);
        }
    }
    let (pad_l, pad_t, pad_r, pad_b) = (TILE_W, TILE_H * 8, TILE_W * 2, TILE_H * 6);
    let img_w = max_x - min_x + TILE_W + pad_l + pad_r;
    let img_h = max_y - min_y + TILE_H + pad_t + pad_b;
    let view = [pad_l - min_x, pad_t - min_y];
    let mut fb = Fb::new(img_w, img_h);
    fb.fill(level_bg);

    // The base layers (base_map's loop over layers 0..len-1, full range).
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
    // The top layer + actors, interleaved per cell (paint_gameplay's order).
    if !world.layers.is_empty() {
        let top = world.layers.len() - 1;
        for x in 0..world.map_w {
            for y in 0..world.map_h {
                let tile = world.layers[top][(x * world.map_h + y) as usize];
                let (ix, iy) = cell_iso(x, y);
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
                if !with_actors {
                    continue;
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
