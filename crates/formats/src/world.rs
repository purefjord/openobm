//! Map collision detection — `b.java`'s per-actor collision sampling
//! (`h.java::boolean_a(j)` + its `(j, byte)` helper).
//!
//! An actor samples three tile cells (its collision-box corners). A cell blocks
//! depending on its collision-layer value: `0` = open, `1` = solid, and `2..=5`
//! are directional **slope** tiles resolved against the corner's sub-tile position
//! (`world % 128`). Out-of-bounds samples count as blocked.

use crate::iso::{screen_to_world, Vec2i};
use crate::Actor;

/// `h.void_a(j, direction, dt)` — the per-frame movement step ("moveInWorld").
/// Accumulates the step timer by `dt`; once it passes 50, advances the actor by a
/// speed (`var_short_w` scaled by the elapsed time) in `direction` (1=+y/down,
/// 2=-y/up, 3=+x/right, 4=-x/left), reverting to the prior position if the move
/// collides. A no-op until the timer threshold is crossed.
pub fn move_in_world(a: &mut Actor, direction: i32, dt: i64, map: &[i8], width: i32, height: i32) {
    a.var_short_g = (i64::from(a.var_short_g) + dt) as i16;
    if a.var_short_g <= 50 {
        return;
    }
    if a.var_short_g > 400 {
        a.var_short_g = 50;
    }
    let speed = i32::from(a.var_short_w) / (1000 / i32::from(a.var_short_g));
    match direction {
        2 => apply_delta(a, 0, -speed),
        1 => apply_delta(a, 0, speed),
        4 => apply_delta(a, -speed, 0),
        3 => apply_delta(a, speed, 0),
        _ => {}
    }
    if collides(a, map, width, height) {
        // h.void_c: revert to the pre-step position.
        let (ex, ey) = (a.var_int_arr_e[0], a.var_int_arr_e[1]);
        set_position(a, ex, ey);
    }
    a.var_short_g = 0;
}

/// `h.d(j, n, n2)` — apply a world-space delta to the position and both collision
/// corners, refresh the derived iso/tile coords, set facing + the walk-anim timer.
fn apply_delta(a: &mut Actor, dx: i32, dy: i32) {
    a.var_int_arr_e = a.var_int_arr_b;
    a.var_int_arr_b = [a.var_int_arr_b[0] + dx, a.var_int_arr_b[1] + dy];
    a.var_int_arr_c = [a.var_int_arr_c[0] + dx, a.var_int_arr_c[1] + dy];
    a.var_int_arr_d = [a.var_int_arr_d[0] + dx, a.var_int_arr_d[1] + dy];
    recompute_iso(a);
    recompute_tiles(a);
    if dx > 0 {
        a.var_byte_d = 3;
    } else if dx < 0 {
        a.var_byte_d = 4;
    } else if dy > 0 {
        a.var_byte_d = 1;
    } else if dy < 0 {
        a.var_byte_d = 2;
    }
    a.var_short_a = 500;
}

/// `h.a(j, n, n2)` — set the position absolutely and rebuild both collision corners
/// from the box half-extents (`var_byte_a`/`_b` via the inverse iso transform),
/// then the iso and tile coords.
pub fn set_position(a: &mut Actor, n: i32, n2: i32) {
    let c = screen_to_world(Vec2i::new(i32::from(a.var_byte_b), 0));
    let d = screen_to_world(Vec2i::new(i32::from(a.var_byte_a), 0));
    a.var_int_arr_b = [n, n2];
    a.var_int_arr_c = [n + c.x, n2 + c.y];
    a.var_int_arr_d = [n + d.x, n2 + d.y];
    recompute_iso(a);
    recompute_tiles(a);
}

/// `h.d(j)` — iso/screen position from world position.
fn recompute_iso(a: &mut Actor) {
    a.var_int_arr_i = [
        (a.var_int_arr_b[0] - a.var_int_arr_b[1]) >> 3,
        (a.var_int_arr_b[0] + a.var_int_arr_b[1]) >> 4,
    ];
}

/// `h.void_b(j)` — the three corners' tile coordinates (world `>> 7`), then `h.e`.
fn recompute_tiles(a: &mut Actor) {
    a.var_byte_arr_b = [
        (a.var_int_arr_b[0] >> 7) as i8,
        (a.var_int_arr_b[1] >> 7) as i8,
    ];
    a.var_byte_arr_c = [
        (a.var_int_arr_c[0] >> 7) as i8,
        (a.var_int_arr_c[1] >> 7) as i8,
    ];
    a.var_byte_arr_d = [
        (a.var_int_arr_d[0] >> 7) as i8,
        (a.var_int_arr_d[1] >> 7) as i8,
    ];
    pick_primary(a);
}

/// `h.e(j)` — choose `var_byte_arr_a` (the draw-order sample) among the corners.
fn pick_primary(a: &mut Actor) {
    if a.var_byte_arr_d[0] > a.var_byte_arr_c[0] {
        a.var_byte_arr_a = a.var_byte_arr_d;
    } else if a.var_byte_arr_c[1] > a.var_byte_arr_d[1] || a.var_byte_arr_c[0] > a.var_byte_arr_b[0]
    {
        a.var_byte_arr_a = a.var_byte_arr_c;
    } else {
        a.var_byte_arr_a = a.var_byte_arr_b;
    }
}

/// `h.boolean_a(j)` — true if the actor collides with the map at any of its three
/// sampled cells. `map` is the collision layer (`b.var_byte_arr_a`, signed bytes);
/// `width`/`height` are the map dims (`b.var_byte_g`/`b.var_byte_f`).
pub fn collides(a: &Actor, map: &[i8], width: i32, height: i32) -> bool {
    if a.var_byte_p == 0 {
        return false;
    }
    // Outer bounds checks (note: only these specific corners/axes are guarded,
    // exactly as the original).
    if a.var_byte_arr_b[0] < 0
        || i32::from(a.var_byte_arr_b[1]) >= width
        || i32::from(a.var_byte_arr_d[0]) >= height
        || a.var_byte_arr_d[1] < 0
    {
        return true;
    }
    cell_blocks(map, width, a.var_byte_arr_b, a.var_int_arr_b)
        || cell_blocks(map, width, a.var_byte_arr_c, a.var_int_arr_c)
        || cell_blocks(map, width, a.var_byte_arr_d, a.var_int_arr_d)
}

/// One sampled cell (`h.boolean_a(j, by)`): `tile` is the cell `[row, col]`,
/// `world` the corner's world position whose low 7 bits pick the slope side.
fn cell_blocks(map: &[i8], width: i32, tile: [i8; 2], world: [i32; 2]) -> bool {
    let idx = i32::from(tile[0]) * width + i32::from(tile[1]);
    // The original tests `> length` (an exact-length index would throw in Java);
    // valid play never reaches it, so we mirror the bound and index in range.
    if idx > map.len() as i32 {
        return true;
    }
    let n = map[idx as usize];
    if n == 0 {
        return false;
    }
    let n2 = world[0] % 128;
    let n3 = world[1] % 128;
    match n {
        1 => true,
        4 => n3 <= n2,
        3 => n3 >= n2,
        2 => n2 <= n3,
        5 => n2 >= n3,
        _ => false,
    }
}
